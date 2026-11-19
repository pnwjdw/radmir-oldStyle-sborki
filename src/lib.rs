#![allow(non_snake_case)]

mod assets;
mod bridge;
mod d3d9;
mod game_input;
mod graphics;
mod inspector;
mod schema;
mod settings;
#[allow(clippy::too_many_arguments)]
mod ui;
#[allow(clippy::too_many_arguments)]
mod widgets;

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::{Duration, Instant};

static MODULE: AtomicIsize = AtomicIsize::new(0);
static STOP: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static RESULT: AtomicU32 = AtomicU32::new(0);
static LIFECYCLE: Mutex<isize> = Mutex::new(0);
static ASI_BOOTSTRAP: AtomicBool = AtomicBool::new(false);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateThread(
        attributes: *const c_void,
        stack: usize,
        entry: unsafe extern "system" fn(*mut c_void) -> u32,
        parameter: *mut c_void,
        flags: u32,
        id: *mut u32,
    ) -> isize;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut isize) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, buffer: *mut u16, length: u32) -> u32;
    fn FreeLibrary(module: isize) -> i32;
    fn FreeLibraryAndExitThread(module: isize, code: u32) -> !;
    fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
    fn CloseHandle(handle: isize) -> i32;
}

fn directory() -> PathBuf {
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        GetModuleFileNameW(
            MODULE.load(Ordering::Acquire) as *mut c_void,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    };
    PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]))
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf()
}

unsafe extern "system" fn worker(parameter: *mut c_void) -> u32 {
    let module = parameter as isize;
    let root = directory();
    std::panic::set_hook(Box::new({
        let root = root.clone();
        move |information| {
            let _ = std::fs::write(root.join("last-error.txt"), information.to_string());
        }
    }));
    let result = match graphics::run(root.clone(), &STOP) {
        Ok(()) => 0,
        Err(error) => {
            let _ = std::fs::write(root.join("last-error.txt"), &error);
            RESULT.store(31, Ordering::Release);
            31
        }
    };
    ACTIVE.store(false, Ordering::Release);
    unsafe { FreeLibraryAndExitThread(module, result) }
}

unsafe extern "system" fn asi_start(_: *mut c_void) -> u32 {
    OldDialogsStart(std::ptr::null_mut())
}

fn schedule_asi_start() {
    if ASI_BOOTSTRAP.swap(true, Ordering::AcqRel) {
        return;
    }
    let handle = unsafe {
        CreateThread(
            std::ptr::null(),
            0,
            asi_start,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == 0 {
        ASI_BOOTSTRAP.store(false, Ordering::Release);
    } else {
        unsafe {
            CloseHandle(handle);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn InitializeASI() {
    schedule_asi_start();
}

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(module: isize, reason: u32, _: *mut c_void) -> i32 {
    if reason == 1 {
        MODULE.store(module, Ordering::Release);
        let mut path = [0u16; 1024];
        let length = unsafe {
            GetModuleFileNameW(module as *mut c_void, path.as_mut_ptr(), path.len() as u32)
        } as usize;
        if length >= 4
            && length < path.len()
            && path[length - 4..length]
                .iter()
                .copied()
                .map(|c| (c as u8).to_ascii_lowercase())
                .eq(b".asi".iter().copied())
        {
            schedule_asi_start();
        }
    }
    if reason == 0 {
        STOP.store(true, Ordering::Release);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsStart(_: *mut c_void) -> u32 {
    let Ok(mut handle) = LIFECYCLE.lock() else {
        return 31;
    };
    if ACTIVE.load(Ordering::Acquire) {
        if STOP.load(Ordering::Acquire) {
            return 21;
        }
        graphics::set_visible(true);
        return if graphics::state() >= 3 { 0 } else { 21 };
    }
    if *handle != 0 {
        unsafe {
            CloseHandle(*handle);
        }
        *handle = 0;
    }
    STOP.store(false, Ordering::Release);
    RESULT.store(0, Ordering::Release);
    let mut keep_alive = 0;
    if unsafe { GetModuleHandleExW(4, worker as *const u16, &mut keep_alive) } == 0 {
        return 31;
    }
    ACTIVE.store(true, Ordering::Release);
    let thread = unsafe {
        CreateThread(
            std::ptr::null(),
            0,
            worker,
            keep_alive as *mut c_void,
            0,
            std::ptr::null_mut(),
        )
    };
    if thread == 0 {
        ACTIVE.store(false, Ordering::Release);
        unsafe {
            FreeLibrary(keep_alive);
        }
        return 31;
    }
    *handle = thread;
    let until = Instant::now() + Duration::from_secs(15);
    while graphics::state() < 3 && ACTIVE.load(Ordering::Acquire) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    if graphics::state() >= 3 {
        0
    } else if ACTIVE.load(Ordering::Acquire) {
        21
    } else {
        RESULT.load(Ordering::Acquire)
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsStop(_: *mut c_void) -> u32 {
    let Ok(mut handle) = LIFECYCLE.lock() else {
        return 31;
    };
    STOP.store(true, Ordering::Release);
    graphics::cancel_connection();
    if *handle == 0 {
        return 0;
    }
    if unsafe { WaitForSingleObject(*handle, 15000) } != 0 {
        return 21;
    }
    unsafe {
        CloseHandle(*handle);
    }
    *handle = 0;
    RESULT.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsRunning(_: *mut c_void) -> u32 {
    ACTIVE.load(Ordering::Acquire) as u32
}
#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsState(_: *mut c_void) -> u32 {
    graphics::state()
}
#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsVersion(_: *mut c_void) -> u32 {
    0x0009_0000
}
#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsMenu(visible: *mut c_void) -> u32 {
    graphics::set_visible(!visible.is_null());
    0
}
#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsReloadSettings(_: *mut c_void) -> u32 {
    graphics::reload();
    0
}
#[unsafe(no_mangle)]
pub extern "system" fn OldDialogsSnapshot(_: *mut c_void) -> u32 {
    if graphics::snapshot() { 0 } else { 21 }
}
#[unsafe(no_mangle)]
pub extern "system" fn HasslePluginStart(parameter: *mut c_void) -> u32 {
    OldDialogsStart(parameter)
}
#[unsafe(no_mangle)]
pub extern "system" fn HasslePluginRunning(parameter: *mut c_void) -> u32 {
    OldDialogsRunning(parameter)
}
#[unsafe(no_mangle)]
pub extern "system" fn HasslePluginState(parameter: *mut c_void) -> u32 {
    OldDialogsState(parameter)
}
