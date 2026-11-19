use crate::assets::{self, AssetServer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ElementBounds {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub parent_scale: f32,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub title: String,
    pub kind: String,
    pub opened: bool,
    pub loaded: bool,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ViewInfo {
    #[serde(default)]
    pub applied: Value,
    pub id: String,
    pub url: String,
    pub error: String,
    pub styles: u32,
    pub rules: u32,
    pub pending: u32,
    pub failed: u32,
    pub windows: u32,
    pub enabled: bool,
    pub radar: bool,
    pub safety: bool,
    pub chat: bool,
    pub radar_custom: bool,
    pub safety_custom: bool,
    pub chat_custom: bool,
    pub interfaces: Vec<InterfaceInfo>,
    pub width: f32,
    pub height: f32,
    pub elements: Vec<ElementBounds>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct BridgeStatus {
    pub port: u16,
    pub error: String,
    pub views: Vec<ViewInfo>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VehicleState {
    pub visible: bool,
    pub engine: bool,
    pub lights: bool,
    pub belt: bool,
    pub locked: bool,
    pub alarm: bool,
    pub left: bool,
    pub right: bool,
    pub has_key: bool,
    pub speed: f32,
    pub fuel: f32,
    pub max_fuel: f32,
    pub mileage: String,
    pub received: u64,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerHudState {
    pub visible: bool,
    pub bars: bool,
    pub in_water: bool,
    pub weapon: i32,
    pub ammo: i32,
    pub total_ammo: i32,
    pub wanted: i32,
    pub server: i32,
    pub money: i64,
    pub health: f32,
    pub armour: f32,
    pub hunger: f32,
    pub air: f32,
    pub received: u64,
}

pub fn ticks() -> u64 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount64() -> u64;
    }
    unsafe { GetTickCount64() }
}

pub fn owned_ports(pid: u32) -> Vec<u16> {
    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetExtendedTcpTable(
            table: *mut std::ffi::c_void,
            size: *mut u32,
            order: i32,
            family: u32,
            class: u32,
            reserved: u32,
        ) -> u32;
    }
    let mut size = 0_u32;
    if unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, 2, 3, 0) } != 122
        || !(4..=16_777_216).contains(&size)
    {
        return Vec::new();
    }
    let mut memory = vec![0_u8; size as usize];
    if unsafe { GetExtendedTcpTable(memory.as_mut_ptr().cast(), &mut size, 0, 2, 3, 0) } != 0 {
        return Vec::new();
    }
    let Some(head) = memory.get(..4) else {
        return Vec::new();
    };
    let count = u32::from_ne_bytes(head.try_into().unwrap()) as usize;
    let mut ports = BTreeSet::new();
    for row in memory[4..].chunks_exact(24).take(count) {
        let owner = u32::from_ne_bytes(row[20..24].try_into().unwrap());
        if owner == pid {
            ports.insert(u16::from_be(
                u32::from_ne_bytes(row[8..12].try_into().unwrap()) as u16,
            ));
        }
    }
    let mut output: Vec<_> = ports.into_iter().collect();
    if let Some(index) = output.iter().position(|v| *v == 9444) {
        output[..=index].rotate_right(1);
    }
    output
}

pub fn http_get(port: u16, path: &str) -> Result<String, String> {
    if !path.starts_with('/') || path.contains(['\r', '\n']) {
        return Err("Invalid inspector HTTP path".into());
    }
    let mut socket = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(450),
    )
    .map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(1000)))
        .map_err(|e| e.to_string())?;
    socket
        .set_write_timeout(Some(Duration::from_millis(1000)))
        .map_err(|e| e.to_string())?;
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    let mut buffer = [0_u8; 16384];
    let mut body_offset = None;
    let mut length = None;
    let mut chunked = false;
    loop {
        match socket.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buffer[..n]),
            Err(e) => return Err(e.to_string()),
        }
        if data.len() > 8_388_608 {
            return Err("Inspector response exceeds 8 MiB".into());
        }
        if body_offset.is_none() {
            if let Some(offset) = data.windows(4).position(|v| v == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..offset]);
                if headers.split_whitespace().nth(1) != Some("200") {
                    return Err(format!(
                        "Inspector HTTP: {}",
                        headers.lines().next().unwrap_or("invalid response")
                    ));
                }
                for line in headers.lines().skip(1) {
                    if let Some((key, value)) = line.split_once(':') {
                        if key.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse::<usize>().ok();
                        }
                        if key.eq_ignore_ascii_case("transfer-encoding") {
                            chunked = value.to_lowercase().contains("chunked");
                        }
                    }
                }
                body_offset = Some(offset + 4);
            }
        }
        if let Some(offset) = body_offset {
            if length.is_some_and(|n| data.len() - offset >= n)
                || (chunked && data.ends_with(b"\r\n0\r\n\r\n"))
            {
                break;
            }
        }
    }
    let offset = body_offset.ok_or("Invalid inspector HTTP response")?;
    let mut body = &data[offset..];
    let result = if chunked {
        let mut decoded = Vec::new();
        loop {
            let end = body
                .windows(2)
                .position(|v| v == b"\r\n")
                .ok_or("Invalid HTTP chunk")?;
            let size = usize::from_str_radix(
                std::str::from_utf8(&body[..end])
                    .map_err(|e| e.to_string())?
                    .split(';')
                    .next()
                    .unwrap_or(""),
                16,
            )
            .map_err(|e| e.to_string())?;
            if size == 0 {
                break;
            }
            let chunk = body
                .get(end + 2..end + 2 + size)
                .ok_or("Truncated HTTP chunk")?;
            decoded.extend_from_slice(chunk);
            body = body
                .get(end + 2 + size + 2..)
                .ok_or("Truncated HTTP chunk")?;
        }
        decoded
    } else {
        if length.is_some_and(|n| body.len() < n) {
            return Err("Truncated inspector HTTP response".into());
        }
        body[..length.unwrap_or(body.len())].to_vec()
    };
    String::from_utf8(result).map_err(|e| e.to_string())
}

