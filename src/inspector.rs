use std::ffi::c_void;
use std::net::{Ipv4Addr, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const COHTML_SHA256: [u8; 32] = [
    0x74, 0x9e, 0x9a, 0xe7, 0x07, 0x14, 0xdd, 0xee, 0x94, 0xd3, 0xde, 0xfe, 0x8c, 0x76, 0x6a, 0xa6,
    0x68, 0x86, 0x4e, 0x4d, 0x15, 0x3b, 0xac, 0xce, 0x01, 0xb6, 0xcb, 0xa1, 0x60, 0xc4, 0xd1, 0xb1,
];
const SYSTEM_VTABLE: usize = 0x4536a0;
const VIEW_VTABLE: usize = 0x453438;
const CREATE_SYSTEM: usize = 0x11d36a;
const INSPECTOR_START: usize = 0x396a;
const INSPECTOR_REGISTER: usize = 0x38760;
const DOCUMENT_LOADED: usize = 0x4789;
const ENTRY_CTOR: usize = 0x79f57;
const ENTRY_DTOR: usize = 0x799f2;
const TEMP_CTOR: usize = 0x79c20;
const TEMP_DTOR: usize = 0x79caf;
const DOM_THREAD_ID: usize = 0x4f78f0;
const SYSTEM_SIZE: usize = 1384;
const SYSTEM_INSPECTOR: usize = 1080;
const VIEW_SIZE: usize = 6768;
const VIEW_SYSTEM: usize = 532;
const VIEW_DOCUMENT: usize = 204;
const VIEW_SCRIPTING: usize = 1256;
const VIEW_INSPECTOR_SERVER: usize = 6476;
const VIEW_INSPECTOR_CONTROLLER: usize = 6480;
const VIEW_INSPECTION_ENABLED: usize = 362;
const PROLOGUE: [u8; 6] = [0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8];

#[repr(C)]
#[derive(Default)]
struct MemoryInformation {
    base: usize,
    allocation_base: usize,
    allocation_protect: u32,
    #[cfg(target_pointer_width = "64")]
    partition_id: u16,
    region_size: usize,
    state: u32,
    protect: u32,
    memory_type: u32,
}

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
    fn WriteProcessMemory(
        process: *mut c_void,
        address: *mut c_void,
        input: *const c_void,
        length: usize,
        written: *mut usize,
    ) -> i32;
    fn VirtualQuery(address: *const c_void, info: *mut MemoryInformation, length: usize) -> usize;
    fn VirtualAlloc(
        address: *mut c_void,
        length: usize,
        allocation: u32,
        protection: u32,
    ) -> *mut c_void;
    fn VirtualFree(address: *mut c_void, length: usize, free_type: u32) -> i32;
    fn VirtualProtect(
        address: *mut c_void,
        length: usize,
        protection: u32,
        previous: *mut u32,
    ) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, length: usize) -> i32;
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

#[derive(Clone)]
struct Prepared {
    base: usize,
    system: usize,
    views: Vec<usize>,
}

struct StartupHook {
    address: usize,
    trampoline: usize,
    patch: [u8; 6],
}

static PREPARED: Mutex<Option<Prepared>> = Mutex::new(None);
static RESCAN: AtomicBool = AtomicBool::new(false);

pub fn invalidate() {
    RESCAN.store(true, Ordering::Release);
}
static STARTUP_HOOK: Mutex<Option<StartupHook>> = Mutex::new(None);
static CREATE_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
static CREATE_CALLS: AtomicUsize = AtomicUsize::new(0);
static LATEST_SYSTEM: AtomicUsize = AtomicUsize::new(0);

fn read(address: usize, length: usize) -> Option<Vec<u8>> {
    if address < 0x10000 || length == 0 || address.checked_add(length).is_none() {
        return None;
    }
    let mut bytes = vec![0_u8; length];
    let mut count = 0;
    let result = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            length,
            &mut count,
        )
    };
    (result != 0 && count == length).then_some(bytes)
}

fn word(address: usize) -> Option<usize> {
    read(address, 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
}

fn put_word(address: usize, value: usize) -> Result<(), String> {
    let bytes = (value as u32).to_le_bytes();
    let mut count = 0;
    let result = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *mut c_void,
            bytes.as_ptr().cast(),
            bytes.len(),
            &mut count,
        )
    };
    if result == 0 || count != bytes.len() {
        return Err("Не удалось подключить контроллер Gameface к представлению".into());
    }
    Ok(())
}

