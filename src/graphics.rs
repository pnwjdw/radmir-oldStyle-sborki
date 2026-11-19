use crate::bridge::{Bridge, BridgeStatus, PlayerHudState, VehicleState};
use crate::d3d9::Painter;
use crate::ui::App;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

type Hwnd = isize;
type Lresult = isize;
type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> Lresult;
type EndScene = unsafe extern "system" fn(*mut c_void) -> i32;
type Present = unsafe extern "system" fn(
    *mut c_void,
    *const WinRect,
    *const WinRect,
    Hwnd,
    *const c_void,
) -> i32;
type Reset = unsafe extern "system" fn(*mut c_void, *mut PresentParameters) -> i32;

#[repr(C)]
struct WndClass {
    style: u32,
    wnd_proc: Option<WndProc>,
    cls_extra: i32,
    wnd_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu: *const u16,
    class: *const u16,
}

#[repr(C)]
#[derive(Default)]
pub struct PresentParameters {
    width: u32,
    height: u32,
    format: u32,
    count: u32,
    multisample: u32,
    quality: u32,
    swap_effect: u32,
    window: Hwnd,
    windowed: i32,
    auto_depth: i32,
    depth_format: u32,
    flags: u32,
    refresh: u32,
    interval: u32,
}

#[repr(C)]
#[derive(Default)]
struct CreationParameters {
    adapter: u32,
    kind: u32,
    window: Hwnd,
    flags: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClass) -> u16;
    fn UnregisterClassW(name: *const u16, instance: isize) -> i32;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Hwnd,
        menu: isize,
        instance: isize,
        param: *mut c_void,
    ) -> Hwnd;
    fn DestroyWindow(window: Hwnd) -> i32;
    fn DefWindowProcW(window: Hwnd, msg: u32, w: usize, l: isize) -> Lresult;
    fn CallWindowProcW(
        proc: Option<WndProc>,
        window: Hwnd,
        msg: u32,
        w: usize,
        l: isize,
    ) -> Lresult;
    fn GetWindowLongW(window: Hwnd, index: i32) -> i32;
    fn SetWindowLongW(window: Hwnd, index: i32, value: i32) -> i32;
    fn GetClientRect(window: Hwnd, rect: *mut WinRect) -> i32;
    fn GetDpiForWindow(window: Hwnd) -> u32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn ScreenToClient(window: Hwnd, point: *mut Point) -> i32;
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowThreadProcessId(window: Hwnd, process: *mut u32) -> u32;
    fn IsWindow(window: Hwnd) -> i32;
    fn IsWindowUnicode(window: Hwnd) -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn ClipCursor(rect: *const WinRect) -> i32;
    fn GetClipCursor(rect: *mut WinRect) -> i32;
    fn SetCursor(cursor: isize) -> isize;
    fn LoadCursorW(instance: isize, name: *const u16) -> isize;
    fn OpenClipboard(window: Hwnd) -> i32;
    fn CloseClipboard() -> i32;
    fn GetClipboardData(format: u32) -> isize;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: isize) -> isize;
    fn SetCapture(window: Hwnd) -> Hwnd;
    fn ReleaseCapture() -> i32;
    fn PostMessageW(window: Hwnd, message: u32, w: usize, l: isize) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetModuleFileNameW(module: *mut c_void, path: *mut u16, size: u32) -> u32;
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut isize) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn ReadProcessMemory(
        process: *mut c_void,
        address: *const c_void,
        output: *mut c_void,
        length: usize,
        read: *mut usize,
    ) -> i32;
    fn VirtualProtect(address: *mut c_void, size: usize, protection: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
    fn MultiByteToWideChar(
        page: u32,
        flags: u32,
        source: *const u8,
        source_len: i32,
        dest: *mut u16,
        dest_len: i32,
    ) -> i32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(memory: isize) -> *mut c_void;
    fn GlobalUnlock(memory: isize) -> i32;
    fn GlobalFree(memory: isize) -> isize;
    fn GlobalSize(memory: isize) -> usize;
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

#[link(name = "d3d9")]
unsafe extern "system" {
    fn Direct3DCreate9(version: u32) -> *mut c_void;
}

static FRAME: OnceLock<Mutex<Option<Frame>>> = OnceLock::new();
static INPUT: OnceLock<Mutex<Vec<Event>>> = OnceLock::new();
static STATUS: OnceLock<Mutex<Snapshot>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static VISIBLE: AtomicBool = AtomicBool::new(true);
static RELOAD: AtomicBool = AtomicBool::new(false);
static SNAPSHOT: AtomicBool = AtomicBool::new(false);
static PHASE: AtomicU32 = AtomicU32::new(0);
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static PREVIOUS_PROC: AtomicUsize = AtomicUsize::new(0);
static PRESENT: AtomicUsize = AtomicUsize::new(0);
static RESET: AtomicUsize = AtomicUsize::new(0);
static CALLBACKS: AtomicUsize = AtomicUsize::new(0);
static KEY: AtomicU32 = AtomicU32::new(0x2d);
static CURSOR: AtomicIsize = AtomicIsize::new(0);
static ERROR: OnceLock<Mutex<String>> = OnceLock::new();
static INSPECTOR_READY: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static RENDER_OK: AtomicBool = AtomicBool::new(false);
static CONNECTION_CANCEL: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
static PRESENT_CALLS: AtomicU64 = AtomicU64::new(0);
static RESET_CALLS: AtomicU64 = AtomicU64::new(0);
static LAST_DEVICE: AtomicUsize = AtomicUsize::new(0);
static RENDER_THREAD: AtomicU32 = AtomicU32::new(0);
static HOOK_INFO: OnceLock<Mutex<serde_json::Value>> = OnceLock::new();
static RUN_STARTED: AtomicU64 = AtomicU64::new(0);
static INPUT_ERROR: OnceLock<Mutex<String>> = OnceLock::new();
const RESTORE_INPUT_MESSAGE: u32 = 0x82d1;
const INSPECTOR_MESSAGE: u32 = 0x82d2;
static INSPECTOR_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

fn inspector_error(value: String) {
    if let Ok(mut error) = INSPECTOR_ERROR
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
    {
        *error = value;
    }
}

fn last_inspector_error() -> String {
    INSPECTOR_ERROR
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
        .map(|error| error.clone())
        .unwrap_or_default()
}

fn start_inspector() {
    match crate::inspector::ensure_started() {
        Ok(_) => {
            inspector_error(String::new());
            INSPECTOR_READY.store(true, Ordering::Release);
        }
        Err(value) => {
            inspector_error(value);
            INSPECTOR_READY.store(false, Ordering::Release);
        }
    }
}

fn input_error(value: String) {
    if let Ok(mut error) = INPUT_ERROR.get_or_init(|| Mutex::new(String::new())).lock() {
        *error = value;
    }
}

#[derive(Default)]
struct Snapshot {
    status: BridgeStatus,
    vehicle: VehicleState,
    player: PlayerHudState,
}

struct Frame {
    app: App,
    context: egui::Context,
    painter: Option<Painter>,
    device: usize,
    start: Instant,
    clip: Option<WinRect>,
    previous_visible: bool,
    last_frame: Instant,
    root: PathBuf,
    dpi: f32,
    backdrop_updated: Instant,
}

unsafe impl Send for Frame {}

fn frames() -> &'static Mutex<Option<Frame>> {
    FRAME.get_or_init(|| Mutex::new(None))
}
fn events() -> &'static Mutex<Vec<Event>> {
    INPUT.get_or_init(|| Mutex::new(Vec::new()))
}
fn snapshots() -> &'static Mutex<Snapshot> {
    STATUS.get_or_init(|| Mutex::new(Snapshot::default()))
}
fn error(message: String) {
    if let Ok(mut value) = ERROR.get_or_init(|| Mutex::new(String::new())).lock() {
        *value = message;
    }
}

