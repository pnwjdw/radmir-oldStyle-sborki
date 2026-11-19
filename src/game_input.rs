use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const GAME_SHA256: &str = "c5c73f30d8b0341a3d2c9bf4e1d44ae036dfdc2f561e138107f63a17747484e0";
const SAMP_SHA256: &str = "38b9aaa02746cfd1aec09d195e2076cc5e025a137ee8fbb3c403d23c81a4921b";
const GAME_REFERENCE: usize = 0x26e8f4;
const DEVICE_REFERENCE: usize = 0x26e888;
const SET_CURSOR_MODE: usize = 0x9ffe0;
const RESTORE_CURSOR: usize = 0x9fec0;
const CURSOR_MODE: usize = 0x61;
const LOCK_CAMERA_AND_CONTROLS: i32 = 2;

#[derive(Clone, Copy)]
struct Native {
    base: usize,
}

struct Lease {
    native: Native,
    game: usize,
    previous_mode: i32,
    thread: u32,
}

static NATIVE: OnceLock<Native> = OnceLock::new();
static LEASE: Mutex<Option<Lease>> = Mutex::new(None);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetModuleFileNameW(module: *mut c_void, path: *mut u16, size: u32) -> u32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentThreadId() -> u32;
    fn ReadProcessMemory(
        process: *mut c_void,
        address: *const c_void,
        output: *mut c_void,
        length: usize,
        read: *mut usize,
    ) -> i32;
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptOpenAlgorithmProvider(
        algorithm: *mut *mut c_void,
        name: *const u16,
        implementation: *const u16,
        flags: u32,
    ) -> i32;
    fn BCryptCloseAlgorithmProvider(algorithm: *mut c_void, flags: u32) -> i32;
    fn BCryptHash(
        algorithm: *mut c_void,
        secret: *const u8,
        secret_length: u32,
        input: *const u8,
        input_length: u32,
        output: *mut u8,
        output_length: u32,
    ) -> i32;
}

fn memory(address: usize, length: usize) -> Option<Vec<u8>> {
    if address < 0x10000 || address.checked_add(length).is_none() {
        return None;
    }
    let mut bytes = vec![0; length];
    let mut read = 0;
    if unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            length,
            &mut read,
        )
    } == 0
        || read != length
    {
        None
    } else {
        Some(bytes)
    }
}

fn word(address: usize) -> Option<usize> {
    let bytes = memory(address, 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?) as usize)
}

fn mode(game: usize) -> Option<i32> {
    let value = word(game.checked_add(CURSOR_MODE)?)? as i32;
    (0..=4).contains(&value).then_some(value)
}

fn module_path(module: usize) -> Result<PathBuf, String> {
    let mut path = vec![0u16; 32768];
    let count =
        unsafe { GetModuleFileNameW(module as *mut c_void, path.as_mut_ptr(), path.len() as u32) }
            as usize;
    if count == 0 || count >= path.len() {
        return Err("Не удалось определить файл игрового модуля для управления мышью".into());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(&path[..count])))
}