fn module() -> Result<usize, String> {
    let name: Vec<u16> = "cohtml.WindowsDesktop.dll\0".encode_utf16().collect();
    let handle = unsafe { GetModuleHandleW(name.as_ptr()) };
    if handle.is_null() {
        return Err("Модуль Gameface ещё не загружен".into());
    }
    static VALIDATED: OnceLock<Mutex<Option<(usize, bool)>>> = OnceLock::new();
    let mut validated = VALIDATED
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "Проверка версии Gameface недоступна")?;
    if let Some((base, valid)) = *validated {
        if base == handle as usize {
            return if valid {
                Ok(base)
            } else {
                Err("Версия Gameface не совпадает с проверенной сборкой RADMIR; подключение отменено".into())
            };
        }
    }
    let mut path = vec![0_u16; 32768];
    let count =
        unsafe { GetModuleFileNameW(handle, path.as_mut_ptr(), path.len() as u32) } as usize;
    if count == 0 || count >= path.len() {
        return Err("Не удалось определить путь модуля Gameface".into());
    }
    let bytes = std::fs::read(String::from_utf16_lossy(&path[..count]))
        .map_err(|e| format!("Не удалось проверить модуль Gameface: {e}"))?;
    let mut algorithm = std::ptr::null_mut();
    let algorithm_name: Vec<u16> = "SHA256\0".encode_utf16().collect();
    let opened = unsafe {
        BCryptOpenAlgorithmProvider(&mut algorithm, algorithm_name.as_ptr(), std::ptr::null(), 0)
    };
    if opened < 0 {
        return Err("SHA-256 недоступен для проверки Gameface".into());
    }
    let mut digest = [0_u8; 32];
    let hashed = unsafe {
        BCryptHash(
            algorithm,
            std::ptr::null(),
            0,
            bytes.as_ptr(),
            bytes.len() as u32,
            digest.as_mut_ptr(),
            digest.len() as u32,
        )
    };
    unsafe {
        BCryptCloseAlgorithmProvider(algorithm, 0);
    }
    if hashed < 0 {
        return Err("Не удалось вычислить SHA-256 модуля Gameface".into());
    }
    let valid = digest == COHTML_SHA256;
    *validated = Some((handle as usize, valid));
    if !valid {
        return Err(
            "Версия Gameface не совпадает с проверенной сборкой RADMIR; подключение отменено"
                .into(),
        );
    }
    Ok(handle as usize)
}

fn valid_system(base: usize, address: usize) -> bool {
    let Some(bytes) = read(address, SYSTEM_SIZE) else {
        return false;
    };
    u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize == base + SYSTEM_VTABLE
        && u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize == address
        && word(address + 4) == Some(base + 0x453878)
}

fn valid_view(base: usize, address: usize, system: usize) -> bool {
    let Some(bytes) = read(address, VIEW_SIZE) else {
        return false;
    };
    u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize == base + VIEW_VTABLE
        && u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize == address
        && u32::from_le_bytes(bytes[VIEW_SYSTEM..VIEW_SYSTEM + 4].try_into().unwrap()) as usize
            == system
        && u32::from_le_bytes(
            bytes[VIEW_INSPECTOR_SERVER..VIEW_INSPECTOR_SERVER + 4]
                .try_into()
                .unwrap(),
        ) as usize
            == system + SYSTEM_INSPECTOR
}