pub fn state() -> u32 {
    PHASE.load(Ordering::Acquire)
}
pub fn running() -> bool {
    ENABLED.load(Ordering::Acquire)
}
pub fn set_visible(value: bool) {
    VISIBLE.store(value, Ordering::Release);
}
pub fn reload() {
    RELOAD.store(true, Ordering::Release);
}
pub fn cancel_connection() {
    if let Ok(token) = CONNECTION_CANCEL.get_or_init(|| Mutex::new(None)).lock() {
        if let Some(token) = token.as_ref() {
            token.store(true, Ordering::Release);
        }
    }
}
pub fn snapshot() -> bool {
    if !running() {
        return false;
    }
    SNAPSHOT.store(true, Ordering::Release);
    true
}
pub fn last_error() -> String {
    ERROR
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default()
}

unsafe fn method<T: Copy>(object: *mut c_void, index: usize) -> T {
    unsafe {
        let address = *(*(object as *mut *mut usize)).add(index);
        std::mem::transmute_copy(&address)
    }
}

struct Slot {
    address: usize,
    original: usize,
    replacement: usize,
}

impl Slot {
    unsafe fn install(
        object: *mut c_void,
        index: usize,
        replacement: usize,
    ) -> Result<Self, String> {
        unsafe {
            let address = (*(object as *mut *mut usize)).add(index);
            let mut previous = 0;
            if VirtualProtect(
                address.cast(),
                std::mem::size_of::<usize>(),
                0x04,
                &mut previous,
            ) == 0
            {
                return Err("Direct3D9: cannot change vtable protection".into());
            }
            let original = (&*address.cast::<AtomicUsize>()).swap(replacement, Ordering::SeqCst);
            let mut ignored = 0;
            VirtualProtect(
                address.cast(),
                std::mem::size_of::<usize>(),
                previous,
                &mut ignored,
            );
            FlushInstructionCache(
                usize::MAX as *mut c_void,
                address.cast(),
                std::mem::size_of::<usize>(),
            );
            Ok(Self {
                address: address as usize,
                original,
                replacement,
            })
        }
    }

    unsafe fn restore(&self) {
        unsafe {
            let mut previous = 0;
            let pointer = self.address as *mut AtomicUsize;
            if VirtualProtect(
                pointer.cast(),
                std::mem::size_of::<usize>(),
                0x04,
                &mut previous,
            ) != 0
            {
                let _ = (*pointer).compare_exchange(
                    self.replacement,
                    self.original,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                let mut ignored = 0;
                VirtualProtect(
                    pointer.cast(),
                    std::mem::size_of::<usize>(),
                    previous,
                    &mut ignored,
                );
            }
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        unsafe {
            self.restore();
        }
    }
}

struct Callback;
struct Scene {
    device: *mut c_void,
    end: EndScene,
    ended: bool,
}

impl Scene {
    fn finish(mut self) -> i32 {
        self.ended = true;
        unsafe { (self.end)(self.device) }
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        if !self.ended {
            unsafe {
                (self.end)(self.device);
            }
        }
    }
}
impl Callback {
    fn enter() -> Self {
        CALLBACKS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}
impl Drop for Callback {
    fn drop(&mut self) {
        CALLBACKS.fetch_sub(1, Ordering::AcqRel);
    }
}

unsafe extern "system" fn present(
    device: *mut c_void,
    source: *const WinRect,
    destination: *const WinRect,
    window: Hwnd,
    dirty: *const c_void,
) -> i32 {
    let _call = Callback::enter();
    PRESENT_CALLS.fetch_add(1, Ordering::Relaxed);
    LAST_DEVICE.store(device as usize, Ordering::Release);
    RENDER_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    if !ENABLED.load(Ordering::Acquire) && crate::game_input::active() {
        if let Err(value) = crate::game_input::shutdown() {
            input_error(value);
        }
    }
    if ENABLED.load(Ordering::Acquire)
        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            render(device);
        }))
        .is_err()
    {
        error("Rust render callback panicked".into());
        ENABLED.store(false, Ordering::Release);
    }
    let address = PRESENT.load(Ordering::Acquire);
    if address == 0 {
        return 0;
    }
    let original: Present = unsafe { std::mem::transmute(address) };
    let result = unsafe { original(device, source, destination, window, dirty) };
    if result < 0 {
        RENDER_OK.store(false, Ordering::Release);
        if ENABLED.load(Ordering::Acquire) {
            PHASE.store(2, Ordering::Release);
        }
    }
    result
}

unsafe extern "system" fn reset(device: *mut c_void, parameters: *mut PresentParameters) -> i32 {
    let _call = Callback::enter();
    RESET_CALLS.fetch_add(1, Ordering::Relaxed);
    RENDER_OK.store(false, Ordering::Release);
    if ENABLED.load(Ordering::Acquire) {
        PHASE.store(2, Ordering::Release);
    }
    if let Ok(mut frame) = frames().lock() {
        if let Some(frame) = frame.as_mut() {
            if frame.device == device as usize {
                if let Some(painter) = frame.painter.as_mut() {
                    painter.invalidate();
                }
            }
        }
    }
    let address = RESET.load(Ordering::Acquire);
    if address == 0 {
        return -2147467259;
    }
    let original: Reset = unsafe { std::mem::transmute(address) };
    unsafe { original(device, parameters) }
}

fn modifiers() -> Modifiers {
    unsafe {
        Modifiers {
            alt: GetKeyState(0x12) < 0,
            ctrl: GetKeyState(0x11) < 0,
            shift: GetKeyState(0x10) < 0,
            mac_cmd: false,
            command: GetKeyState(0x11) < 0,
        }
    }
}

struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

fn clipboard_text(window: Hwnd) -> Option<String> {
    unsafe {
        if OpenClipboard(window) == 0 {
            return None;
        }
        let _clipboard = Clipboard;
        let memory = GetClipboardData(13);
        if memory == 0 {
            return None;
        }
        let length = (GlobalSize(memory) / 2).min(8_388_608);
        let pointer = GlobalLock(memory).cast::<u16>();
        if pointer.is_null() {
            return None;
        }
        let values = std::slice::from_raw_parts(pointer, length);
        let end = values
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(length);
        let result = String::from_utf16_lossy(&values[..end]);
        GlobalUnlock(memory);
        Some(result)
    }
}

fn copy_text(window: Hwnd, text: &str) {
    unsafe {
        let values: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let memory = GlobalAlloc(0x02, values.len() * 2);
        if memory == 0 {
            return;
        }
        let pointer = GlobalLock(memory).cast::<u16>();
        if pointer.is_null() {
            GlobalFree(memory);
            return;
        }
        std::ptr::copy_nonoverlapping(values.as_ptr(), pointer, values.len());
        GlobalUnlock(memory);
        if OpenClipboard(window) == 0 {
            GlobalFree(memory);
            return;
        }
        let _clipboard = Clipboard;
        if EmptyClipboard() == 0 || SetClipboardData(13, memory) == 0 {
            GlobalFree(memory);
        }
    }
}