struct Channel {
    socket: WebSocket<TcpStream>,
    sequence: u64,
    registered_fonts: BTreeSet<usize>,
    configuration: String,
    id: String,
    url: String,
}

impl Channel {
    fn new(port: u16, id: &str, url: &str) -> Result<Self, String> {
        if id.is_empty() || id.len() > 128 {
            return Err("Invalid inspector view ID".into());
        }
        let encoded: String = id
            .bytes()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' {
                    char::from(c).to_string()
                } else {
                    format!("%{c:02X}")
                }
            })
            .collect();
        let stream = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            Duration::from_millis(1000),
        )
        .map_err(|e| e.to_string())?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(1500)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_millis(1500)))
            .map_err(|e| e.to_string())?;
        let (socket, _) = tungstenite::client(
            format!("ws://127.0.0.1:{port}/devtools/page/{encoded}"),
            stream,
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            socket,
            sequence: 0,
            registered_fonts: BTreeSet::new(),
            configuration: String::new(),
            id: id.into(),
            url: url.into(),
        })
    }

    fn call(
        &mut self,
        method: &str,
        params: Value,
        cancelled: &AtomicBool,
    ) -> Result<Value, String> {
        if cancelled.load(Ordering::Acquire) {
            return Err("Inspector command cancelled".into());
        }
        self.sequence = self.sequence.wrapping_add(1);
        let sequence = self.sequence;
        let request = json!({"id": sequence, "method": method, "params": params}).to_string();
        if request.len() > 16_777_216 {
            return Err("Inspector command exceeds 16 MiB".into());
        }
        self.socket
            .send(Message::Text(request.into()))
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("Inspector command cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("Inspector command timeout".into());
            }
            let message = match self.socket.read() {
                Ok(message) => message,
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            };
            let text = match message {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => {
                    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())?
                }
                Message::Close(_) => return Err("Inspector disconnected".into()),
                _ => continue,
            };
            if text.len() > 16_777_216 {
                return Err("Inspector response exceeds 16 MiB".into());
            }
            let response: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if response["id"].as_u64() != Some(sequence) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(error["message"]
                    .as_str()
                    .unwrap_or("Inspector error")
                    .into());
            }
            return Ok(response["result"].clone());
        }
    }

    fn evaluate(&mut self, expression: &str, cancelled: &AtomicBool) -> Result<Value, String> {
        let response = self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true}),
            cancelled,
        )?;
        if let Some(details) = response.get("exceptionDetails") {
            return Err(details["exception"]["description"]
                .as_str()
                .or_else(|| details["text"].as_str())
                .unwrap_or("UI script failed")
                .into());
        }
        Ok(response["result"]
            .get("value")
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or_else(|| json!({})))
    }
}

pub struct Bridge {
    pid: u32,
    state: BridgeStatus,
    channels: BTreeMap<String, Channel>,
    fonts: BTreeMap<usize, String>,
    assets: AssetServer,
    vehicle_state: VehicleState,
    player_state: PlayerHudState,
    rediscover: bool,
    cancelled: Arc<AtomicBool>,
    preview_kind: Option<i32>,
    last_discovery: u64,
    last_poll: u64,
    revision: u64,
    custom_logo: bool,
    port: u16,
}

