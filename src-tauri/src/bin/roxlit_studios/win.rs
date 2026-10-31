//! Win32 pieces for session Studios: launch without taking focus, park every window of
//! the process on the Studio display, kill. Only SetWindowPos/SetWindowPlacement with
//! SWP_NOACTIVATE are used: never SetForegroundWindow, never synthetic mouse input, so
//! Juan's focus and cursor are never touched.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};

type Hwnd = *mut c_void;
type Handle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Default)]
struct WindowPlacement {
    length: u32,
    flags: u32,
    show_cmd: u32,
    pt_min: Point,
    pt_max: Point,
    rc_normal: Rect,
}

#[repr(C)]
struct MonitorInfoEx {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
    device: [u16; 32],
}

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
    fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn IsZoomed(hwnd: Hwnd) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn GetWindowPlacement(hwnd: Hwnd, wp: *mut WindowPlacement) -> i32;
    fn SetWindowPlacement(hwnd: Hwnd, wp: *const WindowPlacement) -> i32;
    fn EnumDisplayMonitors(
        hdc: *mut c_void,
        clip: *const Rect,
        cb: extern "system" fn(*mut c_void, *mut c_void, *mut Rect, isize) -> i32,
        lparam: isize,
    ) -> i32;
    fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfoEx) -> i32;
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

const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SW_SHOWNOACTIVATE: u32 = 4;
const SW_SHOWMINNOACTIVE: u16 = 7;
const STARTF_USESHOWWINDOW: u32 = 0x1;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

/// Bounds of the monitor whose device name is `device` (e.g. `\\.\DISPLAY5`).
pub fn monitor_rect(device: &str) -> Option<Rect> {
    extern "system" fn cb(m: *mut c_void, _: *mut c_void, _: *mut Rect, lp: isize) -> i32 {
        let out = unsafe { &mut *(lp as *mut Vec<(String, Rect)>) };
        let mut info = MonitorInfoEx {
            size: std::mem::size_of::<MonitorInfoEx>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
            device: [0; 32],
        };
        if unsafe { GetMonitorInfoW(m, &mut info) } != 0 {
            let n = info.device.iter().position(|&c| c == 0).unwrap_or(32);
            out.push((String::from_utf16_lossy(&info.device[..n]), info.monitor));
        }
        1
    }
    let mut all: Vec<(String, Rect)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            cb,
            &mut all as *mut _ as isize,
        )
    };
    all.into_iter()
        .find(|(d, _)| d.eq_ignore_ascii_case(device))
        .map(|(_, r)| r)
}

/// Start `exe place` minimized and inactive. Returns the pid.
pub fn launch_inactive(exe: &Path, place: &Path) -> std::io::Result<u32> {
    let mut cmd = wide(&format!("\"{}\" \"{}\"", exe.display(), place.display()));
    let mut si: StartupInfo = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<StartupInfo>() as u32;
    si.flags = STARTF_USESHOWWINDOW;
    si.show_window = SW_SHOWMINNOACTIVE;
    let mut pi: ProcessInformation = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null(),
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

/// Visible top-level windows of `pid` with their titles.
pub fn windows_of(pid: u32) -> Vec<(Hwnd, String)> {
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
    unsafe { EnumWindows(cb, &mut state as *mut _ as isize) };
    state.1
}

/// Move one window inside `area` without activating it. Returns true if it moved.
fn park(h: Hwnd, area: Rect) -> bool {
    let mut r = Rect::default();
    unsafe { GetWindowRect(h, &mut r) };
    if r.left >= area.left && r.left < area.right && r.top >= area.top && r.top < area.bottom {
        return false;
    }
    let (aw, ah) = (area.right - area.left, area.bottom - area.top);
    if unsafe { IsIconic(h) != 0 || IsZoomed(h) != 0 } {
        let mut wp = WindowPlacement {
            length: std::mem::size_of::<WindowPlacement>() as u32,
            ..Default::default()
        };
        unsafe { GetWindowPlacement(h, &mut wp) };
        wp.rc_normal = area;
        wp.show_cmd = SW_SHOWNOACTIVATE;
        unsafe { SetWindowPlacement(h, &wp) };
    } else {
        let (w, hh) = ((r.right - r.left).min(aw), (r.bottom - r.top).min(ah));
        unsafe {
            SetWindowPos(
                h,
                std::ptr::null_mut(),
                area.left + 20,
                area.top + 20,
                w,
                hh,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
    }
    true
}

/// Park every window of `pid` until a window title contains `marker` (the place is
/// loaded), then 20 s more because Studio relayouts after load. Gives up at `cap`.
/// Returns whether the marker window appeared.
pub fn park_until_loaded(pid: u32, area: Option<Rect>, marker: &str, cap: Duration) -> bool {
    let start = Instant::now();
    let mut loaded_at: Option<Instant> = None;
    while start.elapsed() < cap {
        let wins = windows_of(pid);
        if let Some(a) = area {
            for (h, _) in &wins {
                park(*h, a);
            }
        }
        if loaded_at.is_none() && wins.iter().any(|(_, t)| t.contains(marker)) {
            loaded_at = Some(Instant::now());
        }
        if loaded_at.is_some_and(|t| t.elapsed() > Duration::from_secs(20)) {
            return true;
        }
        if !pid_alive(pid) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    loaded_at.is_some()
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