fn cursor(icon: egui::CursorIcon) -> isize {
    let id = match icon {
        egui::CursorIcon::None => return 0,
        egui::CursorIcon::PointingHand => 32649,
        egui::CursorIcon::Text | egui::CursorIcon::VerticalText => 32513,
        egui::CursorIcon::Crosshair | egui::CursorIcon::Cell => 32515,
        egui::CursorIcon::Wait => 32514,
        egui::CursorIcon::Progress => 32650,
        egui::CursorIcon::NotAllowed | egui::CursorIcon::NoDrop => 32648,
        egui::CursorIcon::ResizeHorizontal
        | egui::CursorIcon::ResizeEast
        | egui::CursorIcon::ResizeWest
        | egui::CursorIcon::ResizeColumn => 32644,
        egui::CursorIcon::ResizeVertical
        | egui::CursorIcon::ResizeNorth
        | egui::CursorIcon::ResizeSouth
        | egui::CursorIcon::ResizeRow => 32645,
        egui::CursorIcon::ResizeNwSe
        | egui::CursorIcon::ResizeNorthWest
        | egui::CursorIcon::ResizeSouthEast => 32642,
        egui::CursorIcon::ResizeNeSw
        | egui::CursorIcon::ResizeNorthEast
        | egui::CursorIcon::ResizeSouthWest => 32643,
        egui::CursorIcon::Move
        | egui::CursorIcon::AllScroll
        | egui::CursorIcon::Grab
        | egui::CursorIcon::Grabbing => 32646,
        _ => 32512,
    };
    unsafe { LoadCursorW(0, id as *const u16) }
}

fn keyboard_key(value: usize) -> Option<Key> {
    Some(match value {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0d => Key::Enter,
        0x1b => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::ArrowLeft,
        0x26 => Key::ArrowUp,
        0x27 => Key::ArrowRight,
        0x28 => Key::ArrowDown,
        0x2d => Key::Insert,
        0x2e => Key::Delete,
        0x30 => Key::Num0,
        0x31 => Key::Num1,
        0x32 => Key::Num2,
        0x33 => Key::Num3,
        0x34 => Key::Num4,
        0x35 => Key::Num5,
        0x36 => Key::Num6,
        0x37 => Key::Num7,
        0x38 => Key::Num8,
        0x39 => Key::Num9,
        0x41 => Key::A,
        0x42 => Key::B,
        0x43 => Key::C,
        0x44 => Key::D,
        0x45 => Key::E,
        0x46 => Key::F,
        0x47 => Key::G,
        0x48 => Key::H,
        0x49 => Key::I,
        0x4a => Key::J,
        0x4b => Key::K,
        0x4c => Key::L,
        0x4d => Key::M,
        0x4e => Key::N,
        0x4f => Key::O,
        0x50 => Key::P,
        0x51 => Key::Q,
        0x52 => Key::R,
        0x53 => Key::S,
        0x54 => Key::T,
        0x55 => Key::U,
        0x56 => Key::V,
        0x57 => Key::W,
        0x58 => Key::X,
        0x59 => Key::Y,
        0x5a => Key::Z,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7a => Key::F11,
        0x7b => Key::F12,
        _ => return None,
    })
}

unsafe extern "system" fn procedure(window: Hwnd, message: u32, w: usize, l: isize) -> Lresult {
    let _call = Callback::enter();
    if message == INSPECTOR_MESSAGE {
        if ENABLED.load(Ordering::Acquire) && !INSPECTOR_READY.load(Ordering::Acquire) {
            start_inspector();
        }
        return 0;
    }
    if message == RESTORE_INPUT_MESSAGE {
        if crate::game_input::thread_id() == unsafe { GetCurrentThreadId() } {
            if let Err(value) = crate::game_input::shutdown() {
                input_error(value);
            }
        }
        unsafe {
            ReleaseCapture();
            ClipCursor(std::ptr::null());
        }
        return 0;
    }
    if message == 0x08 || (message == 0x1c && w == 0) || (message == 0x06 && (w & 0xffff) == 0) {
        if crate::game_input::thread_id() == unsafe { GetCurrentThreadId() } {
            if let Err(value) = crate::game_input::shutdown() {
                input_error(value);
            }
        }
        unsafe {
            ReleaseCapture();
            ClipCursor(std::ptr::null());
        }
        if let Ok(mut pending) = events().lock() {
            pending.clear();
            pending.push(Event::PointerGone);
        }
    }
    if ENABLED.load(Ordering::Acquire)
        && w == KEY.load(Ordering::Acquire) as usize
        && matches!(message, 0x100 | 0x101 | 0x104 | 0x105)
    {
        if matches!(message, 0x100 | 0x104) && (l as usize & (1 << 30)) == 0 {
            VISIBLE.fetch_xor(true, Ordering::AcqRel);
        }
        return 0;
    }
    let open = VISIBLE.load(Ordering::Acquire) && ENABLED.load(Ordering::Acquire);
    if message == 0x20 && open {
        unsafe {
            SetCursor(CURSOR.load(Ordering::Acquire));
        }
        return 1;
    }
    if open {
        let position = Pos2::new(
            (l as u16 as i16) as f32,
            ((l as usize >> 16) as u16 as i16) as f32,
        );
        let event = match message {
            0x200 => Some(Event::PointerMoved(position)),
            0x201 | 0x202 | 0x203 | 0x204 | 0x205 | 0x206 | 0x207 | 0x208 | 0x209 | 0x20b
            | 0x20c | 0x20d => Some(Event::PointerButton {
                pos: position,
                button: match message {
                    0x201..=0x203 => PointerButton::Primary,
                    0x204..=0x206 => PointerButton::Secondary,
                    0x207..=0x209 => PointerButton::Middle,
                    _ if w >> 16 == 1 => PointerButton::Extra1,
                    _ => PointerButton::Extra2,
                },
                pressed: matches!(
                    message,
                    0x201 | 0x203 | 0x204 | 0x206 | 0x207 | 0x209 | 0x20b | 0x20d
                ),
                modifiers: modifiers(),
            }),
            0x20a | 0x20e => {
                let delta = ((w >> 16) as u16 as i16) as f32 / 3.0;
                Some(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: if message == 0x20e {
                        Vec2::new(delta, 0.0)
                    } else {
                        Vec2::new(0.0, delta)
                    },
                    modifiers: modifiers(),
                })
            }
            0x100 | 0x104 if modifiers().command && w == 0x43 => Some(Event::Copy),
            0x100 | 0x104 if modifiers().command && w == 0x58 => Some(Event::Cut),
            0x100 | 0x104 if modifiers().command && w == 0x56 => {
                clipboard_text(window).map(Event::Paste)
            }
            0x100 | 0x101 | 0x104 | 0x105 => keyboard_key(w).map(|key| Event::Key {
                key,
                physical_key: None,
                pressed: message == 0x100 || message == 0x104,
                repeat: (l as usize & (1 << 30)) != 0,
                modifiers: modifiers(),
            }),
            0x102 => {
                let code = if unsafe { IsWindowUnicode(window) } == 0 && (0x80..=255).contains(&w) {
                    let source = w as u8;
                    let mut value = 0u16;
                    unsafe {
                        MultiByteToWideChar(1251, 0, &source, 1, &mut value, 1);
                    }
                    u32::from(value)
                } else {
                    w as u32
                };
                char::from_u32(code)
                    .filter(|c| !c.is_control())
                    .map(|c| Event::Text(c.to_string()))
            }
            0x02a3 => Some(Event::PointerGone),
            _ => None,
        };
        if let Some(event) = event {
            if matches!(&event, Event::PointerButton { pressed: true, .. }) {
                unsafe {
                    SetCapture(window);
                }
            }
            if matches!(&event, Event::PointerButton { pressed: false, .. }) {
                unsafe {
                    ReleaseCapture();
                }
            }
            if let Ok(mut input) = events().lock() {
                if input.len() < 8192 {
                    input.push(event);
                }
            }
        }
        if message == 0xff {
            return unsafe { DefWindowProcW(window, message, w, l) };
        }
        if (0x200..=0x20e).contains(&message) || (0x100..=0x109).contains(&message) {
            return 0;
        }
    }
    let previous = PREVIOUS_PROC.load(Ordering::Acquire);
    if previous == 0 {
        return unsafe { DefWindowProcW(window, message, w, l) };
    }
    unsafe {
        CallWindowProcW(
            Some(std::mem::transmute::<usize, WndProc>(previous)),
            window,
            message,
            w,
            l,
        )
    }
}