impl Bridge {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            state: BridgeStatus::default(),
            channels: BTreeMap::new(),
            fonts: BTreeMap::new(),
            assets: AssetServer::new(),
            vehicle_state: VehicleState::default(),
            player_state: PlayerHudState::default(),
            rediscover: true,
            cancelled: Arc::new(AtomicBool::new(false)),
            preview_kind: None,
            last_discovery: 0,
            last_poll: 0,
            revision: 0,
            custom_logo: false,
            port: 0,
        }
    }

    pub fn status(&self) -> BridgeStatus {
        self.state.clone()
    }
    pub fn vehicle(&self) -> VehicleState {
        self.vehicle_state.clone()
    }
    pub fn player(&self) -> PlayerHudState {
        self.player_state.clone()
    }
    pub fn refresh(&mut self) {
        self.rediscover = true;
    }
    pub fn preview(&mut self, kind: i32) {
        self.preview_kind = Some(kind);
    }
    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    pub fn tick(&mut self, config: &Value, revision: u64, dpi: f32, custom_logo: bool) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let now = ticks();
        if now.saturating_sub(self.last_poll) < 1000
            && self.revision == revision
            && self.custom_logo == custom_logo
            && !self.rediscover
            && self.preview_kind.is_none()
        {
            if self.poll_vehicle(config).is_err() {
                self.vehicle_state.visible = false;
                self.player_state.visible = false;
            }
            return;
        }
        self.last_poll = now;
        self.revision = revision;
        self.custom_logo = custom_logo;
        if let Err(error) = self.update(config, revision, dpi, custom_logo, now) {
            if self.cancelled.load(Ordering::Acquire) {
                return;
            }
            self.channels.clear();
            self.port = 0;
            self.state = BridgeStatus {
                error,
                ..Default::default()
            };
            self.vehicle_state.visible = false;
            self.player_state.visible = false;
        }
    }

    fn update(
        &mut self,
        config: &Value,
        _revision: u64,
        dpi: f32,
        custom_logo: bool,
        now: u64,
    ) -> Result<(), String> {
        if self.rediscover || self.port == 0 || now.saturating_sub(self.last_discovery) >= 3000 {
            self.rediscover = false;
            self.last_discovery = now;
            let ports = owned_ports(self.pid);
            if !ports.contains(&self.port) {
                self.channels.clear();
                self.port = 0;
            }
            if self.port == 0 {
                for port in ports {
                    if let Ok(response) = http_get(port, "/json/list") {
                        let gameface_targets = serde_json::from_str::<Value>(&response)
                            .ok()
                            .is_some_and(|targets| {
                                targets.as_array().is_some_and(|targets| {
                                    targets.iter().any(|target| {
                                        target["url"]
                                            .as_str()
                                            .is_some_and(|url| url.starts_with("coui://"))
                                            && target["id"]
                                                .as_str()
                                                .is_some_and(|id| !id.is_empty())
                                    })
                                })
                            });
                        if gameface_targets {
                            self.port = port;
                            break;
                        }
                    }
                    if let Ok(response) = http_get(port, "/json/version") {
                        if serde_json::from_str::<Value>(&response)
                            .ok()
                            .and_then(|v| v["Browser"].as_str().map(|s| s.starts_with("Cohtml/")))
                            .unwrap_or(false)
                        {
                            self.port = port;
                            break;
                        }
                    }
                }
            }
            if self.port == 0 {
                return Err("В процессе игры не найден инспектор Gameface. Включите отладчик интерфейса клиента.".into());
            }
            let targets: Value = serde_json::from_str(&http_get(self.port, "/json/list")?)
                .map_err(|e| e.to_string())?;
            let targets = targets
                .as_array()
                .ok_or("Inspector target list is invalid")?;
            let mut current = BTreeSet::new();
            for target in targets {
                let url = target["url"].as_str().unwrap_or("");
                if !url.starts_with("coui://") {
                    continue;
                }
                let Some(id) = target["id"].as_str() else {
                    continue;
                };
                current.insert(id.to_string());
                if !self.channels.contains_key(id) {
                    self.channels
                        .insert(id.to_string(), Channel::new(self.port, id, url)?);
                }
            }
            self.channels.retain(|id, _| current.contains(id));
            if self.channels.is_empty() {
                return Err(
                    "Инспектор Gameface работает, игровые интерфейсы ещё не загружены".into(),
                );
            }
        }
        let mut browser_config = config.clone();
        if let Some(root) = browser_config.as_object_mut() {
            for key in ["weaponImages", "statusIcons", "logoImages"] {
                root.remove(key);
            }
        }
        if !browser_config["widgets"].is_object() {
            browser_config["widgets"] = json!({});
        }
        let widgets = browser_config["widgets"]
            .as_object_mut()
            .ok_or("Invalid widget configuration")?;
        widgets.remove("clock");
        widgets.insert("nativeDpi".into(), json!(dpi));
        widgets.entry("logo").or_insert_with(|| json!({}))["customActive"] = json!(custom_logo);
        let widgets_json = Value::Object(widgets.clone()).to_string();
        let configuration = browser_config.to_string();
        let preview = self.preview_kind.take();
        let mut wanted = BTreeSet::from([config["fontFamily"].as_u64().unwrap_or(0) as usize]);
        for group in ["chat", "safety", "gametext", "logo"] {
            let widget = &config["widgets"][group];
            if widget["enabled"].as_bool().unwrap_or(false)
                || widget["style"].as_u64().unwrap_or(0) > 0
            {
                wanted.insert(widget["font"].as_u64().unwrap_or(0) as usize);
                if group == "gametext" {
                    wanted.insert(widget["moneyFont"].as_u64().unwrap_or(5) as usize);
                }
            }
        }
        let mut views = Vec::new();
        for view in self.channels.values_mut() {
            let changed = view.configuration != configuration;
            let mut response = view.evaluate(
                "(window.__oldDialogsHassle?window.__oldDialogsHassle.status():null)",
                &self.cancelled,
            )?;
            if changed
                || response["version"].as_u64() != Some(11)
                || response["leaseExpired"].as_bool() == Some(true)
            {
                if response["version"].as_u64() != Some(11) {
                    view.registered_fonts.clear();
                    view.evaluate(&format!("(function(){{const api={};const speedo={};const player={};const logo={};logo.configure(null,null,1,{});const hud={};const layout={};return api.status();}})()", assets::RUNTIME, assets::SPEEDOMETER, assets::PLAYER_HUD, assets::LOGO, json!(assets::LOGO_CSS), assets::HUD, assets::LAYOUT), &self.cancelled)?;
                }
                for family in &wanted {
                    if view.registered_fonts.contains(family) {
                        continue;
                    }
                    if !self.fonts.contains_key(family) {
                        let mut css = String::new();
                        for (weight, bold) in [(400, false), (700, true)] {
                            let url = self.assets.publish(
                                &format!("fonts/{family}/{}.ttf", usize::from(bold)),
                                assets::font_bytes(*family, bold)?,
                                "font/ttf",
                            )?;
                            css.push_str(&format!("@font-face{{font-family:OldDialogsLocal11_{family};font-style:normal;font-weight:{weight};src:url(\"{url}\") format(\"truetype\");}}"));
                        }
                        self.fonts.insert(*family, css);
                    }
                    let css = &self.fonts[family];
                    view.evaluate(
                        &format!(
                            "window.__oldDialogsHassle.fontChunk({family},{},true,true)",
                            json!(css)
                        ),
                        &self.cancelled,
                    )?;
                    view.registered_fonts.insert(*family);
                }
                response = view.evaluate(&format!("(function(){{const api=window.__oldDialogsHassle;const hud=window.__oldDialogsHud;api.configure({configuration},{});hud.configure({widgets_json});window.__oldDialogsLayout.configure({}, {dpi}, {widgets_json});return api.status();}})()", json!(assets::THEME_CSS), browser_config["layouts"].as_object().map(|v| json!(v)).unwrap_or_else(|| json!({}))), &self.cancelled)?;
                view.configuration = configuration.clone();
            }
            if let Some(kind) = preview {
                view.evaluate(
                    &format!("window.__oldDialogsHud.previewMessage({kind})"),
                    &self.cancelled,
                )?;
            }
            views.push(read_status(view, &response));
        }
        self.poll_vehicle(config)?;
        self.state = BridgeStatus {
            port: self.port,
            error: String::new(),
            views,
        };
        Ok(())
    }

    fn poll_vehicle(&mut self, config: &Value) -> Result<(), String> {
        let mut vehicle = VehicleState {
            max_fuel: 100.0,
            ..Default::default()
        };
        let mut player = PlayerHudState {
            air: 100.0,
            ..Default::default()
        };
        if let Some(view) = self.channels.values_mut().next() {
            if config["widgets"]["speedometer"]["style"]
                .as_u64()
                .unwrap_or(0)
                > 0
            {
                let value = view.evaluate("(window.__oldDialogsSpeedometer?window.__oldDialogsSpeedometer.read():{visible:false})", &self.cancelled)?;
                vehicle = VehicleState {
                    visible: boolean(&value, "visible"),
                    engine: boolean(&value, "engine"),
                    lights: boolean(&value, "lights"),
                    belt: boolean(&value, "belt"),
                    locked: boolean(&value, "locked"),
                    alarm: boolean(&value, "alarm"),
                    left: boolean(&value, "left"),
                    right: boolean(&value, "right"),
                    has_key: boolean(&value, "key"),
                    speed: number(&value, "speed"),
                    fuel: number(&value, "fuel"),
                    max_fuel: value["maxFuel"].as_f64().unwrap_or(100.0) as f32,
                    mileage: value["mileage"].as_str().unwrap_or("000000").into(),
                    received: ticks(),
                };
            }
            if ["playerHud", "logo"]
                .iter()
                .any(|key| config["widgets"][key]["style"].as_u64().unwrap_or(0) > 0)
            {
                let value = view.evaluate("(window.__oldDialogsPlayerHud?window.__oldDialogsPlayerHud.read():{visible:false})", &self.cancelled)?;
                let integer =
                    |key: &str, max: i64| value[key].as_i64().unwrap_or(0).clamp(0, max) as i32;
                let percent = |key: &str| number(&value, key).clamp(0.0, 100.0);
                player = PlayerHudState {
                    visible: boolean(&value, "visible"),
                    bars: boolean(&value, "bars"),
                    in_water: boolean(&value, "inWater"),
                    weapon: integer("weapon", 65535),
                    ammo: integer("ammo", 2_000_000_000),
                    total_ammo: integer("totalAmmo", 2_000_000_000),
                    wanted: integer("wanted", 6),
                    server: integer("server", 999999),
                    money: value["money"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .or_else(|| value["money"].as_i64())
                        .unwrap_or(0),
                    health: percent("health"),
                    armour: percent("armour"),
                    hunger: percent("hunger"),
                    air: percent("air"),
                    received: ticks(),
                };
            }
        }
        self.vehicle_state = vehicle;
        self.player_state = player;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.cancelled.store(false, Ordering::Release);
        for view in self.channels.values_mut() {
            let _ = view.evaluate("(function(){if(window.__oldDialogsHassle)window.__oldDialogsHassle.shutdown();return {};})()", &self.cancelled);
            let _ = view.socket.close(None);
        }
        self.channels.clear();
        self.assets.stop();
        self.fonts.clear();
        self.port = 0;
        self.state = BridgeStatus::default();
        self.vehicle_state = VehicleState::default();
        self.player_state = PlayerHudState::default();
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop();
    }
}