pub fn prepare() -> Result<(), String> {
    let base = module()?;
    let mut prepared = PREPARED.lock().map_err(|_| "Подключение Gameface занято")?;
    let requested = RESCAN.swap(false, Ordering::AcqRel);
    if !requested
        && prepared.as_ref().is_some_and(|p| {
            p.base == base
                && valid_system(base, p.system)
                && p.views.iter().any(|view| valid_view(base, *view, p.system))
        })
    {
        return Ok(());
    }
    let wanted_system = (base + SYSTEM_VTABLE) as u32;
    let wanted_view = (base + VIEW_VTABLE) as u32;
    let mut systems = Vec::new();
    let latest = LATEST_SYSTEM.load(Ordering::Acquire);
    if valid_system(base, latest) {
        systems.push(latest);
    }
    let mut views = Vec::new();
    let mut address = 0x10000_usize;
    while address < 0x8000_0000 {
        let mut info = MemoryInformation::default();
        if unsafe {
            VirtualQuery(
                address as *const c_void,
                &mut info,
                std::mem::size_of::<MemoryInformation>(),
            )
        } == 0
        {
            break;
        }
        let Some(end) = info.base.checked_add(info.region_size) else {
            break;
        };
        if end <= address {
            break;
        }
        if info.state == 0x1000
            && info.memory_type == 0x20000
            && matches!(info.protect, 4 | 8 | 64 | 128)
        {
            let mut chunk = info.base;
            while chunk < end {
                let length = (end - chunk).min(1_048_576);
                if let Some(bytes) = read(chunk, length) {
                    for (i, value) in bytes.chunks_exact(4).enumerate() {
                        let value = u32::from_le_bytes(value.try_into().unwrap());
                        let candidate = chunk + i * 4;
                        if value == wanted_system && valid_system(base, candidate) {
                            systems.push(candidate);
                        }
                        if value == wanted_view && word(candidate + 12) == Some(candidate) {
                            views.push(candidate);
                        }
                    }
                }
                chunk += length;
            }
        }
        address = end;
    }
    systems.sort_unstable();
    systems.dedup();
    for system in systems {
        let views: Vec<_> = views
            .iter()
            .copied()
            .filter(|view| valid_view(base, *view, system))
            .collect();
        if !views.is_empty() {
            *prepared = Some(Prepared {
                base,
                system,
                views,
            });
            return Ok(());
        }
    }
    RESCAN.store(true, Ordering::Release);
    Err("Игровой интерфейс Gameface ещё не создан".into())
}

#[cfg(target_arch = "x86")]
unsafe extern "thiscall" fn create_system_hook(
    instance: *mut c_void,
    settings: *mut u8,
) -> *mut c_void {
    CREATE_CALLS.fetch_add(1, Ordering::AcqRel);
    if !settings.is_null() {
        let _ = put_word(settings as usize + 0x48, 9444);
        let enabled = 1_u8;
        let mut written = 0;
        unsafe {
            WriteProcessMemory(
                GetCurrentProcess(),
                settings.add(0x4c).cast(),
                (&enabled as *const u8).cast(),
                1,
                &mut written,
            );
        }
    }
    let trampoline = CREATE_TRAMPOLINE.load(Ordering::Acquire);
    let result = if trampoline != 0 {
        let original: unsafe extern "thiscall" fn(*mut c_void, *mut u8) -> *mut c_void =
            unsafe { std::mem::transmute(trampoline) };
        unsafe { original(instance, settings) }
    } else {
        std::ptr::null_mut()
    };
    LATEST_SYSTEM.store(result as usize, Ordering::Release);
    CREATE_CALLS.fetch_sub(1, Ordering::AcqRel);
    result
}

fn write_code(address: usize, bytes: &[u8]) -> Result<(), String> {
    let mut old_protection = 0;
    if unsafe {
        VirtualProtect(
            address as *mut c_void,
            bytes.len(),
            0x40,
            &mut old_protection,
        )
    } == 0
    {
        return Err("Не удалось установить перехват Gameface".into());
    }
    let mut count = 0;
    let written = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *mut c_void,
            bytes.as_ptr().cast(),
            bytes.len(),
            &mut count,
        )
    };
    let mut ignored = 0;
    unsafe {
        FlushInstructionCache(GetCurrentProcess(), address as *const c_void, bytes.len());
        VirtualProtect(
            address as *mut c_void,
            bytes.len(),
            old_protection,
            &mut ignored,
        );
    }
    if written == 0 || count != bytes.len() {
        return Err("Не удалось записать перехват Gameface".into());
    }
    Ok(())
}