unsafe fn attach_window(device: *mut c_void) -> Result<Hwnd, String> {
    unsafe {
        let get: unsafe extern "system" fn(*mut c_void, *mut CreationParameters) -> i32 =
            method(device, 9);
        let mut parameters = CreationParameters::default();
        if get(device, &mut parameters) < 0 || parameters.window == 0 {
            return Err("Direct3D9 focus window unavailable".into());
        }
        let existing = WINDOW.load(Ordering::Acquire);
        if existing != parameters.window {
            if existing != 0 {
                restore_window();
            }
            let previous = GetWindowLongW(parameters.window, -4) as u32 as usize;
            PREVIOUS_PROC.store(previous, Ordering::Release);
            WINDOW.store(parameters.window, Ordering::Release);
            let installed = SetWindowLongW(
                parameters.window,
                -4,
                procedure as *const () as usize as i32,
            );
            if installed == 0 {
                WINDOW.store(0, Ordering::Release);
                PREVIOUS_PROC.store(0, Ordering::Release);
                return Err("Cannot install game window procedure".into());
            }
        }
        Ok(parameters.window)
    }
}

unsafe fn restore_window() {
    unsafe {
        let window = WINDOW.swap(0, Ordering::AcqRel);
        let previous = PREVIOUS_PROC.load(Ordering::Acquire);
        if window != 0
            && previous != 0
            && IsWindow(window) != 0
            && GetWindowLongW(window, -4) as u32 as usize == procedure as *const () as usize
        {
            SetWindowLongW(window, -4, previous as i32);
        }
    }
}

unsafe fn render(device: *mut c_void) {
    let Ok(mut guard) = frames().try_lock() else {
        return;
    };
    let Some(frame) = guard.as_mut() else {
        return;
    };
    if frame.last_frame.elapsed() < Duration::from_millis(3) {
        return;
    }
    frame.last_frame = Instant::now();
    RENDER_OK.store(false, Ordering::Release);
    static INSPECTOR_NEXT: OnceLock<Mutex<Instant>> = OnceLock::new();
    if !INSPECTOR_READY.load(Ordering::Acquire) {
        if let Ok(mut next) = INSPECTOR_NEXT
            .get_or_init(|| Mutex::new(Instant::now()))
            .try_lock()
        {
            if Instant::now() >= *next {
                start_inspector();
                let window = WINDOW.load(Ordering::Acquire);
                if !INSPECTOR_READY.load(Ordering::Acquire) && window != 0 {
                    unsafe {
                        PostMessageW(window, INSPECTOR_MESSAGE, 0, 0);
                    }
                }
                *next = Instant::now() + Duration::from_secs(3);
            }
        }
    }
    let window = match unsafe { attach_window(device) } {
        Ok(value) => value,
        Err(value) => {
            error(value);
            return;
        }
    };
    let mut bounds = WinRect::default();
    if unsafe { GetClientRect(window, &mut bounds) } == 0 {
        return;
    }
    let width = (bounds.right - bounds.left).max(0) as u32;
    let height = (bounds.bottom - bounds.top).max(0) as u32;
    if width == 0 || height == 0 {
        return;
    }
    let dpi = (unsafe { GetDpiForWindow(window) } as f32 / 96.0).clamp(0.5, 4.0);
    frame.dpi = dpi;
    let open = VISIBLE.load(Ordering::Acquire);
    let focused = unsafe { GetForegroundWindow() } == window;
    let capture_input = open && focused;
    match crate::game_input::update(capture_input) {
        Ok(()) => input_error(String::new()),
        Err(value) => input_error(value),
    }
    if capture_input != frame.previous_visible {
        unsafe {
            if capture_input {
                let mut clip = WinRect::default();
                if GetClipCursor(&mut clip) != 0 {
                    frame.clip = Some(clip);
                }
                ClipCursor(std::ptr::null());
                SetCursor(LoadCursorW(0, 32512usize as *const u16));
            } else {
                ReleaseCapture();
                if let Some(clip) = frame.clip.take() {
                    if focused {
                        ClipCursor(&clip);
                    } else {
                        ClipCursor(std::ptr::null());
                    }
                }
            }
        }
        frame.previous_visible = capture_input;
    }
    if capture_input {
        unsafe {
            ClipCursor(std::ptr::null());
        }
    }
    frame.app.menu_open = open;
    if RELOAD.swap(false, Ordering::AcqRel) {
        match crate::settings::Config::load(&frame.root) {
            Ok(value) => {
                frame.app.config = value;
                frame.app.revision += 1;
            }
            Err(value) => error(value),
        }
    }
    let snapshot = snapshots()
        .lock()
        .ok()
        .map(|value| Snapshot {
            status: value.status.clone(),
            vehicle: value.vehicle.clone(),
            player: value.player.clone(),
        })
        .unwrap_or_default();
    let mut input = RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(width as f32 / dpi, height as f32 / dpi),
        )),
        time: Some(frame.start.elapsed().as_secs_f64()),
        modifiers: modifiers(),
        focused,
        ..Default::default()
    };
    if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
        viewport.native_pixels_per_point = Some(dpi);
    }
    if let Ok(mut pending) = events().lock() {
        input.events = std::mem::take(&mut *pending);
    }
    for event in &mut input.events {
        match event {
            Event::PointerMoved(position) | Event::PointerButton { pos: position, .. } => {
                position.x /= dpi;
                position.y /= dpi;
            }
            _ => {}
        }
    }
    if capture_input {
        let mut point = Point::default();
        unsafe {
            GetCursorPos(&mut point);
            ScreenToClient(window, &mut point);
        }
        input.events.push(Event::PointerMoved(Pos2::new(
            point.x as f32 / dpi,
            point.y as f32 / dpi,
        )));
    }
    if frame.painter.is_none() {
        frame.painter = match unsafe { Painter::new(device) } {
            Ok(value) => Some(value),
            Err(value) => {
                error(value);
                None
            }
        };
        frame.device = device as usize;
    } else if frame.device != device as usize {
        if let Some(painter) = frame.painter.as_mut() {
            if let Err(value) = unsafe { painter.set_device(device) } {
                error(value);
                return;
            }
        }
        frame.device = device as usize;
    }
    if open && frame.backdrop_updated.elapsed() >= Duration::from_millis(100) {
        let down_width = width.div_ceil(4);
        let down_height = height.div_ceil(4);
        if let Ok((w, h, rgba)) =
            unsafe { read_backbuffer(device, Some((down_width, down_height))) }
        {
            if let Some(image) = image::RgbaImage::from_raw(w, h, rgba) {
                let blurred = image::imageops::blur(&image, 3.7272);
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    blurred.as_raw(),
                );
                if let Some(texture) = frame.app.backdrop.as_mut() {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    frame.app.backdrop = Some(frame.context.load_texture(
                        "scene-backdrop",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
        }
        frame.backdrop_updated = Instant::now();
    }
    let output = frame.context.run(input, |ctx| {
        frame
            .app
            .draw(ctx, &snapshot.status, &snapshot.vehicle, &snapshot.player)
    });
    for command in &output.platform_output.commands {
        if let egui::OutputCommand::CopyText(text) = command {
            copy_text(window, text);
        }
    }
    if capture_input {
        let pointer = cursor(output.platform_output.cursor_icon);
        CURSOR.store(pointer, Ordering::Release);
        unsafe {
            SetCursor(pointer);
        }
    }
    if frame.app.menu_open != open {
        VISIBLE.store(frame.app.menu_open, Ordering::Release);
    }
    let primitives = frame
        .context
        .tessellate(output.shapes, output.pixels_per_point);
    let painted = if let Some(painter) = frame.painter.as_mut() {
        let begin: EndScene = unsafe { method(device, 41) };
        let end: EndScene = unsafe { method(device, 42) };
        let result = unsafe { begin(device) };
        if result >= 0 {
            let scene = Scene {
                device,
                end,
                ended: false,
            };
            let result = unsafe {
                painter.paint(
                    &primitives,
                    &output.textures_delta,
                    output.pixels_per_point,
                    width,
                    height,
                )
            };
            let ended = scene.finish();
            match result {
                Ok(()) if ended >= 0 => true,
                Ok(()) => {
                    error(format!("Direct3D9 EndScene: 0x{:08X}", ended as u32));
                    false
                }
                Err(value) => {
                    error(value);
                    false
                }
            }
        } else {
            let _ = unsafe {
                painter.paint(
                    &[],
                    &output.textures_delta,
                    output.pixels_per_point,
                    width,
                    height,
                )
            };
            error(format!("Direct3D9 BeginScene: 0x{:08X}", result as u32));
            false
        }
    } else {
        false
    };
    RENDER_OK.store(painted, Ordering::Release);
    if SNAPSHOT.swap(false, Ordering::AcqRel) {
        if let Err(value) = unsafe { capture(device, &frame.root.join("snapshot.png")) } {
            error(value);
        }
    }
    KEY.store(
        frame
            .app
            .config
            .data
            .get("hotkey")
            .and_then(|v| v.as_u64())
            .unwrap_or(0x2d) as u32,
        Ordering::Release,
    );
    if painted {
        PHASE.fetch_max(3, Ordering::AcqRel);
        FRAMES.fetch_add(1, Ordering::Relaxed);
    } else {
        PHASE.store(2, Ordering::Release);
    }
}

#[repr(C)]
#[derive(Default)]
struct SurfaceDescription {
    format: u32,
    kind: u32,
    usage: u32,
    pool: u32,
    multisample: u32,
    quality: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct LockedSurface {
    pitch: i32,
    pixels: *mut c_void,
}

struct Surface(*mut c_void);
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            crate::d3d9::release(self.0);
        }
    }
}