fn boolean(value: &Value, key: &str) -> bool {
    value[key].as_bool().unwrap_or(false)
}
fn number(value: &Value, key: &str) -> f32 {
    value[key].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0) as f32
}

fn read_status(view: &Channel, data: &Value) -> ViewInfo {
    let mut interfaces: Vec<_> = data["interfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| InterfaceInfo {
            name: v["name"].as_str().unwrap_or("").into(),
            title: v["title"].as_str().unwrap_or("").into(),
            kind: v["kind"].as_str().unwrap_or("").into(),
            opened: boolean(v, "opened"),
            loaded: boolean(v, "loaded"),
        })
        .collect();
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    ViewInfo {
        id: view.id.clone(),
        url: view.url.clone(),
        error: String::new(),
        applied: data["applied"].clone(),
        styles: data["styles"].as_u64().unwrap_or(0) as u32,
        rules: data["rules"].as_u64().unwrap_or(0) as u32,
        pending: data["pending"].as_u64().unwrap_or(0) as u32,
        failed: data["failed"].as_u64().unwrap_or(0) as u32,
        windows: data["windows"].as_u64().unwrap_or(0) as u32,
        enabled: boolean(data, "enabled"),
        radar: boolean(&data["hud"], "radar"),
        safety: boolean(&data["hud"], "safety"),
        chat: boolean(&data["hud"], "chat"),
        radar_custom: boolean(&data["hud"], "radarCustomized"),
        safety_custom: boolean(&data["hud"], "safetyCustomized"),
        chat_custom: boolean(&data["hud"], "chatCustomized"),
        interfaces,
        width: number(&data["layout"], "width"),
        height: number(&data["layout"], "height"),
        elements: data["layout"]["elements"]
            .as_array()
            .into_iter()
            .flatten()
            .take(256)
            .map(|v| ElementBounds {
                id: v["id"].as_str().unwrap_or("").into(),
                label: v["label"].as_str().unwrap_or("").into(),
                x: number(v, "x"),
                y: number(v, "y"),
                width: number(v, "width"),
                height: number(v, "height"),
                parent_scale: 1.0,
            })
            .collect(),
    }
}
