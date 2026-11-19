use crate::schema::{self, Kind};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const FILE_LIMIT: u64 = 67_108_864;
const IMAGE_LIMIT: usize = 8_388_608;
const IMAGES_LIMIT: usize = 33_554_432;

#[derive(Clone, Debug)]
pub struct Config {
    pub data: Value,
    pub font_names: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data: schema::defaults(),
            font_names: schema::FONT_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        }
    }
}

fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("Expected object: {name}"))
}

fn number(value: &Value, name: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("Invalid number: {name}"))
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("Expected text: {name}"))
}

fn normalized_color(value: &Value, fallback: &Value) -> Result<Value, String> {
    let Some(channels) = value.as_array() else {
        return Err("Expected R,G,B,A color".into());
    };
    if channels.len() != 4 {
        return Ok(fallback.clone());
    }
    channels
        .iter()
        .map(|v| number(v, "color").map(|n| json!(n.clamp(0.0, 1.0))))
        .collect()
}

fn normalize_font(
    index: i64,
    name: Option<&Value>,
    fonts: &[String],
) -> Result<(i64, String), String> {
    let maximum = fonts.len().saturating_sub(1) as i64;
    let mut index = index.clamp(0, maximum);
    let mut selected = fonts
        .get(index as usize)
        .cloned()
        .unwrap_or_else(|| "Arial".into());
    if let Some(value) = name {
        let name = text(value, "font name")?;
        if !name.is_empty() {
            selected = name.to_owned();
            if let Some(found) = fonts
                .iter()
                .position(|font| font.eq_ignore_ascii_case(name))
            {
                index = found as i64;
            }
        }
    }
    Ok((index, selected))
}

impl Config {
    pub fn load(root: &Path) -> Result<Self, String> {
        let mut result = Self::default();
        let path = root.join("settings.ini");
        if path.is_file() {
            result.read(&path)?;
        } else {
            let legacy = root.join("settings.json");
            if legacy.is_file() {
                let value: Value =
                    serde_json::from_str(&read_text(&legacy)?).map_err(|e| e.to_string())?;
                result.apply(&value)?;
                for (key, value) in [
                    ("buttonWidth", 105.0),
                    ("padding", 15.0),
                    ("radius", 0.0),
                    ("windowWidth", 231.0),
                    ("dialogWidth", 0.0),
                ] {
                    result.data["sizes"][key] = json!(value);
                }
                result.data["colors"]["overlay"] = json!([0, 0, 0, 0]);
            }
        }
        Ok(result)
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        self.write(&root.join("settings.ini"))
    }

    pub fn save_profile(&self, root: &Path) -> Result<(), String> {
        self.write(&root.join("profile.ini"))
    }

    pub fn load_profile(&mut self, root: &Path) -> Result<(), String> {
        self.read(&root.join("profile.ini"))
    }

    pub fn set_font_names(&mut self, names: &[String]) {
        if !names.is_empty() {
            self.font_names = names.to_vec();
            let current = self.data.clone();
            let _ = self.apply(&current);
        }
    }

    pub fn to_wire(&self) -> Value {
        self.data.clone()
    }