unsafe fn capture(device: *mut c_void, path: &std::path::Path) -> Result<(), String> {
    let (width, height, rgba) = unsafe { read_backbuffer(device, None) }?;
    image::save_buffer_with_format(
        path,
        &rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|value| value.to_string())
}

unsafe fn read_backbuffer(
    device: *mut c_void,
    requested: Option<(u32, u32)>,
) -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        let get: unsafe extern "system" fn(*mut c_void, u32, u32, u32, *mut *mut c_void) -> i32 =
            method(device, 18);
        let mut pointer = std::ptr::null_mut();
        if get(device, 0, 0, 0, &mut pointer) < 0 || pointer.is_null() {
            return Err("Cannot read Direct3D9 backbuffer".into());
        }
        let source = Surface(pointer);
        let describe: unsafe extern "system" fn(*mut c_void, *mut SurfaceDescription) -> i32 =
            method(source.0, 12);
        let mut description = SurfaceDescription::default();
        if describe(source.0, &mut description) < 0 || !matches!(description.format, 21 | 22) {
            return Err("Unsupported screenshot surface format".into());
        }
        let (width, height) = requested.unwrap_or((description.width, description.height));
        if width == 0 || height == 0 {
            return Err("Empty screenshot dimensions".into());
        }
        let length = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= 134_217_728)
            .ok_or("Screenshot dimensions exceed limit")?;
        let resolved = if description.multisample != 0 {
            let create: unsafe extern "system" fn(
                *mut c_void,
                u32,
                u32,
                u32,
                u32,
                u32,
                i32,
                *mut *mut c_void,
                *mut c_void,
            ) -> i32 = method(device, 28);
            let stretch: unsafe extern "system" fn(
                *mut c_void,
                *mut c_void,
                *const WinRect,
                *mut c_void,
                *const WinRect,
                u32,
            ) -> i32 = method(device, 34);
            let mut pointer = std::ptr::null_mut();
            if create(
                device,
                description.width,
                description.height,
                description.format,
                0,
                0,
                0,
                &mut pointer,
                std::ptr::null_mut(),
            ) < 0
                || pointer.is_null()
            {
                return Err("Cannot resolve screenshot surface".into());
            }
            let surface = Surface(pointer);
            if stretch(
                device,
                source.0,
                std::ptr::null(),
                surface.0,
                std::ptr::null(),
                0,
            ) < 0
            {
                return Err("Cannot resolve screenshot multisampling".into());
            }
            Some(surface)
        } else {
            None
        };
        let resized = if width != description.width || height != description.height {
            let create: unsafe extern "system" fn(
                *mut c_void,
                u32,
                u32,
                u32,
                u32,
                u32,
                i32,
                *mut *mut c_void,
                *mut c_void,
            ) -> i32 = method(device, 28);
            let stretch: unsafe extern "system" fn(
                *mut c_void,
                *mut c_void,
                *const WinRect,
                *mut c_void,
                *const WinRect,
                u32,
            ) -> i32 = method(device, 34);
            let mut pointer = std::ptr::null_mut();
            if create(
                device,
                width,
                height,
                description.format,
                0,
                0,
                0,
                &mut pointer,
                std::ptr::null_mut(),
            ) < 0
                || pointer.is_null()
            {
                return Err("Cannot allocate backdrop surface".into());
            }
            let surface = Surface(pointer);
            if stretch(
                device,
                resolved.as_ref().map(|v| v.0).unwrap_or(source.0),
                std::ptr::null(),
                surface.0,
                std::ptr::null(),
                2,
            ) < 0
            {
                return Err("Cannot downsample backdrop".into());
            }
            Some(surface)
        } else {
            None
        };
        let create: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            u32,
            u32,
            *mut *mut c_void,
            *mut c_void,
        ) -> i32 = method(device, 36);
        let download: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 =
            method(device, 32);
        let mut pointer = std::ptr::null_mut();
        if create(
            device,
            width,
            height,
            description.format,
            2,
            &mut pointer,
            std::ptr::null_mut(),
        ) < 0
            || pointer.is_null()
        {
            return Err("Cannot allocate screenshot surface".into());
        }
        let cpu = Surface(pointer);
        if download(
            device,
            resized
                .as_ref()
                .or(resolved.as_ref())
                .map(|v| v.0)
                .unwrap_or(source.0),
            cpu.0,
        ) < 0
        {
            return Err("Cannot download screenshot".into());
        }
        let lock: unsafe extern "system" fn(
            *mut c_void,
            *mut LockedSurface,
            *const WinRect,
            u32,
        ) -> i32 = method(cpu.0, 13);
        let unlock: unsafe extern "system" fn(*mut c_void) -> i32 = method(cpu.0, 14);
        let mut mapped = LockedSurface {
            pitch: 0,
            pixels: std::ptr::null_mut(),
        };
        if lock(cpu.0, &mut mapped, std::ptr::null(), 0x10) < 0 {
            return Err("Cannot map screenshot".into());
        }
        if mapped.pixels.is_null() || mapped.pitch < (width * 4) as i32 {
            unlock(cpu.0);
            return Err("Invalid screenshot pitch".into());
        }
        let mut rgba = Vec::with_capacity(length);
        for row in 0..height as usize {
            let pixels = std::slice::from_raw_parts(
                (mapped.pixels as *const u8).add(row * mapped.pitch as usize),
                width as usize * 4,
            );
            for pixel in pixels.chunks_exact(4) {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
            }
        }
        unlock(cpu.0);
        Ok((width, height, rgba))
    }
}

