use std::ffi::{CString, c_void};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

#[repr(C)]
struct ProcessEntry {
    size: u32,
    usage: u32,
    pid: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    priority: i32,
    flags: u32,
    exe: [u16; 260],
}
#[repr(C)]
struct ModuleEntry {
    size: u32,
    module_id: u32,
    pid: u32,
    global_usage: u32,
    process_usage: u32,
    base: *mut u8,
    length: u32,
    module: isize,
    name: [u16; 256],
    path: [u16; 260],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
    fn Process32FirstW(snapshot: isize, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: isize, entry: *mut ProcessEntry) -> i32;
    fn Module32FirstW(snapshot: isize, entry: *mut ModuleEntry) -> i32;
    fn Module32NextW(snapshot: isize, entry: *mut ModuleEntry) -> i32;
    fn OpenProcess(rights: u32, inherit: i32, pid: u32) -> isize;
    fn CloseHandle(handle: isize) -> i32;
    fn K32GetModuleFileNameExW(process: isize, module: isize, name: *mut u16, count: u32) -> u32;
    fn QueryFullProcessImageNameW(
        process: isize,
        flags: u32,
        name: *mut u16,
        count: *mut u32,
    ) -> i32;
    fn VirtualAllocEx(
        process: isize,
        address: *mut c_void,
        size: usize,
        kind: u32,
        protection: u32,
    ) -> *mut c_void;
    fn VirtualFreeEx(process: isize, address: *mut c_void, size: usize, kind: u32) -> i32;
    fn WriteProcessMemory(
        process: isize,
        address: *mut c_void,
        bytes: *const c_void,
        count: usize,
        written: *mut usize,
    ) -> i32;
    fn CreateRemoteThread(
        process: isize,
        attributes: *const c_void,
        stack: usize,
        start: unsafe extern "system" fn(*mut c_void) -> u32,
        param: *mut c_void,
        flags: u32,
        id: *mut u32,
    ) -> isize;
    fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
    fn GetExitCodeThread(thread: isize, code: *mut u32) -> i32;
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut isize) -> i32;
    fn GetModuleFileNameW(module: isize, buffer: *mut u16, length: u32) -> u32;
    fn GetProcAddress(module: isize, name: *const u8) -> *mut c_void;
    fn LoadLibraryExW(name: *const u16, file: isize, flags: u32) -> isize;
    fn FreeLibrary(module: isize) -> i32;
    fn GetLastError() -> u32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(window: isize, text: *const u16, title: *const u16, flags: u32) -> i32;
}

pub(super) struct Handle(pub(super) isize);
impl Drop for Handle {
    fn drop(&mut self) {
        if self.0 != 0 && self.0 != -1 {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn utf16(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())])
}
pub(super) fn win_error(operation: &str) -> String {
    format!("{operation}: Windows {}", unsafe { GetLastError() })
}
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

pub(super) fn processes() -> Result<Vec<u32>, String> {
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(2, 0) });
    if snapshot.0 == -1 {
        return Err(win_error("Process snapshot"));
    }
    let mut entry: ProcessEntry = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<ProcessEntry>() as u32;
    let mut found = Vec::new();
    let mut valid = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while valid {
        if utf16(&entry.exe).eq_ignore_ascii_case("gta_sa.exe") {
            found.push(entry.pid);
        }
        valid = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    Ok(found)
}

pub(super) fn modules(pid: u32) -> Result<Vec<(String, String, usize)>, String> {
    let reader = Handle(unsafe { OpenProcess(0x410, 0, pid) });
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(0x18, pid) });
    if snapshot.0 == -1 {
        return Err(win_error("Module snapshot"));
    }
    let mut entry: ModuleEntry = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<ModuleEntry>() as u32;
    let mut found = Vec::new();
    let mut valid = unsafe { Module32FirstW(snapshot.0, &mut entry) } != 0;
    while valid {
        let mut path = utf16(&entry.path);
        if reader.0 != 0 {
            let mut buffer = vec![0u16; 32768];
            let count = unsafe {
                K32GetModuleFileNameExW(
                    reader.0,
                    entry.module,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                )
            } as usize;
            if count > 0 && count < buffer.len() {
                path = String::from_utf16_lossy(&buffer[..count]);
            }
        }
        found.push((utf16(&entry.name), path, entry.base as usize));
        valid = unsafe { Module32NextW(snapshot.0, &mut entry) } != 0;
    }
    Ok(found)
}