pub fn install() -> Result<(), String> {
    #[cfg(not(target_arch = "x86"))]
    {
        return Err("Для RADMIR требуется 32-разрядная сборка OldDialogs".into());
    }
    #[cfg(target_arch = "x86")]
    {
        let base = module()?;
        let mut hook = STARTUP_HOOK.lock().map_err(|_| "Перехват Gameface занят")?;
        if hook.is_some() {
            return Ok(());
        }
        let address = base + CREATE_SYSTEM;
        if read(address, PROLOGUE.len()).as_deref() != Some(&PROLOGUE) {
            return Err("CreateSystem уже изменён другим модулем; перехват не установлен".into());
        }
        let trampoline = unsafe { VirtualAlloc(std::ptr::null_mut(), 11, 0x3000, 0x40) } as usize;
        if trampoline == 0 {
            return Err("Не удалось выделить память перехвата Gameface".into());
        }
        let mut gateway = PROLOGUE.to_vec();
        gateway.push(0xe9);
        gateway
            .extend_from_slice(&((address + 6).wrapping_sub(trampoline + 11) as u32).to_le_bytes());
        if let Err(error) = write_code(trampoline, &gateway) {
            unsafe {
                VirtualFree(trampoline as *mut c_void, 0, 0x8000);
            }
            return Err(error);
        }
        let mut executable = 0;
        unsafe {
            VirtualProtect(trampoline as *mut c_void, 11, 0x20, &mut executable);
        }
        let mut patch = [0x90_u8; 6];
        patch[0] = 0xe9;
        patch[1..5].copy_from_slice(
            &(create_system_hook as *const () as usize)
                .wrapping_sub(address + 5)
                .to_le_bytes()[..4],
        );
        CREATE_TRAMPOLINE.store(trampoline, Ordering::Release);
        if let Err(error) = write_code(address, &patch) {
            CREATE_TRAMPOLINE.store(0, Ordering::Release);
            unsafe {
                VirtualFree(trampoline as *mut c_void, 0, 0x8000);
            }
            return Err(error);
        }
        *hook = Some(StartupHook {
            address,
            trampoline,
            patch,
        });
        Ok(())
    }
}

#[cfg(target_arch = "x86")]
struct NativeScope {
    base: usize,
    entry: [u8; 1],
    temporary: [u32; 1],
}

#[cfg(target_arch = "x86")]
impl NativeScope {
    unsafe fn enter(&mut self) {
        let entry: unsafe extern "thiscall" fn(*mut c_void) =
            unsafe { std::mem::transmute(self.base + ENTRY_CTOR) };
        let temporary: unsafe extern "thiscall" fn(*mut c_void) =
            unsafe { std::mem::transmute(self.base + TEMP_CTOR) };
        unsafe {
            entry(self.entry.as_mut_ptr().cast());
            temporary(self.temporary.as_mut_ptr().cast());
        }
    }
}

#[cfg(target_arch = "x86")]
impl Drop for NativeScope {
    fn drop(&mut self) {
        let temporary: unsafe extern "thiscall" fn(*mut c_void) =
            unsafe { std::mem::transmute(self.base + TEMP_DTOR) };
        let entry: unsafe extern "thiscall" fn(*mut c_void) =
            unsafe { std::mem::transmute(self.base + ENTRY_DTOR) };
        unsafe {
            temporary(self.temporary.as_mut_ptr().cast());
            entry(self.entry.as_mut_ptr().cast());
        }
    }
}

