//! Focus guard. Studio (Qt) activates itself when a place finishes loading and gets past
//! the Windows foreground lock; launching inactive and disabling its windows do not stop
//! it (measured 2026-10-30). So the guard watches foreground changes and, when a session
//! Studio comes to the front without Juan choosing it, hands the focus straight back to
//! the window he was in. "Chose it" = mouse button or Alt held at the switch, or input in
//! the last 500 ms with the cursor over that Studio or the taskbar.

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type Hwnd = *mut c_void;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct LastInput {
    size: u32,
    time: u32,
}

#[link(name = "user32")]
extern "system" {
    fn SetWinEventHook(
        min: u32,
        max: u32,
        module: *mut c_void,
        cb: extern "system" fn(*mut c_void, u32, Hwnd, i32, i32, u32, u32),
        pid: u32,
        tid: u32,
        flags: u32,
    ) -> *mut c_void;
    fn GetForegroundWindow() -> Hwnd;
    fn GetMessageW(msg: *mut u8, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetCursorPos(p: *mut Point) -> i32;
    fn WindowFromPoint(p: Point) -> Hwnd;
    fn GetAncestor(hwnd: Hwnd, flags: u32) -> Hwnd;
    fn GetClassNameW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
    fn IsWindow(hwnd: Hwnd) -> i32;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn AttachThreadInput(from: u32, to: u32, attach: i32) -> i32;
    fn GetLastInputInfo(info: *mut LastInput) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn GetTickCount() -> u32;
}

const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const GA_ROOT: u32 = 2;
const VK_LBUTTON: i32 = 0x01;
const VK_RBUTTON: i32 = 0x02;
const VK_MENU: i32 = 0x12;
/// A Studio that keeps grabbing focus is given up on after this many hand-backs per window
/// of time, so the guard can never fight it in a loop.
const MAX_RESTORES: usize = 30;
const RESTORE_WINDOW: Duration = Duration::from_secs(10);

struct State {
    pids: Vec<u32>,
    /// Recent foreground windows that were not guarded Studios, newest last. A history,
    /// not one slot: the newest can die (e.g. a closing Studio's window flashes active).
    prev: Vec<isize>,
    restores: Vec<Instant>,
    paused_logged: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    pids: Vec::new(),
    prev: Vec::new(),
    restores: Vec::new(),
    paused_logged: false,
});

pub fn watch(pid: u32) {
    let mut s = STATE.lock().unwrap();
    if !s.pids.contains(&pid) {
        s.pids.push(pid);
    }
}

pub fn unwatch(pid: u32) {
    STATE.lock().unwrap().pids.retain(|p| *p != pid);
}

pub(crate) fn log(line: &str) {
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".roxlit")
        .join("sessions")
        .join("focus.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {line}", stamp());
    }
}

fn stamp() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    s.to_string()
}

fn pid_of(h: Hwnd) -> (u32, u32) {
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(h, &mut pid) };
    (pid, tid)
}

fn held(key: i32) -> bool {
    unsafe { GetAsyncKeyState(key) as u16 & 0x8000 != 0 }
}

fn idle_ms() -> u32 {
    let mut li = LastInput { size: 8, time: 0 };
    unsafe { GetLastInputInfo(&mut li) };
    unsafe { GetTickCount() }.wrapping_sub(li.time)
}

fn is_taskbar(h: Hwnd) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), 64) };
    let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd"
}

fn user_chose(fg: Hwnd) -> bool {
    if held(VK_LBUTTON) || held(VK_RBUTTON) || held(VK_MENU) {
        return true;
    }
    if idle_ms() > 500 {
        return false;
    }
    let mut p = Point::default();
    unsafe { GetCursorPos(&mut p) };
    let under = unsafe { GetAncestor(WindowFromPoint(p), GA_ROOT) };
    under == fg || is_taskbar(under)
}

extern "system" fn on_foreground(_: *mut c_void, _: u32, fg: Hwnd, _: i32, _: i32, _: u32, _: u32) {
    if fg.is_null() {
        return;
    }
    let (pid, studio_tid) = pid_of(fg);
    let mut s = STATE.lock().unwrap();
    if !s.pids.contains(&pid) {
        s.prev.retain(|h| *h != fg as isize);
        s.prev.push(fg as isize);
        if s.prev.len() > 8 {
            s.prev.remove(0);
        }
        return;
    }
    if user_chose(fg) {
        return;
    }
    s.prev.retain(|h| unsafe { IsWindow(*h as Hwnd) } != 0);
    let Some(prev) = s.prev.last().map(|h| *h as Hwnd) else {
        log(&format!("studio {pid} took focus; no window to return to"));
        return;
    };
    s.restores.retain(|t| t.elapsed() < RESTORE_WINDOW);
    if s.restores.len() >= MAX_RESTORES {
        if !s.paused_logged {
            log(&format!("studio {pid} keeps taking focus; guard paused"));
            s.paused_logged = true;
        }
        return;
    }
    s.paused_logged = false;
    s.restores.push(Instant::now());
    drop(s);
    // Joining Studio's input queue for the call is what lets a background process hand
    // the foreground back; it is detached right after.
    // A hand-back sometimes fails while Studio is mid-activation; retry briefly.
    let me = unsafe { GetCurrentThreadId() };
    let mut tries = 0;
    let ok = loop {
        tries += 1;
        let ok = unsafe {
            AttachThreadInput(me, studio_tid, 1);
            let ok = SetForegroundWindow(prev);
            AttachThreadInput(me, studio_tid, 0);
            ok != 0
        };
        if ok || tries == 3 || pid_of(unsafe { GetForegroundWindow() }).0 != pid {
            break ok;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    log(&format!(
        "studio {pid} took focus; returned ok={ok} tries={tries}"
    ));
}

/// Runs the hook on its own thread with a message loop (out-of-context WinEvents are
/// delivered through it).
pub fn start() {
    std::thread::spawn(|| unsafe {
        STATE
            .lock()
            .unwrap()
            .prev
            .push(GetForegroundWindow() as isize);
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            on_foreground,
            0,
            0,
            0,
        );
        let mut msg = [0u8; 64];
        while GetMessageW(msg.as_mut_ptr(), std::ptr::null_mut(), 0, 0) > 0 {}
    });
}
