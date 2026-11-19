use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const RUNTIME: &str = include_str!("../assets/runtime.js");
pub const HUD: &str = include_str!("../assets/hud.js");
pub const SPEEDOMETER: &str = include_str!("../assets/speedometer.js");
pub const PLAYER_HUD: &str = include_str!("../assets/player_hud.js");
pub const LOGO: &str = include_str!("../assets/logo.js");
pub const LAYOUT: &str = include_str!("../assets/layout.js");
pub const LOGO_CSS: &str = include_str!("../assets/logo.css");
pub const THEME_CSS: &str = concat!(
    include_str!("../assets/theme.css"),
    "\n",
    include_str!("../assets/dialogs.css")
);
pub const PLAYER_HUD_IMAGES: &str = include_str!("../assets/player_hud.json");
pub const SPEEDOMETER_IMAGES: &str = include_str!("../assets/speedometer.json");

#[derive(Clone)]
struct FontFace {
    path: PathBuf,
    index: u32,
    weight: u16,
}

#[derive(Clone)]
struct FontFamily {
    name: String,
    faces: [Option<FontFace>; 2],
    embedded: Option<[&'static [u8]; 2]>,
}

#[repr(C)]
struct FontGuid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}

struct FontCom(*mut std::ffi::c_void);

impl FontCom {
    fn checked(value: i32, pointer: *mut std::ffi::c_void) -> Result<Self, String> {
        if value < 0 || pointer.is_null() {
            Err(format!("DirectWrite font catalog: {value:#010x}"))
        } else {
            Ok(Self(pointer))
        }
    }

    fn method<T: Copy>(&self, index: usize) -> T {
        unsafe {
            let value = *(*(self.0 as *const *const usize)).add(index);
            std::mem::transmute_copy(&value)
        }
    }
}

impl Drop for FontCom {
    fn drop(&mut self) {
        let release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = self.method(2);
        unsafe {
            release(self.0);
        }
    }
}

#[link(name = "dwrite")]
unsafe extern "system" {
    fn DWriteCreateFactory(
        kind: i32,
        iid: *const FontGuid,
        factory: *mut *mut std::ffi::c_void,
    ) -> i32;
}

fn directwrite_name(names: &FontCom) -> Result<String, String> {
    let find: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u16,
        *mut u32,
        *mut i32,
    ) -> i32 = names.method(4);
    let length: unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32 =
        names.method(7);
    let string: unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut u16, u32) -> i32 =
        names.method(8);
    let locale: Vec<u16> = "en-us\0".encode_utf16().collect();
    let mut selected = 0;
    let mut exists = 0;
    if unsafe { find(names.0, locale.as_ptr(), &mut selected, &mut exists) } < 0 || exists == 0 {
        selected = 0;
    }
    let mut size = 0;
    if unsafe { length(names.0, selected, &mut size) } < 0 || size == 0 || size >= 32768 {
        return Err("DirectWrite font family name is invalid".into());
    }
    let mut output = vec![0u16; size as usize + 1];
    if unsafe { string(names.0, selected, output.as_mut_ptr(), output.len() as u32) } < 0 {
        return Err("DirectWrite font family name is unavailable".into());
    }
    Ok(String::from_utf16_lossy(&output[..size as usize]))
}