    pub fn apply(&mut self, incoming: &Value) -> Result<(), String> {
        object(incoming, "settings")?;
        let mut data = self.data.clone();
        if let Some(version) = incoming.get("version") {
            let version = number(version, "version")?;
            if ![1.0, 2.0, 3.0, 4.0].contains(&version) {
                return Err("Unsupported profile version".into());
            }
        }
        data["version"] = json!(4);
        for key in ["enabled", "bold", "preserveColors", "animations"] {
            if let Some(value) = incoming.get(key) {
                let value = value
                    .as_bool()
                    .ok_or_else(|| format!("Expected flag: {key}"))?;
                data[key] = json!(value);
            }
        }
        let font = match incoming.get("fontFamily") {
            Some(value) => number(value, "fontFamily")? as i64,
            None => data["fontFamily"].as_i64().unwrap_or(0),
        };
        let (font, name) = normalize_font(font, incoming.get("fontName"), &self.font_names)?;
        data["fontFamily"] = json!(font);
        data["fontName"] = json!(name);
        if let Some(value) = incoming.get("customCss") {
            let value = text(value, "customCss")?;
            if value.len() > 262_144 {
                return Err("Custom CSS exceeds 256 KiB".into());
            }
            data["customCss"] = json!(value);
        } else {
            data["customCss"] = json!("");
        }
        if let Some(colors) = incoming.get("colors") {
            object(colors, "colors")?;
            for spec in schema::PALETTE {
                if let Some(value) = colors.get(spec.key) {
                    data["colors"][spec.key] = normalized_color(value, &data["colors"][spec.key])?;
                }
            }
        }
        if let Some(sizes) = incoming.get("sizes") {
            object(sizes, "sizes")?;
            for spec in schema::DIMENSIONS {
                if let Some(value) = sizes.get(spec.key) {
                    data["sizes"][spec.key] =
                        json!(number(value, spec.key)?.clamp(spec.min, spec.max));
                }
            }
        }
        if let Some(widgets) = incoming.get("widgets") {
            object(widgets, "widgets")?;
            for group in schema::GROUPS {
                let Some(values) = widgets.get(group.key) else {
                    continue;
                };
                object(values, group.key)?;
                for spec in schema::WIDGETS
                    .iter()
                    .filter(|spec| spec.group == group.key)
                {
                    if let Some(value) = values.get(spec.key) {
                        let normalized = match spec.kind {
                            Kind::Flag => json!(value.as_bool().ok_or_else(|| format!(
                                "Expected flag: {}.{}",
                                group.key, spec.key
                            ))?),
                            Kind::Choice => {
                                let maximum = if spec.max < 0.0 {
                                    self.font_names.len().saturating_sub(1) as f64
                                } else {
                                    spec.max
                                };
                                json!(number(value, spec.key)?.clamp(spec.min, maximum) as i64)
                            }
                            Kind::Size => json!(number(value, spec.key)?.clamp(spec.min, spec.max)),
                            Kind::Color => {
                                normalized_color(value, &data["widgets"][group.key][spec.key])?
                            }
                            Kind::Text => {
                                let value = text(value, spec.key)?;
                                if value.len() > spec.max as usize {
                                    return Err("Widget text is too long".into());
                                }
                                json!(value)
                            }
                        };
                        data["widgets"][group.key][spec.key] = normalized;
                    }
                    if spec.kind == Kind::Choice && spec.max < 0.0 {
                        let name_key = format!("{}Name", spec.key);
                        let index = data["widgets"][group.key][spec.key].as_i64().unwrap_or(0);
                        let (index, name) =
                            normalize_font(index, values.get(&name_key), &self.font_names)?;
                        data["widgets"][group.key][spec.key] = json!(index);
                        data["widgets"][group.key][&name_key] = json!(name);
                    }
                }
            }
        }
        let lines = data["widgets"]["chat"]["lines"]
            .as_i64()
            .unwrap_or(10)
            .clamp(1, 50);
        data["widgets"]["chat"]["lines"] = json!(lines);
        let mut rows = [false; 4];
        for row in ["row0", "row1", "row2", "row3"] {
            let mut index = data["widgets"]["playerHud"][row]
                .as_u64()
                .unwrap_or(0)
                .min(3) as usize;
            if rows[index] {
                index = rows.iter().position(|used| !used).unwrap_or(0);
            }
            rows[index] = true;
            data["widgets"]["playerHud"][row] = json!(index);
        }
        if let Some(layouts) = incoming.get("layouts") {
            let layouts = object(layouts, "layouts")?;
            if layouts.len() > 256 {
                return Err("Too many interface elements".into());
            }
            let mut next = Map::new();
            for (id, value) in layouts {
                if id.is_empty()
                    || id.len() > 96
                    || !id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
                {
                    return Err("Invalid interface element identifier".into());
                }
                object(value, id)?;
                let x = value
                    .get("x")
                    .map(|v| number(v, "layout x"))
                    .transpose()?
                    .unwrap_or(0.0);
                let y = value
                    .get("y")
                    .map(|v| number(v, "layout y"))
                    .transpose()?
                    .unwrap_or(0.0);
                let scale = value
                    .get("scale")
                    .map(|v| number(v, "layout scale"))
                    .transpose()?
                    .unwrap_or(1.0);
                next.insert(id.clone(), json!({"x": x.clamp(-10_000.0, 10_000.0), "y": y.clamp(-10_000.0, 10_000.0), "scale": scale.clamp(0.25, 4.0)}));
            }
            data["layouts"] = Value::Object(next);
        }
        let mut total = 0usize;
        for (key, max_id, max_count) in [
            ("weaponImages", 65_535u64, 64),
            ("statusIcons", 3, 4),
            ("logoImages", 999_999, 128),
        ] {
            let images = object(incoming.get(key).unwrap_or(&data[key]), key)?;
            if images.len() > max_count {
                return Err(format!("Too many images: {key}"));
            }
            let mut next = Map::new();
            for (id, value) in images {
                let numeric_id = id
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid image ID: {key}"))?;
                if numeric_id > max_id {
                    return Err(format!("Invalid image ID: {key}"));
                }
                let value = text(value, key)?;
                total = total
                    .checked_add(value.len())
                    .ok_or("Images exceed the profile size limit")?;
                if value.len() > IMAGE_LIMIT || total > IMAGES_LIMIT {
                    return Err("Images exceed the profile size limit".into());
                }
                next.insert(numeric_id.to_string(), json!(value));
            }
            data[key] = Value::Object(next);
        }
        self.data = data;
        Ok(())
    }

