//! Win32 pieces for session Studios: launch on a hidden desktop, wait for the place to
//! load, kill. Windows on a desktop that is not the input desktop are never shown on
//! Juan's screens, never take his focus and get no taskbar button (measured 2026-10-31:
//! the splash cannot be moved from outside because Studio's UI thread does not pump while
//! loading). Sessions Juan views live run on his desktop instead: see screen.rs.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub(crate) type Hwnd = *mut c_void;
type Handle = *mut c_void;

/// Desktop that holds every session Studio.
pub const DESKTOP: &str = "roxlit-studios";

#[repr(C)]
struct StartupInfo {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    cb_reserved2: u16,
    reserved2: *mut u8,
    std_input: Handle,
    std_output: Handle,
    std_error: Handle,
}

#[repr(C)]
struct ProcessInformation {
    process: Handle,
    thread: Handle,
    process_id: u32,
    thread_id: u32,
}

#[link(name = "user32")]
extern "system" {
    fn CreateDesktopW(
        name: *const u16,
        device: *const u16,
        devmode: *mut c_void,
        flags: u32,
        access: u32,
        sa: *mut c_void,
    ) -> Handle;
    fn EnumDesktopWindows(
        desktop: Handle,
        cb: extern "system" fn(Hwnd, isize) -> i32,
        lparam: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
    fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut [i32; 4]) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateProcessW(
        app: *const u16,
        cmd: *mut u16,
        pattr: *mut c_void,
        tattr: *mut c_void,
        inherit: i32,
        flags: u32,
        env: *mut c_void,
        cwd: *const u16,
        si: *mut StartupInfo,
        pi: *mut ProcessInformation,
    ) -> i32;
    fn CloseHandle(h: Handle) -> i32;
}

const GENERIC_ALL: u32 = 0x1000_0000;
const STARTF_USESHOWWINDOW: u32 = 0x1;
const SW_SHOWMINNOACTIVE: u16 = 7;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

/// The hidden desktop, created once and held open for the manager's lifetime (opening
/// it again after a manager restart returns the same desktop with its Studios).
pub fn desktop() -> std::io::Result<Handle> {
    static DESK: OnceLock<usize> = OnceLock::new();
    let h = *DESK.get_or_init(|| {
        let name = wide(DESKTOP);
        let h = unsafe {
            CreateDesktopW(
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
                GENERIC_ALL,
                std::ptr::null_mut(),
            )
        };
        h as usize
    });
    if h == 0 {
        Err(std::io::Error::other("CreateDesktopW failed"))
    } else {
        Ok(h as Handle)
    }
}

/// Start `exe place` on the hidden desktop, or on Juan's when `visible` (the viewer's
/// "Take control"). Returns the pid.
pub fn launch(exe: &Path, place: &Path, visible: bool) -> std::io::Result<u32> {
    desktop()?;
    let mut desk = wide(&format!("WinSta0\\{DESKTOP}"));
    let mut cmd = wide(&format!("\"{}\" \"{}\"", exe.display(), place.display()));
    let app = wide(&exe.to_string_lossy());
    let cwd = wide(&exe.parent().unwrap_or(exe).to_string_lossy());
    let mut si: StartupInfo = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<StartupInfo>() as u32;
    if visible {
        // Minimized and inactive; screen.rs parks it on the Studio display.
        si.flags = STARTF_USESHOWWINDOW;
        si.show_window = SW_SHOWMINNOACTIVE;
    } else {
        si.desktop = desk.as_mut_ptr();
    }
    let mut pi: ProcessInformation = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            app.as_ptr(),
            cmd.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            std::ptr::null_mut(),
            cwd.as_ptr(),
            &mut si,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    unsafe {
        CloseHandle(pi.thread);
        CloseHandle(pi.process);
    }
    Ok(pi.process_id)
}

/// Visible windows of `pid`, with titles: on the hidden desktop, or on the manager's own
/// (Juan's) when `visible`.
pub(crate) fn windows_of(pid: u32, visible: bool) -> Vec<(Hwnd, String)> {
    extern "system" fn cb(h: Hwnd, lp: isize) -> i32 {
        let (pid, out) = unsafe { &mut *(lp as *mut (u32, Vec<(Hwnd, String)>)) };
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut p) };
        if p == *pid && unsafe { IsWindowVisible(h) } != 0 {
            let mut buf = [0u16; 512];
            let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
            out.push((h, String::from_utf16_lossy(&buf[..n.max(0) as usize])));
        }
        1
    }
    let mut state = (pid, Vec::new());
    let lp = &mut state as *mut _ as isize;
    if visible {
        unsafe { EnumWindows(cb, lp) };
    } else if let Ok(desk) = desktop() {
        unsafe { EnumDesktopWindows(desk, cb, lp) };
    }
    state.1
}

/// Largest visible window of `pid` (Studio's main window), on the hidden desktop or on
/// Juan's when `visible`.
pub fn main_window_on(pid: u32, visible: bool) -> Option<Hwnd> {
    windows_of(pid, visible)
        .into_iter()
        .map(|(h, _)| {
            let mut r = [0i32; 4];
            unsafe { GetWindowRect(h, &mut r) };
            (h, (r[2] - r[0]) as i64 * (r[3] - r[1]) as i64)
        })
        .max_by_key(|(_, area)| *area)
        .map(|(h, _)| h)
}

/// Wait until a window title of `pid` contains `marker` (the place is loaded). Gives up
/// at `cap` or when the process dies.
pub fn wait_loaded(pid: u32, marker: &str, cap: Duration, visible: bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < cap {
        if windows_of(pid, visible)
            .iter()
            .any(|(_, t)| t.contains(marker))
        {
            return true;
        }
        if !pid_alive(pid) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

pub fn pid_alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .creation_flags_no_window()
        .output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
        .unwrap_or(false)
}

pub fn kill_tree(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags_no_window()
        .output();
}

pub trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000)
    }
}