fn directwrite_face(font: &FontCom) -> Result<FontFace, String> {
    let create: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> i32 = font.method(13);
    let weight: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = font.method(4);
    let mut pointer = std::ptr::null_mut();
    let face = FontCom::checked(unsafe { create(font.0, &mut pointer) }, pointer)?;
    let files: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut u32,
        *mut *mut std::ffi::c_void,
    ) -> i32 = face.method(4);
    let index: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = face.method(5);
    let mut count = 0;
    if unsafe { files(face.0, &mut count, std::ptr::null_mut()) } < 0 || count != 1 {
        return Err("Unsupported DirectWrite font file collection".into());
    }
    let mut pointer = std::ptr::null_mut();
    let file = FontCom::checked(unsafe { files(face.0, &mut count, &mut pointer) }, pointer)?;
    let key: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut *const std::ffi::c_void,
        *mut u32,
    ) -> i32 = file.method(3);
    let get_loader: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> i32 = file.method(4);
    let mut reference = std::ptr::null();
    let mut reference_size = 0;
    if unsafe { key(file.0, &mut reference, &mut reference_size) } < 0
        || reference.is_null()
        || reference_size == 0
    {
        return Err("DirectWrite font file reference is unavailable".into());
    }
    let mut pointer = std::ptr::null_mut();
    let loader = FontCom::checked(unsafe { get_loader(file.0, &mut pointer) }, pointer)?;
    let query: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const FontGuid,
        *mut *mut std::ffi::c_void,
    ) -> i32 = loader.method(0);
    let iid = FontGuid {
        a: 0xb2d9f3ec,
        b: 0xc9fe,
        c: 0x4a11,
        d: [0xa2, 0xec, 0xd8, 0x62, 0x08, 0xf7, 0xc0, 0xa2],
    };
    let mut pointer = std::ptr::null_mut();
    let local = FontCom::checked(unsafe { query(loader.0, &iid, &mut pointer) }, pointer)?;
    let path_length: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const std::ffi::c_void,
        u32,
        *mut u32,
    ) -> i32 = local.method(4);
    let path: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const std::ffi::c_void,
        u32,
        *mut u16,
        u32,
    ) -> i32 = local.method(5);
    let mut size = 0;
    if unsafe { path_length(local.0, reference, reference_size, &mut size) } < 0
        || size == 0
        || size >= 32768
    {
        return Err("DirectWrite font path is invalid".into());
    }
    let mut output = vec![0u16; size as usize + 1];
    if unsafe {
        path(
            local.0,
            reference,
            reference_size,
            output.as_mut_ptr(),
            output.len() as u32,
        )
    } < 0
    {
        return Err("DirectWrite font path is unavailable".into());
    }
    Ok(FontFace {
        path: PathBuf::from(String::from_utf16_lossy(&output[..size as usize])),
        index: unsafe { index(face.0) },
        weight: unsafe { weight(font.0) } as u16,
    })
}

fn directwrite_family(family: &FontCom) -> Result<FontFamily, String> {
    let names: unsafe extern "system" fn(*mut std::ffi::c_void, *mut *mut std::ffi::c_void) -> i32 =
        family.method(6);
    let matching: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        u32,
        u32,
        u32,
        *mut *mut std::ffi::c_void,
    ) -> i32 = family.method(7);
    let mut pointer = std::ptr::null_mut();
    let names = FontCom::checked(unsafe { names(family.0, &mut pointer) }, pointer)?;
    let name = directwrite_name(&names)?;
    let mut faces = [None, None];
    for (slot, weight) in [400, 700].iter().enumerate() {
        let mut pointer = std::ptr::null_mut();
        let font = FontCom::checked(
            unsafe { matching(family.0, *weight, 5, 0, &mut pointer) },
            pointer,
        )?;
        faces[slot] = Some(directwrite_face(&font)?);
    }
    Ok(FontFamily {
        name,
        faces,
        embedded: None,
    })
}

fn directwrite_families() -> Result<BTreeMap<String, FontFamily>, String> {
    let iid = FontGuid {
        a: 0xb859ee5a,
        b: 0xd838,
        c: 0x4b5b,
        d: [0xa2, 0xe8, 0x1a, 0xdc, 0x7d, 0x93, 0xdb, 0x48],
    };
    let mut pointer = std::ptr::null_mut();
    let factory = FontCom::checked(
        unsafe { DWriteCreateFactory(0, &iid, &mut pointer) },
        pointer,
    )?;
    let collection: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
        i32,
    ) -> i32 = factory.method(3);
    let mut pointer = std::ptr::null_mut();
    let collection = FontCom::checked(unsafe { collection(factory.0, &mut pointer, 0) }, pointer)?;
    let count: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = collection.method(3);
    let family: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        u32,
        *mut *mut std::ffi::c_void,
    ) -> i32 = collection.method(4);
    let count = unsafe { count(collection.0) };
    if count > 65536 {
        return Err("DirectWrite font family count is invalid".into());
    }
    let mut result = BTreeMap::new();
    for index in 0..count {
        let mut pointer = std::ptr::null_mut();
        if let Ok(family) = FontCom::checked(
            unsafe { family(collection.0, index, &mut pointer) },
            pointer,
        ) {
            if let Ok(item) = directwrite_family(&family) {
                result.insert(item.name.to_lowercase(), item);
            }
        }
    }
    if ["arial", "tahoma", "verdana", "consolas"]
        .iter()
        .any(|name| !result.contains_key(*name))
    {
        return Err("DirectWrite core Windows fonts are unavailable".into());
    }
    Ok(result)
}