fn memory_bytes(address: usize, length: usize) -> Option<Vec<u8>> {
    if address < 0x10000 || length == 0 || address.checked_add(length).is_none() {
        return None;
    }
    let mut bytes = vec![0u8; length];
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

fn memory_word(address: usize) -> Option<usize> {
    let bytes = memory_bytes(address, 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?) as usize)
}

fn module_handle(name: &str) -> usize {
    let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { GetModuleHandleW(name.as_ptr()) as usize }
}

fn module_path(module: usize) -> Option<PathBuf> {
    let mut name = vec![0u16; 32768];
    let length =
        unsafe { GetModuleFileNameW(module as *mut c_void, name.as_mut_ptr(), name.len() as u32) }
            as usize;
    (length != 0 && length < name.len())
        .then(|| PathBuf::from(String::from_utf16_lossy(&name[..length])))
}

fn address_info(address: usize) -> serde_json::Value {
    let mut module = 0isize;
    let result = unsafe { GetModuleHandleExW(6, address as *const u16, &mut module) };
    serde_json::json!({"address":format!("0x{address:08X}"),"module":if result!=0{module_path(module as usize).map(|p|p.to_string_lossy().into_owned())}else{None},"offset":if result!=0{Some(format!("0x{:X}",address.saturating_sub(module as usize)))}else{None}})
}