    pub fn reset_dialogs(&mut self) {
        let defaults = schema::defaults();
        for key in [
            "enabled",
            "bold",
            "preserveColors",
            "animations",
            "fontFamily",
            "fontName",
            "colors",
            "sizes",
            "customCss",
        ] {
            self.data[key] = defaults[key].clone();
        }
        if let Some(layouts) = self.data["layouts"].as_object_mut() {
            layouts.retain(|id, _| !id.starts_with("dialog."));
        }
    }

    pub fn reset_widget(&mut self, group: &str) {
        let defaults = schema::defaults();
        if defaults["widgets"].get(group).is_none() {
            return;
        }
        self.data["widgets"][group] = defaults["widgets"][group].clone();
        self.reset_positions(group);
    }

    pub fn reset_positions(&mut self, group: &str) {
        let prefix = match group {
            "playerHud" => "hud",
            "gametext" => "message",
            value => value,
        };
        if let Some(layouts) = self.data["layouts"].as_object_mut() {
            let nested = format!("{prefix}.");
            layouts.retain(|id, _| id != prefix && !id.starts_with(&nested));
        }
    }

    pub fn read(&mut self, path: &Path) -> Result<(), String> {
        let text = read_text(path)?;
        let value = parse_ini(&text)?;
        self.apply(&value)
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        let mut validated = self.clone();
        validated.apply(&self.data)?;
        atomic_write(path, &encode_ini(&validated.data)?)
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    let length = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if length > FILE_LIMIT {
        return Err("INI profile exceeds 64 MiB".into());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

fn ini_number(text: &str) -> Result<Value, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| "Invalid number in INI profile".to_owned())?;
    if !value.is_finite() {
        return Err("Invalid number in INI profile".into());
    }
    Ok(json!(value))
}

fn ini_string(text: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !value.is_string() {
        return Err("INI string must be JSON-quoted".into());
    }
    Ok(value)
}

fn ini_array(text: &str, count: usize) -> Result<Value, String> {
    let result: Vec<Value> = text.split(',').map(ini_number).collect::<Result<_, _>>()?;
    if result.len() != count {
        return Err(format!("INI entry must contain {count} numbers"));
    }
    Ok(Value::Array(result))
}

pub fn parse_ini(text: &str) -> Result<Value, String> {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section = String::new();
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_owned();
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("Invalid INI entry")?;
        sections
            .entry(section.clone())
            .or_default()
            .insert(key.trim().to_owned(), value.trim().to_owned());
    }
    let empty = BTreeMap::new();
    let get = |name: &str| sections.get(name).unwrap_or(&empty);
    let mut data = json!({"colors": {}, "sizes": {}, "widgets": {}, "layouts": {}, "weaponImages": {}, "statusIcons": {}, "logoImages": {}});
    let general = get("General");
    for key in ["version", "fontFamily"] {
        if let Some(value) = general.get(key) {
            data[key] = ini_number(value)?;
        }
    }
    if let Some(value) = general.get("fontName") {
        data["fontName"] = ini_string(value)?;
    }
    for key in ["enabled", "bold", "preserveColors", "animations"] {
        if let Some(value) = general.get(key) {
            data[key] = json!(ini_number(value)?.as_f64().unwrap_or(0.0) != 0.0);
        }
    }
    for (key, value) in get("Palette") {
        data["colors"][key] = ini_array(value, 4)?;
    }
    for (key, value) in get("Sizes") {
        data["sizes"][key] = ini_number(value)?;
    }
    if let Some(value) = get("CSS").get("text") {
        data["customCss"] = ini_string(value)?;
    }
    for group in schema::GROUPS {
        let values = get(group.section);
        let mut widget = Map::new();
        for spec in schema::WIDGETS
            .iter()
            .filter(|spec| spec.group == group.key)
        {
            if let Some(value) = values.get(spec.key) {
                widget.insert(
                    spec.key.to_owned(),
                    match spec.kind {
                        Kind::Flag => json!(ini_number(value)?.as_f64().unwrap_or(0.0) != 0.0),
                        Kind::Choice | Kind::Size => ini_number(value)?,
                        Kind::Color => ini_array(value, 4)?,
                        Kind::Text => ini_string(value)?,
                    },
                );
            }
            if spec.kind == Kind::Choice && spec.max < 0.0 {
                let name = format!("{}Name", spec.key);
                if let Some(value) = values.get(&name) {
                    widget.insert(name, ini_string(value)?);
                }
            }
        }
        data["widgets"][group.key] = Value::Object(widget);
    }
    for (id, value) in get("Elements") {
        let array = ini_array(value, 3)?;
        data["layouts"][id] = json!({"x": array[0], "y": array[1], "scale": array[2]});
    }
    for (section, key) in [
        ("WeaponImages", "weaponImages"),
        ("StatusIcons", "statusIcons"),
        ("LogoImages", "logoImages"),
    ] {
        for (id, value) in get(section) {
            data[key][id] = ini_string(value)?;
        }
    }
    Ok(data)
}