fn catalog() -> &'static Vec<FontFamily> {
    static FONTS: OnceLock<Vec<FontFamily>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut families = directwrite_families().unwrap_or_else(|_| {
            let mut families: BTreeMap<String, FontFamily> = BTreeMap::new();
            let mut folders = vec![
                PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into()))
                    .join("Fonts"),
            ];
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                folders.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
            }
            for folder in folders {
                let Ok(entries) = std::fs::read_dir(folder) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.extension().is_some_and(|e| {
                        ["ttf", "otf", "ttc"]
                            .iter()
                            .any(|v| e.to_string_lossy().eq_ignore_ascii_case(v))
                    }) {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(&path) else {
                        continue;
                    };
                    for index in 0..ttf_parser::fonts_in_collection(&bytes).unwrap_or(1) {
                        let Ok(face) = ttf_parser::Face::parse(&bytes, index) else {
                            continue;
                        };
                        if face.is_italic() {
                            continue;
                        }
                        let mut name = None;
                        for id in [
                            ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
                            ttf_parser::name_id::FAMILY,
                        ] {
                            let mut matches: Vec<_> = face
                                .names()
                                .into_iter()
                                .filter(|n| n.name_id == id && n.is_unicode())
                                .collect();
                            matches.sort_by_key(|n| n.language_id != 0x409);
                            name = matches.iter().find_map(|n| n.to_string());
                            if name.is_some() {
                                break;
                            }
                        }
                        let Some(name) = name else { continue };
                        let weight = face.weight().to_number();
                        let family =
                            families
                                .entry(name.to_lowercase())
                                .or_insert_with(|| FontFamily {
                                    name,
                                    faces: [None, None],
                                    embedded: None,
                                });
                        for (slot, desired) in [400_u16, 700_u16].iter().enumerate() {
                            if family.faces[slot].as_ref().is_none_or(|old| {
                                weight.abs_diff(*desired) < old.weight.abs_diff(*desired)
                            }) {
                                family.faces[slot] = Some(FontFace {
                                    path: path.clone(),
                                    index,
                                    weight,
                                });
                            }
                        }
                    }
                }
            }
            families
        });
        let mut result = Vec::new();
        for name in ["Arial", "Tahoma", "Verdana", "Consolas"] {
            result.push(
                families
                    .remove(&name.to_lowercase())
                    .unwrap_or_else(|| FontFamily {
                        name: name.into(),
                        faces: [None, None],
                        embedded: None,
                    }),
            );
        }
        for (name, regular, bold) in [
            (
                "BankGothic RUSS",
                include_bytes!("../assets/fonts/BankGothic.ttf").as_slice(),
                include_bytes!("../assets/fonts/BankGothic.ttf").as_slice(),
            ),
            (
                "Pricedown RUS",
                include_bytes!("../assets/fonts/Pricedown.ttf").as_slice(),
                include_bytes!("../assets/fonts/Pricedown.ttf").as_slice(),
            ),
            (
                "Gotham Pro",
                include_bytes!("../assets/fonts/GothamPro.ttf").as_slice(),
                include_bytes!("../assets/fonts/GothamProBold.ttf").as_slice(),
            ),
            (
                "SAfonts",
                include_bytes!("../assets/fonts/SAfonts.ttf").as_slice(),
                include_bytes!("../assets/fonts/SAfonts.ttf").as_slice(),
            ),
            (
                "Gotham Pro Black",
                include_bytes!("../assets/fonts/GothamProBlack.ttf").as_slice(),
                include_bytes!("../assets/fonts/GothamProBlack.ttf").as_slice(),
            ),
        ] {
            result.push(FontFamily {
                name: name.into(),
                faces: [None, None],
                embedded: Some([regular, bold]),
            });
        }
        result.extend(families.into_values());
        result
    })
}