fn file_sha256(path: &std::path::Path) -> Result<String, String> {
    if std::fs::metadata(path)
        .map_err(|value| value.to_string())?
        .len()
        > 134_217_728
    {
        return Err("Game module is too large to validate".into());
    }
    let bytes = std::fs::read(path).map_err(|value| value.to_string())?;
    let name: Vec<u16> = "SHA256\0".encode_utf16().collect();
    let mut algorithm = std::ptr::null_mut();
    if unsafe { BCryptOpenAlgorithmProvider(&mut algorithm, name.as_ptr(), std::ptr::null(), 0) }
        < 0
    {
        return Err("SHA-256 is unavailable for the game device validation".into());
    }
    let mut digest = [0u8; 32];
    let result = unsafe {
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
    if result < 0 {
        return Err("Cannot validate the game device modules".into());
    }
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

unsafe fn install_proxy_hooks(samp: usize) -> Result<Vec<Slot>, String> {
    let game = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    let game_path = module_path(game).ok_or("Cannot identify the game executable")?;
    let samp_path = module_path(samp).ok_or("Cannot identify the SA-MP module")?;
    let game_hash = file_sha256(&game_path)?;
    let samp_hash = file_sha256(&samp_path)?;
    let mut diagnostic = serde_json::json!({"mode":"samp-proxy","game":game_path,"gameSha256":game_hash,"samp":samp_path,"sampSha256":samp_hash,"global":format!("0x{:08X}",game+0x897c28),"d3d9":format!("0x{:08X}",module_handle("d3d9.dll")),"dxgi":format!("0x{:08X}",module_handle("dxgi.dll")),"d3d11":format!("0x{:08X}",module_handle("d3d11.dll"))});
    if let Ok(mut value) = HOOK_INFO
        .get_or_init(|| Mutex::new(serde_json::Value::Null))
        .lock()
    {
        *value = diagnostic.clone();
    }
    if game_hash != "c5c73f30d8b0341a3d2c9bf4e1d44ae036dfdc2f561e138107f63a17747484e0"
        || samp_hash != "38b9aaa02746cfd1aec09d195e2076cc5e025a137ee8fbb3c403d23c81a4921b"
    {
        return Err("This RADMIR game or SA-MP build has not been validated; the graphics hook was not installed".into());
    }
    let device = memory_word(game + 0x897c28)
        .filter(|pointer| *pointer >= 0x10000 && pointer & 3 == 0)
        .ok_or("The game Direct3D9 device has not been created yet")?;
    let table = memory_word(device).ok_or("Cannot read the game Direct3D9 device")?;
    if table != samp + 0xed8c8 {
        return Err(
            "The game Direct3D9 proxy vtable does not match the validated SA-MP build".into(),
        );
    }
    for (index, rva) in [
        (9, 0x73e60),
        (16, 0x73ed0),
        (17, 0x75020),
        (41, 0x74110),
        (42, 0x74130),
    ] {
        if memory_word(table + index * 4) != Some(samp + rva) {
            return Err(format!(
                "The SA-MP Direct3D9 proxy method {index} has changed; the hook was not installed"
            ));
        }
    }
    let get_parameters: unsafe extern "system" fn(*mut c_void, *mut CreationParameters) -> i32 =
        unsafe { std::mem::transmute(samp + 0x73e60) };
    let mut parameters = CreationParameters::default();
    if unsafe { get_parameters(device as *mut c_void, &mut parameters) } < 0
        || parameters.window == 0
        || unsafe { IsWindow(parameters.window) } == 0
    {
        return Err("The validated game device has no active focus window".into());
    }
    let mut owner = 0;
    let window_thread = unsafe { GetWindowThreadProcessId(parameters.window, &mut owner) };
    if owner != unsafe { GetCurrentProcessId() } {
        return Err("The Direct3D9 device focus window does not belong to this game".into());
    }
    diagnostic["device"] = serde_json::json!(format!("0x{device:08X}"));
    diagnostic["vtable"] = serde_json::json!(format!("0x{table:08X}"));
    diagnostic["focusWindow"] = serde_json::json!(format!("0x{:08X}", parameters.window as usize));
    diagnostic["creationFlags"] = serde_json::json!(parameters.flags);
    diagnostic["windowThread"] = serde_json::json!(window_thread);
    diagnostic["presentOriginal"] = address_info(samp + 0x75020);
    diagnostic["resetOriginal"] = address_info(samp + 0x73ed0);
    let device = device as *mut c_void;
    PRESENT.store(samp + 0x75020, Ordering::Release);
    RESET.store(samp + 0x73ed0, Ordering::Release);
    let present_slot = unsafe { Slot::install(device, 17, present as *const () as usize) }?;
    let reset_slot = unsafe { Slot::install(device, 16, reset as *const () as usize) }?;
    diagnostic["installed"] = serde_json::json!(true);
    if let Ok(mut value) = HOOK_INFO
        .get_or_init(|| Mutex::new(serde_json::Value::Null))
        .lock()
    {
        *value = diagnostic;
    }
    Ok(vec![present_slot, reset_slot])
}

unsafe fn install_graphics_hooks() -> Result<Vec<Slot>, String> {
    let samp = module_handle("samp.dll");
    if samp != 0 {
        return unsafe { install_proxy_hooks(samp) };
    }
    unsafe {
        let name: Vec<u16> = "OldDialogsRadmirD3D9\0".encode_utf16().collect();
        let instance = GetModuleHandleW(std::ptr::null()) as isize;
        let class = WndClass {
            style: 0,
            wnd_proc: Some(DefWindowProcW),
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: 0,
            cursor: 0,
            background: 0,
            menu: std::ptr::null(),
            class: name.as_ptr(),
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            0,
            name.as_ptr(),
            name.as_ptr(),
            0,
            0,
            0,
            8,
            8,
            0,
            0,
            instance,
            std::ptr::null_mut(),
        );
        if window == 0 {
            return Err("Cannot create Direct3D9 initialization window".into());
        }
        let d3d = Direct3DCreate9(32);
        if d3d.is_null() {
            DestroyWindow(window);
            UnregisterClassW(name.as_ptr(), instance);
            return Err("Direct3DCreate9 failed".into());
        }
        let create: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            Hwnd,
            u32,
            *mut PresentParameters,
            *mut *mut c_void,
        ) -> i32 = method(d3d, 16);
        let mut parameters = PresentParameters {
            width: 8,
            height: 8,
            format: 0,
            count: 1,
            swap_effect: 1,
            window,
            windowed: 1,
            ..Default::default()
        };
        let mut device = std::ptr::null_mut();
        let result = create(d3d, 0, 1, window, 0x24, &mut parameters, &mut device);
        let outcome = if result < 0 || device.is_null() {
            Err(format!("Direct3D9 initialization: 0x{:08X}", result as u32))
        } else {
            if let Ok(mut value) = HOOK_INFO
                .get_or_init(|| Mutex::new(serde_json::Value::Null))
                .lock()
            {
                *value = serde_json::json!({"mode":"native-d3d9","device":format!("0x{:08X}",device as usize),"vtable":format!("0x{:08X}",*(device as *const usize))});
            }
            let present_pointer = *(*(device as *mut *mut usize)).add(17);
            let reset_pointer = *(*(device as *mut *mut usize)).add(16);
            PRESENT.store(present_pointer, Ordering::Release);
            RESET.store(reset_pointer, Ordering::Release);
            let present = Slot::install(device, 17, present as *const () as usize);
            match present {
                Ok(present) => match Slot::install(device, 16, reset as *const () as usize) {
                    Ok(reset) => Ok(vec![present, reset]),
                    Err(value) => Err(value),
                },
                Err(value) => Err(value),
            }
        };
        if !device.is_null() {
            let release: unsafe extern "system" fn(*mut c_void) -> u32 = method(device, 2);
            release(device);
        }
        let release: unsafe extern "system" fn(*mut c_void) -> u32 = method(d3d, 2);
        release(d3d);
        DestroyWindow(window);
        UnregisterClassW(name.as_ptr(), instance);
        outcome
    }
}

pub fn run(root: PathBuf, stop: &AtomicBool) -> Result<(), String> {
    if ENABLED.swap(true, Ordering::AcqRel) {
        set_visible(true);
        return Ok(());
    }
    let mut pinned = 0isize;
    if unsafe { GetModuleHandleExW(5, run as *const () as *const u16, &mut pinned) } == 0 {
        ENABLED.store(false, Ordering::Release);
        return Err("Cannot retain the graphics module for callback safety".into());
    }
    RUN_STARTED.store(crate::bridge::ticks(), Ordering::Release);
    PHASE.store(1, Ordering::Release);
    RENDER_OK.store(false, Ordering::Release);
    INSPECTOR_READY.store(false, Ordering::Release);
    FRAMES.store(0, Ordering::Release);
    PRESENT_CALLS.store(0, Ordering::Release);
    RESET_CALLS.store(0, Ordering::Release);
    RENDER_THREAD.store(0, Ordering::Release);
    LAST_DEVICE.store(0, Ordering::Release);
    VISIBLE.store(true, Ordering::Release);
    SNAPSHOT.store(false, Ordering::Release);
    error(String::new());
    input_error(String::new());
    if let Ok(mut snapshot) = snapshots().lock() {
        *snapshot = Snapshot::default();
    }
    let _ = std::fs::write(root.join("last-error.txt"), "");
    inspector_error(String::new());
    let mut inspector_installed = match crate::inspector::install() {
        Ok(()) => true,
        Err(value) => {
            inspector_error(value);
            false
        }
    };
    let mut inspector_prepared = match crate::inspector::prepare() {
        Ok(()) => true,
        Err(value) => {
            inspector_error(value);
            false
        }
    };
    let mut inspector_retry = Instant::now() + Duration::from_secs(3);
    let mut app = App::new(&root);
    app.config.set_font_names(&crate::assets::font_names());
    let context = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = crate::assets::menu_font_bytes(false) {
        fonts.font_data.insert(
            "MenuRegular".into(),
            Arc::new(egui::FontData::from_owned((*bytes).clone())),
        );
        fonts
            .families
            .insert(egui::FontFamily::Proportional, vec!["MenuRegular".into()]);
    }
    context.set_fonts(fonts);
    {
        let mut frame = frames().lock().map_err(|_| "Render lock poisoned")?;
        *frame = Some(Frame {
            app,
            context,
            painter: None,
            device: 0,
            start: Instant::now(),
            clip: None,
            previous_visible: false,
            last_frame: Instant::now() - Duration::from_secs(1),
            root: root.clone(),
            dpi: 1.0,
            backdrop_updated: Instant::now() - Duration::from_secs(1),
        });
    }
    let device_deadline = Instant::now() + Duration::from_secs(120);
    let mut startup_diagnostic = Instant::now() - Duration::from_secs(1);
    loop {
        if !inspector_installed {
            inspector_installed = crate::inspector::install().is_ok();
        }
        let game = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        let samp = module_handle("samp.dll");
        let device =
            memory_word(game + 0x897c28).filter(|pointer| *pointer >= 0x10000 && pointer & 3 == 0);
        let table = device.and_then(memory_word);
        let raw_device_ready = samp != 0
            && memory_word(samp + 0x26e888)
                .filter(|pointer| *pointer >= 0x10000 && pointer & 3 == 0)
                .and_then(memory_word)
                .is_some_and(|table| table >= 0x10000 && table & 3 == 0);
        let methods_ready = samp != 0
            && [
                (9, 0x73e60),
                (16, 0x73ed0),
                (17, 0x75020),
                (41, 0x74110),
                (42, 0x74130),
            ]
            .iter()
            .all(|(index, rva)| {
                table.and_then(|table| memory_word(table + index * 4)) == Some(samp + rva)
            });
        let device_ready = samp != 0
            && module_handle("d3d9.dll") != 0
            && raw_device_ready
            && table == samp.checked_add(0xed8c8)
            && methods_ready;
        if device_ready || stop.load(Ordering::Acquire) {
            break;
        }
        if startup_diagnostic.elapsed() >= Duration::from_secs(1) {
            if let Ok(mut info) = HOOK_INFO
                .get_or_init(|| Mutex::new(serde_json::Value::Null))
                .lock()
            {
                *info = serde_json::json!({"mode":"waiting-samp-proxy","device":device.map(|pointer| format!("0x{pointer:08X}")),"vtable":table.map(|table| format!("0x{table:08X}")),"samp":format!("0x{samp:08X}"),"rawDeviceReady":raw_device_ready,"methodsReady":methods_ready});
            }
            write_runtime(&root, inspector_prepared);
            startup_diagnostic = Instant::now();
        }
        if Instant::now() >= device_deadline {
            ENABLED.store(false, Ordering::Release);
            PHASE.store(0, Ordering::Release);
            frames().lock().ok().and_then(|mut frame| frame.take());
            crate::inspector::uninstall();
            let message =
                "RADMIR не создал проверенную графическую обёртку SA-MP за 120 секунд".to_string();
            error(message.clone());
            write_runtime(&root, inspector_prepared);
            return Err(message);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if stop.load(Ordering::Acquire) {
        ENABLED.store(false, Ordering::Release);
        PHASE.store(0, Ordering::Release);
        frames().lock().ok().and_then(|mut frame| frame.take());
        crate::inspector::uninstall();
        return Ok(());
    }
    let hooks = match unsafe { install_graphics_hooks() } {
        Ok(value) => value,
        Err(value) => {
            ENABLED.store(false, Ordering::Release);
            PHASE.store(0, Ordering::Release);
            frames().lock().ok().and_then(|mut value| value.take());
            crate::inspector::uninstall();
            return Err(value);
        }
    };
    PHASE.store(2, Ordering::Release);
    let mut bridge = Bridge::new(unsafe { GetCurrentProcessId() });
    if let Ok(mut token) = CONNECTION_CANCEL.get_or_init(|| Mutex::new(None)).lock() {
        *token = Some(bridge.cancel_token());
    }
    let mut wire = serde_json::Value::Null;
    let mut wire_revision = 0;
    let mut written_error = String::new();
    let mut diagnostic_at = Instant::now() - Duration::from_secs(2);
    while !stop.load(Ordering::Acquire) && ENABLED.load(Ordering::Acquire) {
        if !INSPECTOR_READY.load(Ordering::Acquire) && Instant::now() >= inspector_retry {
            if !inspector_installed {
                inspector_installed = crate::inspector::install().is_ok();
            }
            inspector_prepared = match crate::inspector::prepare() {
                Ok(()) => true,
                Err(value) => {
                    inspector_error(value);
                    false
                }
            };
            inspector_retry = Instant::now() + Duration::from_secs(3);
        }
        let config = frames().lock().ok().and_then(|mut guard| {
            guard.as_mut().map(|frame| {
                let preview = frame.app.take_preview();
                let refresh = std::mem::take(&mut frame.app.refresh_requested);
                let updated = if frame.app.revision != wire_revision || wire.is_null() {
                    Some(frame.app.config.to_wire())
                } else {
                    None
                };
                (
                    updated,
                    frame.app.revision,
                    preview,
                    refresh,
                    frame.dpi,
                    frame.app.widgets.custom_logo_active,
                )
            })
        });
        if let Some((updated, revision, preview, refresh, dpi, custom_logo)) = config {
            if let Some(updated) = updated {
                wire = updated;
                wire_revision = revision;
            }
            if refresh {
                INSPECTOR_READY.store(false, Ordering::Release);
                crate::inspector::invalidate();
                inspector_retry = Instant::now();
                bridge.refresh();
            }
            if let Some(kind) = preview {
                bridge.preview(kind);
            }
            bridge.tick(&wire, revision, dpi, custom_logo);
        }
        let mut bridge_status = bridge.status();
        if (bridge_status.port == 0 || bridge_status.views.is_empty())
            && INSPECTOR_READY.swap(false, Ordering::AcqRel)
        {
            crate::inspector::invalidate();
        }
        if bridge_status.views.is_empty() {
            let detail = last_inspector_error();
            if !detail.is_empty() {
                bridge_status.error = detail;
            }
        }
        let connected = bridge_status.port != 0
            && !bridge_status.views.is_empty()
            && bridge_status.error.is_empty()
            && bridge_status
                .views
                .iter()
                .all(|view| view.error.is_empty() && view.pending == 0 && view.failed == 0);
        if RENDER_OK.load(Ordering::Acquire) {
            PHASE.store(if connected { 4 } else { 3 }, Ordering::Release);
        }
        if let Ok(mut value) = snapshots().lock() {
            value.status = bridge_status;
            value.vehicle = bridge.vehicle();
            value.player = bridge.player();
        }
        let current_error = last_error();
        if !current_error.is_empty() && current_error != written_error {
            let _ = std::fs::write(root.join("last-error.txt"), &current_error);
            written_error = current_error;
        }
        if diagnostic_at.elapsed() >= Duration::from_secs(1) {
            write_runtime(&root, inspector_prepared);
            diagnostic_at = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    ENABLED.store(false, Ordering::Release);
    bridge.stop();
    if let Ok(mut token) = CONNECTION_CANCEL.get_or_init(|| Mutex::new(None)).lock() {
        *token = None;
    }
    let window = WINDOW.load(Ordering::Acquire);
    if window != 0 {
        unsafe {
            PostMessageW(window, RESTORE_INPUT_MESSAGE, 0, 0);
        }
    }
    while crate::game_input::active() {
        PHASE.store(2, Ordering::Release);
        input_error("Возврат игрового ввода ожидает кадр RADMIR; разверните игру".into());
        write_runtime(&root, inspector_prepared);
        std::thread::sleep(Duration::from_millis(100));
    }
    input_error(String::new());
    drop(hooks);
    crate::inspector::uninstall();
    unsafe {
        restore_window();
    }
    let until = Instant::now() + Duration::from_secs(3);
    while CALLBACKS.load(Ordering::Acquire) != 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(1));
    }
    if CALLBACKS.load(Ordering::Acquire) != 0 {
        return Err("Rendering callback is still active; keep module loaded".into());
    }
    if let Ok(mut frame) = frames().lock() {
        if let Some(mut frame) = frame.take() {
            if let Some(clip) = frame.clip.take() {
                unsafe {
                    if GetForegroundWindow() == window {
                        ClipCursor(&clip);
                    }
                }
            }
            if let Err(value) = frame.app.config.save(&root) {
                error(value);
            }
        }
    }
    PHASE.store(0, Ordering::Release);
    write_runtime(&root, false);
    if stop.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(last_error())
    }
}

fn write_runtime(root: &std::path::Path, prepared: bool) {
    let status = snapshots()
        .lock()
        .map(|value| value.status.clone())
        .unwrap_or_default();
    let views: Vec<_> = status.views.iter().map(|view| serde_json::json!({"id":view.id,"url":view.url,"error":view.error,"enabled":view.enabled,"styles":view.styles,"rules":view.rules,"windows":view.windows,"interfaces":view.interfaces.len(),"elements":view.elements.len(),"applied":view.applied})).collect();
    let value = serde_json::json!({"state":state(),"running":running(),"menu":VISIBLE.load(Ordering::Acquire),"frames":FRAMES.load(Ordering::Relaxed),"inspectorPrepared":prepared,"inspectorReady":INSPECTOR_READY.load(Ordering::Acquire),"port":status.port,"bridgeError":status.error,"renderError":last_error(),"views":views});
    let mut value = value;
    if let Ok(snapshot) = snapshots().lock() {
        value["vehicle"] = serde_json::to_value(&snapshot.vehicle).unwrap_or_default();
        value["player"] = serde_json::json!({"visible":snapshot.player.visible,"inWater":snapshot.player.in_water,"totalAmmo":snapshot.player.total_ammo});
    }
    value["processId"] = serde_json::json!(unsafe { GetCurrentProcessId() });
    value["runStartedTick"] = serde_json::json!(RUN_STARTED.load(Ordering::Acquire));
    value["diagnosticTick"] = serde_json::json!(crate::bridge::ticks());
    value["inspectorError"] = serde_json::json!(last_inspector_error());
    value["inputCaptured"] = serde_json::json!(crate::game_input::active());
    value["inputThread"] = serde_json::json!(crate::game_input::thread_id());
    value["inputError"] = serde_json::json!(
        INPUT_ERROR
            .get_or_init(|| Mutex::new(String::new()))
            .lock()
            .map(|error| error.clone())
            .unwrap_or_default()
    );
    value["presentCallbacks"] = serde_json::json!(PRESENT_CALLS.load(Ordering::Relaxed));
    value["resetCallbacks"] = serde_json::json!(RESET_CALLS.load(Ordering::Relaxed));
    value["renderThread"] = serde_json::json!(RENDER_THREAD.load(Ordering::Acquire));
    value["lastDevice"] =
        serde_json::json!(format!("0x{:08X}", LAST_DEVICE.load(Ordering::Acquire)));
    value["graphics"] = HOOK_INFO
        .get_or_init(|| Mutex::new(serde_json::Value::Null))
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(root.join("runtime.json"), text);
    }
}