fn ini_value(value: &Value) -> Result<String, String> {
    match value {
        Value::Bool(value) => Ok(if *value { "1" } else { "0" }.into()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(_) => serde_json::to_string(value).map_err(|e| e.to_string()),
        Value::Array(values) => values
            .iter()
            .map(ini_value)
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join(",")),
        _ => Err("Unsupported INI value".into()),
    }
}

pub fn encode_ini(data: &Value) -> Result<String, String> {
    let mut output = String::from("[General]\r\nversion=4\r\n");
    let mut line = |key: &str, value: &Value| -> Result<(), String> {
        output.push_str(key);
        output.push('=');
        output.push_str(&ini_value(value)?);
        output.push_str("\r\n");
        Ok(())
    };
    for key in [
        "fontFamily",
        "fontName",
        "enabled",
        "bold",
        "preserveColors",
        "animations",
    ] {
        line(key, &data[key])?;
    }
    output.push_str("\r\n[Palette]\r\n");
    for spec in schema::PALETTE {
        output.push_str(&format!(
            "{}={}\r\n",
            spec.key,
            ini_value(&data["colors"][spec.key])?
        ));
    }
    output.push_str("\r\n[Sizes]\r\n");
    for spec in schema::DIMENSIONS {
        output.push_str(&format!(
            "{}={}\r\n",
            spec.key,
            ini_value(&data["sizes"][spec.key])?
        ));
    }
    output.push_str(&format!(
        "\r\n[CSS]\r\ntext={}\r\n",
        ini_value(&data["customCss"])?
    ));
    for group in schema::GROUPS {
        output.push_str(&format!("\r\n[{}]\r\n", group.section));
        for spec in schema::WIDGETS
            .iter()
            .filter(|spec| spec.group == group.key)
        {
            output.push_str(&format!(
                "{}={}\r\n",
                spec.key,
                ini_value(&data["widgets"][group.key][spec.key])?
            ));
            if spec.kind == Kind::Choice && spec.max < 0.0 {
                let name = format!("{}Name", spec.key);
                output.push_str(&format!(
                    "{}={}\r\n",
                    name,
                    ini_value(&data["widgets"][group.key][&name])?
                ));
            }
        }
    }
    output.push_str("\r\n[Elements]\r\n");
    for (id, value) in object(&data["layouts"], "layouts")? {
        output.push_str(&format!(
            "{}={},{},{}\r\n",
            id,
            ini_value(&value["x"])?,
            ini_value(&value["y"])?,
            ini_value(&value["scale"])?
        ));
    }
    for (section, key) in [
        ("WeaponImages", "weaponImages"),
        ("StatusIcons", "statusIcons"),
        ("LogoImages", "logoImages"),
    ] {
        output.push_str(&format!("\r\n[{section}]\r\n"));
        for (id, value) in object(&data[key], key)? {
            output.push_str(&format!("{}={}\r\n", id, ini_value(value)?));
        }
    }
    Ok(output)
}

fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut file = File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, destination: *const u16, flags: u32) -> i32;
        }
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 9) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    Ok(())
}