pub fn ensure_started() -> Result<u16, String> {
    #[cfg(not(target_arch = "x86"))]
    {
        return Err("Для RADMIR требуется 32-разрядная сборка OldDialogs".into());
    }
    #[cfg(target_arch = "x86")]
    {
        let base = module()?;
        let dom_thread = word(base + DOM_THREAD_ID).unwrap_or(0) as u32;
        if dom_thread == 0 || dom_thread != unsafe { GetCurrentThreadId() } {
            return Err("Подключение Gameface ожидает основной поток интерфейса игры".into());
        }
        let prepared = PREPARED
            .try_lock()
            .map_err(|_| "Поиск интерфейса Gameface ещё выполняется")?
            .clone()
            .ok_or("Поиск интерфейса Gameface ещё не завершён")?;
        if prepared.base != base || !valid_system(base, prepared.system) {
            invalidate();
            return Err("Игровой интерфейс Gameface был пересоздан".into());
        }
        for (rva, signature) in [
            (
                INSPECTOR_START,
                &[0x55_u8, 0x8b, 0xec, 0x51, 0x51, 0x53][..],
            ),
            (
                INSPECTOR_REGISTER,
                &[0x55, 0x8b, 0xec, 0x51, 0x56, 0x8b][..],
            ),
            (DOCUMENT_LOADED, &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x1c][..]),
        ] {
            if read(base + rva, signature.len()).as_deref() != Some(signature) {
                return Err("Функции инспектора Gameface изменены; подключение отменено".into());
            }
        }
        let server = prepared.system + SYSTEM_INSPECTOR;
        let mut native_scope = NativeScope {
            base,
            entry: [0],
            temporary: [0],
        };
        unsafe {
            native_scope.enter();
        }
        if word(server) == Some(0) {
            let (port, reservation) = (9444_u16..9460)
                .find_map(|port| {
                    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                        .ok()
                        .map(|listener| (port, listener))
                })
                .ok_or("Нет свободного порта инспектора Gameface")?;
            drop(reservation);
            let start: unsafe extern "thiscall" fn(*mut c_void, i32) =
                unsafe { std::mem::transmute(base + INSPECTOR_START) };
            unsafe {
                start(server as *mut c_void, i32::from(port));
            }
        }
        let implementation = word(server)
            .filter(|v| *v != 0)
            .ok_or("Инспектор Gameface не запустился")?;
        let port = word(implementation + 8)
            .filter(|v| (1..=65535).contains(v))
            .ok_or("Порт инспектора Gameface недоступен")? as u16;
        if word(implementation + 12).unwrap_or(0) == 0 {
            return Err("Сервер инспектора Gameface недоступен".into());
        }
        let register: unsafe extern "thiscall" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
        ) -> *mut c_void = unsafe { std::mem::transmute(base + INSPECTOR_REGISTER) };
        let loaded: unsafe extern "thiscall" fn(*mut c_void) =
            unsafe { std::mem::transmute(base + DOCUMENT_LOADED) };
        let mut ready_views = 0usize;
        let mut disabled_views = 0usize;
        let mut pending_views = 0usize;
        for view in prepared.views {
            if !valid_view(base, view, prepared.system) {
                continue;
            }
            let controller = word(view + VIEW_INSPECTOR_CONTROLLER).unwrap_or(0);
            if controller != 0 {
                if word(controller) != Some(base + 0x451e28) {
                    invalidate();
                    return Err("Контроллер инспектора Gameface был пересоздан".into());
                }
                ready_views += 1;
                continue;
            }
            if read(view + VIEW_INSPECTION_ENABLED, 1).as_deref() != Some(&[1]) {
                disabled_views += 1;
                continue;
            }
            if word(view + VIEW_DOCUMENT).unwrap_or(0) == 0
                || word(view + VIEW_SCRIPTING).unwrap_or(0) == 0
            {
                pending_views += 1;
                continue;
            }
            let controller = unsafe {
                register(
                    server as *mut c_void,
                    view as *mut c_void,
                    prepared.system as *mut c_void,
                )
            } as usize;
            if controller == 0 || word(controller) != Some(base + 0x451e28) {
                return Err("Не удалось создать контроллер инспектора Gameface".into());
            }
            put_word(view + VIEW_INSPECTOR_CONTROLLER, controller)?;
            unsafe {
                loaded(controller as *mut c_void);
            }
            ready_views += 1;
        }
        if ready_views == 0 {
            invalidate();
            return Err(format!(
                "Инспектор Gameface: готовых представлений нет; загрузка: {pending_views}, инспекция выключена: {disabled_views}"
            ));
        }
        Ok(port)
    }
}

pub fn uninstall() {
    if let Ok(mut hook) = STARTUP_HOOK.lock() {
        if let Some(installed) = hook.take() {
            if read(installed.address, installed.patch.len()).as_deref() == Some(&installed.patch) {
                let _ = write_code(installed.address, &PROLOGUE);
            }
            if CREATE_CALLS.load(Ordering::Acquire) == 0 {
                CREATE_TRAMPOLINE.store(0, Ordering::Release);
                unsafe {
                    VirtualFree(installed.trampoline as *mut c_void, 0, 0x8000);
                }
            }
        }
    }
    if let Ok(mut prepared) = PREPARED.lock() {
        *prepared = None;
    }
}