pub fn menu_font_bytes(bold: bool) -> Result<Arc<Vec<u8>>, String> {
    type MenuFontCache = Mutex<HashMap<bool, Arc<Vec<u8>>>>;
    static CACHE: OnceLock<MenuFontCache> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "Menu font cache unavailable")?;
    if let Some(bytes) = cache.get(&bold) {
        return Ok(bytes.clone());
    }
    let directory =
        PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into()))
            .join("Fonts");
    let read = |name: &str| {
        let bytes = std::fs::read(directory.join(name)).ok()?;
        if bytes.is_empty()
            || bytes.len() > 67_108_864
            || ttf_parser::Face::parse(&bytes, 0).is_err()
        {
            return None;
        }
        Some(bytes)
    };
    for (regular, heavy) in [
        ("segoeui.ttf", "segoeuib.ttf"),
        ("arial.ttf", "arialbd.ttf"),
        ("tahoma.ttf", "tahomabd.ttf"),
    ] {
        let Some(regular) = read(regular) else {
            continue;
        };
        let bytes = Arc::new(if bold {
            read(heavy).unwrap_or(regular)
        } else {
            regular
        });
        cache.insert(bold, bytes.clone());
        return Ok(bytes);
    }
    Err("Windows menu font unavailable: Segoe UI, Arial, Tahoma".into())
}

pub fn font_names() -> Vec<String> {
    catalog().iter().map(|f| f.name.clone()).collect()
}

pub fn font_name(index: usize) -> String {
    catalog().get(index).unwrap_or(&catalog()[0]).name.clone()
}

pub fn font_bytes(index: usize, bold: bool) -> Result<Arc<Vec<u8>>, String> {
    type FontCache = Mutex<HashMap<(usize, bool), Arc<Vec<u8>>>>;
    static CACHE: OnceLock<FontCache> = OnceLock::new();
    let index = index.min(catalog().len() - 1);
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "Font cache unavailable".to_string())?;
    if let Some(bytes) = cache.get(&(index, bold)) {
        return Ok(bytes.clone());
    }
    let family = &catalog()[index];
    let bytes = if let Some(embedded) = family.embedded {
        embedded[usize::from(bold)].to_vec()
    } else {
        let face = family.faces[usize::from(bold)]
            .as_ref()
            .ok_or_else(|| format!("Windows font unavailable: {}", family.name))?;
        standalone(
            std::fs::read(&face.path).map_err(|e| format!("{}: {e}", face.path.display()))?,
            face.index,
        )?
    };
    let bytes = Arc::new(bytes);
    cache.insert((index, bold), bytes.clone());
    Ok(bytes)
}