fn module_hash(module: usize) -> Result<String, String> {
    let bytes = std::fs::read(module_path(module)?).map_err(|error| error.to_string())?;
    let size = u32::try_from(bytes.len()).map_err(|_| "Игровой модуль слишком большой")?;
    let name: Vec<u16> = "SHA256\0".encode_utf16().collect();
    let mut algorithm = std::ptr::null_mut();
    if unsafe { BCryptOpenAlgorithmProvider(&mut algorithm, name.as_ptr(), std::ptr::null(), 0) }
        < 0
    {
        return Err("SHA-256 недоступен для проверки управления мышью".into());
    }
    let mut digest = [0u8; 32];
    let result = unsafe {
        BCryptHash(
            algorithm,
            std::ptr::null(),
            0,
            bytes.as_ptr(),
            size,
            digest.as_mut_ptr(),
            digest.len() as u32,
        )
    };
    unsafe {
        BCryptCloseAlgorithmProvider(algorithm, 0);
    }
    if result < 0 {
        return Err("Не удалось проверить игровые модули для управления мышью".into());
    }
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn native() -> Result<Native, String> {
    if let Some(native) = NATIVE.get() {
        return Ok(*native);
    }
    let name: Vec<u16> = "samp.dll\0".encode_utf16().collect();
    let base = unsafe { GetModuleHandleW(name.as_ptr()) } as usize;
    let game = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    if base == 0 {
        return Err("Управление мышью ожидает загрузку SA-MP".into());
    }
    if game != 0x400000 || module_hash(game)? != GAME_SHA256 || module_hash(base)? != SAMP_SHA256 {
        return Err("Версия RADMIR не проверена для управления мышью".into());
    }
    let signatures: &[(usize, &[u8])] = &[
        (
            SET_CURSOR_MODE,
            &[
                0x55, 0x8b, 0xec, 0x8b, 0x45, 0x08, 0x83, 0xf8, 0x02, 0x56, 0x57, 0x8b, 0xf1, 0x75,
                0x6f, 0x6a, 0x05, 0x68, 0xf5, 0x1d, 0x54, 0x00, 0xe8, 0x35, 0x9c, 0x00, 0x00, 0xb8,
                0x90, 0x90, 0x90, 0x90,
            ],
        ),
        (
            RESTORE_CURSOR,
            &[
                0x56, 0x8b, 0xf1, 0x8b, 0x46, 0x61, 0x57, 0x33, 0xff, 0x3b, 0xc7, 0x0f, 0x85, 0x07,
                0x01, 0x00, 0x00, 0x8b, 0x46, 0x65, 0x3b, 0xc7, 0x0f, 0x85, 0xf6, 0x00, 0x00, 0x00,
            ],
        ),
    ];
    for (offset, expected) in signatures {
        if memory(base + offset, expected.len()).as_deref() != Some(*expected) {
            return Err("Функции управления мышью SA-MP изменены; вызов отменён".into());
        }
    }
    if memory(base + 0x68f8a, 2).as_deref() != Some(&[0x8b, 0x0d])
        || word(base + 0x68f8c) != Some(base + GAME_REFERENCE)
    {
        return Err("Объект управления мышью SA-MP не совпадает с проверенной версией".into());
    }
    let native = Native { base };
    let _ = NATIVE.set(native);
    Ok(native)
}

impl Native {
    fn game(self) -> Option<usize> {
        let game = word(self.base + GAME_REFERENCE).filter(|game| *game >= 0x10000)?;
        mode(game)?;
        let device = word(self.base + DEVICE_REFERENCE).filter(|device| *device >= 0x10000)?;
        let vtable = word(device).filter(|table| *table >= 0x10000)?;
        word(vtable + 12 * 4).filter(|function| *function >= 0x10000)?;
        Some(game)
    }

    #[cfg(target_arch = "x86")]
    unsafe fn set_mode(self, game: usize, mode: i32) {
        let function: unsafe extern "thiscall" fn(*mut c_void, i32, i32) =
            unsafe { std::mem::transmute(self.base + SET_CURSOR_MODE) };
        unsafe { function(game as *mut c_void, mode, 1) };
    }

    #[cfg(target_arch = "x86")]
    unsafe fn restore(self, game: usize) {
        let function: unsafe extern "thiscall" fn(*mut c_void) =
            unsafe { std::mem::transmute(self.base + RESTORE_CURSOR) };
        unsafe { function(game as *mut c_void) };
    }
}

pub fn active() -> bool {
    LEASE.lock().map(|lease| lease.is_some()).unwrap_or(false)
}

pub fn thread_id() -> u32 {
    LEASE
        .lock()
        .map(|lease| lease.as_ref().map_or(0, |lease| lease.thread))
        .unwrap_or(0)
}

#[cfg(target_arch = "x86")]
pub fn update(open: bool) -> Result<(), String> {
    let thread = unsafe { GetCurrentThreadId() };
    let mut lease = LEASE
        .lock()
        .map_err(|_| "Блокировка управления мышью повреждена")?;
    if lease.as_ref().is_some_and(|lease| lease.thread != thread) {
        return Err("Управление мышью должно изменяться в игровом потоке".into());
    }
    if !open {
        if let Some(current) = lease.as_ref() {
            if current.native.game() == Some(current.game)
                && mode(current.game) == Some(LOCK_CAMERA_AND_CONTROLS)
                && current.previous_mode != LOCK_CAMERA_AND_CONTROLS
            {
                unsafe {
                    current.native.set_mode(current.game, 0);
                    current.native.restore(current.game);
                    if current.previous_mode != 0 {
                        current.native.set_mode(current.game, current.previous_mode);
                    }
                }
            }
        }
        *lease = None;
        return Ok(());
    }
    let native = native()?;
    let game = native
        .game()
        .ok_or("Управление мышью ожидает игровой объект и Direct3D")?;
    let current_mode = mode(game).ok_or("Режим мыши SA-MP недоступен")?;
    if lease.as_ref().is_some_and(|lease| lease.game != game) {
        *lease = None;
    }
    if lease.is_none() {
        *lease = Some(Lease {
            native,
            game,
            previous_mode: current_mode,
            thread,
        });
    } else if current_mode != LOCK_CAMERA_AND_CONTROLS {
        if let Some(lease) = lease.as_mut() {
            lease.previous_mode = current_mode;
        }
    }
    if current_mode != LOCK_CAMERA_AND_CONTROLS {
        unsafe {
            native.set_mode(game, LOCK_CAMERA_AND_CONTROLS);
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "x86"))]
pub fn update(_: bool) -> Result<(), String> {
    Err("Для управления мышью RADMIR требуется 32-разрядная сборка".into())
}

pub fn shutdown() -> Result<(), String> {
    update(false)
}