pub(super) fn path_of(process: isize) -> Result<PathBuf, String> {
    let mut name = vec![0u16; 32768];
    let mut count = name.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut count) } == 0 {
        return Err(win_error("Game executable"));
    }
    Ok(PathBuf::from(String::from_utf16_lossy(
        &name[..count as usize],
    )))
}

pub(super) fn thread(process: isize, address: usize, parameter: usize) -> Result<u32, String> {
    let entry: unsafe extern "system" fn(*mut c_void) -> u32 =
        unsafe { std::mem::transmute(address) };
    let handle = Handle(unsafe {
        CreateRemoteThread(
            process,
            null(),
            0,
            entry,
            parameter as *mut c_void,
            0,
            null_mut(),
        )
    });
    if handle.0 == 0 {
        return Err(win_error("CreateRemoteThread"));
    }
    if unsafe { WaitForSingleObject(handle.0, 20000) } != 0 {
        return Err("Game operation timed out; the game may be busy".into());
    }
    let mut code = 0;
    if unsafe { GetExitCodeThread(handle.0, &mut code) } == 0 {
        return Err(win_error("Thread result"));
    }
    Ok(code)
}

pub(super) fn load(process: isize, pid: u32, path: &Path) -> Result<usize, String> {
    let bytes = wide(&path.to_string_lossy());
    let size = bytes.len() * 2;
    let memory = unsafe { VirtualAllocEx(process, null_mut(), size, 0x3000, 4) };
    if memory.is_null() {
        return Err(win_error("DLL path allocation"));
    }
    let mut written = 0;
    if unsafe { WriteProcessMemory(process, memory, bytes.as_ptr().cast(), size, &mut written) }
        == 0
        || written != size
    {
        unsafe {
            VirtualFreeEx(process, memory, 0, 0x8000);
        }
        return Err(win_error("DLL path copy"));
    }
    let local_kernel = unsafe { GetModuleHandleW(wide("kernel32.dll").as_ptr()) };
    let function =
        unsafe { GetProcAddress(local_kernel, c"LoadLibraryW".as_ptr().cast()) } as usize;
    if function == 0 {
        unsafe {
            VirtualFreeEx(process, memory, 0, 0x8000);
        }
        return Err(win_error("LoadLibraryW address"));
    }
    let mut owner = 0;
    if unsafe { GetModuleHandleExW(6, function as *const u16, &mut owner) } == 0 {
        unsafe {
            VirtualFreeEx(process, memory, 0, 0x8000);
        }
        return Err(win_error("Loader module"));
    }
    let mut owner_path = vec![0u16; 32768];
    let count =
        unsafe { GetModuleFileNameW(owner, owner_path.as_mut_ptr(), owner_path.len() as u32) };
    if count == 0 {
        unsafe {
            VirtualFreeEx(process, memory, 0, 0x8000);
        }
        return Err(win_error("Loader module path"));
    }
    let owner_name = file_name(Path::new(&String::from_utf16_lossy(
        &owner_path[..count as usize],
    )));
    let remote_owner = modules(pid)?
        .into_iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(&owner_name))
        .ok_or("Game loader module missing")?
        .2;
    let result = thread(
        process,
        remote_owner + function - owner as usize,
        memory as usize,
    );
    if result.is_ok() {
        unsafe {
            VirtualFreeEx(process, memory, 0, 0x8000);
        }
    }
    let module = result? as usize;
    if module == 0 {
        Err("LoadLibraryW failed in the game. Verify the 32-bit DLL and its path".into())
    } else {
        Ok(module)
    }
}

pub(super) fn export_offset(path: &Path, name: &str) -> Result<usize, String> {
    let module = unsafe { LoadLibraryExW(wide(&path.to_string_lossy()).as_ptr(), 0, 1) };
    if module == 0 {
        return Err(win_error("Inspect plugin exports"));
    }
    let selected = CString::new(name).map_err(|e| e.to_string())?;
    let address = unsafe { GetProcAddress(module, selected.as_ptr().cast()) } as usize;
    unsafe {
        FreeLibrary(module);
    }
    if address == 0 {
        Err(format!("Plugin export missing: {name}"))
    } else {
        Ok(address - module as usize)
    }
}

pub(super) fn show_message(message: &str, failed: bool) {
    unsafe {
        MessageBoxW(
            0,
            wide(message).as_ptr(),
            wide("OldDialogs RADMIR").as_ptr(),
            if failed { 0x10 } else { 0x40 },
        );
    }
}

pub(super) fn open_process(rights: u32, pid: u32) -> Handle {
    Handle(unsafe { OpenProcess(rights, 0, pid) })
}