fn standalone(input: Vec<u8>, index: u32) -> Result<Vec<u8>, String> {
    fn integer(bytes: &[u8], offset: usize, length: usize) -> Result<u32, String> {
        let slice = bytes
            .get(offset..offset + length)
            .ok_or("Truncated font table")?;
        Ok(slice.iter().fold(0_u32, |v, b| (v << 8) | u32::from(*b)))
    }
    if input.get(..4) != Some(b"ttcf") {
        return Ok(input);
    }
    if index >= integer(&input, 8, 4)? {
        return Err("Font collection index is invalid".into());
    }
    let face = integer(&input, 12 + index as usize * 4, 4)? as usize;
    let count = integer(&input, face + 4, 2)? as usize;
    if count == 0 || count > 512 {
        return Err("Font collection directory is invalid".into());
    }
    let mut output = input
        .get(face..face + 12 + count * 16)
        .ok_or("Font collection directory is invalid")?
        .to_vec();
    let mut head = None;
    for i in 0..count {
        let record = 12 + i * 16;
        let source = integer(&input, face + record + 8, 4)? as usize;
        let length = integer(&input, face + record + 12, 4)? as usize;
        if output.len() + length > 67_108_864 {
            return Err("Font is too large".into());
        }
        while output.len() % 4 != 0 {
            output.push(0);
        }
        let offset = output.len();
        output[record + 8..record + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        output.extend_from_slice(
            input
                .get(source..source + length)
                .ok_or("Truncated font collection table")?,
        );
        if input.get(face + record..face + record + 4) == Some(b"head") && length >= 12 {
            head = Some(offset);
        }
    }
    while output.len() % 4 != 0 {
        output.push(0);
    }
    if let Some(head) = head {
        output[head + 8..head + 12].fill(0);
        let sum = output.chunks_exact(4).fold(0_u32, |sum, bytes| {
            sum.wrapping_add(u32::from_be_bytes(bytes.try_into().unwrap()))
        });
        output[head + 8..head + 12]
            .copy_from_slice(&0xb1b0afba_u32.wrapping_sub(sum).to_be_bytes());
    }
    Ok(output)
}

#[derive(Clone)]
struct Resource {
    bytes: Arc<Vec<u8>>,
    content_type: String,
}

pub struct AssetServer {
    resources: Arc<Mutex<HashMap<String, Resource>>>,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    port: u16,
    prefix: String,
}

impl AssetServer {
    pub fn new() -> Self {
        Self {
            resources: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
            worker: None,
            port: 0,
            prefix: String::new(),
        }
    }

    fn start(&mut self) -> Result<(), String> {
        if self.worker.is_some() {
            return Ok(());
        }
        let listener =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        self.port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let mut nonce = [0_u8; 16];
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                algorithm: *mut std::ffi::c_void,
                buffer: *mut u8,
                length: u32,
                flags: u32,
            ) -> i32;
        }
        if unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                nonce.as_mut_ptr(),
                nonce.len() as u32,
                2,
            )
        } < 0
        {
            return Err("Asset nonce generation failed".into());
        }
        self.prefix = format!(
            "/{}/",
            nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        self.running.store(true, Ordering::Release);
        let resources = self.resources.clone();
        let running = self.running.clone();
        self.worker = Some(thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut client, _)) => {
                        let _ = respond(&mut client, &resources);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        }));
        Ok(())
    }

    pub fn publish(
        &mut self,
        name: &str,
        bytes: Arc<Vec<u8>>,
        content_type: &str,
    ) -> Result<String, String> {
        self.start()?;
        let path = format!("{}{name}", self.prefix);
        self.resources
            .lock()
            .map_err(|_| "Asset cache unavailable")?
            .insert(
                path.clone(),
                Resource {
                    bytes,
                    content_type: content_type.into(),
                },
            );
        Ok(format!("http://127.0.0.1:{}{path}", self.port))
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Ok(mut resources) = self.resources.lock() {
            resources.clear();
        }
        self.port = 0;
    }
}

impl Default for AssetServer {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for AssetServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn respond(
    client: &mut TcpStream,
    resources: &Mutex<HashMap<String, Resource>>,
) -> std::io::Result<()> {
    client.set_read_timeout(Some(Duration::from_millis(1500)))?;
    client.set_write_timeout(Some(Duration::from_millis(1500)))?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    while !request.windows(4).any(|v| v == b"\r\n\r\n") {
        let n = client.read(&mut buffer)?;
        if n == 0 || request.len() + n > 16_384 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..n]);
    }
    let text = String::from_utf8_lossy(&request);
    let mut words = text.split_whitespace();
    let method = words.next().unwrap_or("");
    let path = words.next().unwrap_or("");
    let headers = "Connection: close\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, HEAD, OPTIONS\r\nAccess-Control-Allow-Private-Network: true\r\n";
    if method == "OPTIONS" {
        return client.write_all(
            format!("HTTP/1.1 204 No Content\r\n{headers}Content-Length: 0\r\n\r\n").as_bytes(),
        );
    }
    let item = resources
        .lock()
        .ok()
        .and_then(|cache| cache.get(path).cloned());
    if !["GET", "HEAD"].contains(&method) || item.is_none() {
        return client.write_all(
            format!("HTTP/1.1 404 Not Found\r\n{headers}Content-Length: 0\r\n\r\n").as_bytes(),
        );
    }
    let item = item.unwrap();
    client.write_all(format!("HTTP/1.1 200 OK\r\n{headers}Content-Type: {}\r\nCache-Control: private, max-age=86400\r\nContent-Length: {}\r\n\r\n", item.content_type, item.bytes.len()).as_bytes())?;
    if method == "GET" {
        client.write_all(&item.bytes)?;
    }
    Ok(())
}
