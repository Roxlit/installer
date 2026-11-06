//! Session Studios on Juan's desktop (viewer "View live" / "Take control"). A window can't
//! move between desktops, so a viewed session runs a second Studio on the default desktop,
//! parked on the Studio display (the Virtual Display Driver monitor, or
//! `ROXLIT_STUDIO_DISPLAY`) where DWM renders it and PrintWindow captures the 3D view. Taking control moves
//! that window to the primary monitor (instant, no reload); giving back parks it again.

use crate::win;
use std::ffi::c_void;
use std::time::{Duration, Instant};

type Hwnd = *mut c_void;

#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct WindowPlacement {
    length: u32,
    flags: u32,
    show_cmd: u32,
    min: [i32; 2],
    max: [i32; 2],
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
struct DisplayDevice {
    cb: u32,
    name: [u16; 32],
    string: [u16; 128],
    flags: u32,
    id: [u16; 128],
    key: [u16; 128],
}

#[link(name = "user32")]
extern "system" {
    fn EnumDisplayDevicesW(device: *const u16, i: u32, out: *mut DisplayDevice, flags: u32) -> i32;
    fn EnumDisplayMonitors(
        hdc: *mut c_void,
        clip: *const Rect,
        cb: extern "system" fn(*mut c_void, *mut c_void, *mut Rect, isize) -> i32,
        lp: isize,
    ) -> i32;
    fn GetMonitorInfoW(m: *mut c_void, info: *mut MonitorInfoEx) -> i32;
    fn GetWindowPlacement(h: Hwnd, wp: *mut WindowPlacement) -> i32;
    fn SetWindowPlacement(h: Hwnd, wp: *const WindowPlacement) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut [i32; 4]) -> i32;
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn AttachThreadInput(from: u32, to: u32, attach: i32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
}

const MONITORINFOF_PRIMARY: u32 = 1;
const SW_SHOWMAXIMIZED: u32 = 3;
const SW_SHOWNOACTIVATE: u32 = 4;
const RELAYOUT: Duration = Duration::from_secs(20);

struct Monitor {
    device: String,
    work: Rect,
    primary: bool,
}

fn monitors() -> Vec<Monitor> {
    extern "system" fn cb(m: *mut c_void, _: *mut c_void, _: *mut Rect, lp: isize) -> i32 {
        let out = unsafe { &mut *(lp as *mut Vec<Monitor>) };
        let mut info = MonitorInfoEx {
            size: std::mem::size_of::<MonitorInfoEx>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
            device: [0; 32],
        };
        if unsafe { GetMonitorInfoW(m, &mut info) } != 0 {
            let n = info.device.iter().position(|&c| c == 0).unwrap_or(32);
            out.push(Monitor {
                device: String::from_utf16_lossy(&info.device[..n]),
                work: info.work,
                primary: info.flags & MONITORINFOF_PRIMARY != 0,
            });
        }
        1
    }
    let mut all = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            cb,
            &mut all as *mut _ as isize,
        )
    };
    all
}

/// Adapter name ("Virtual Display Driver", "NVIDIA ...") behind a `\\.\DISPLAYn` device.
fn adapter_of(device: &str) -> String {
    let mut d = DisplayDevice {
        cb: std::mem::size_of::<DisplayDevice>() as u32,
        name: [0; 32],
        string: [0; 128],
        flags: 0,
        id: [0; 128],
        key: [0; 128],
    };
    let mut i = 0;
    while unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut d, 0) } != 0 {
        let n = d.name.iter().position(|&c| c == 0).unwrap_or(32);
        if String::from_utf16_lossy(&d.name[..n]).eq_ignore_ascii_case(device) {
            let s = d.string.iter().position(|&c| c == 0).unwrap_or(128);
            return String::from_utf16_lossy(&d.string[..s]);
        }
        i += 1;
    }
    String::new()
}

/// Work area of the Studio display; None when that monitor is missing (driver off).
/// `ROXLIT_STUDIO_DISPLAY` names the device; otherwise the monitor driven by the
/// "Virtual Display Driver" adapter. Its DISPLAYn number changes when the driver
/// reconnects (DISPLAY5 became DISPLAY6), and other virtual adapters exist (USB
/// Mobile Monitor), so neither a number nor "any secondary" is safe.
pub fn studio_area() -> Option<Rect> {
    let forced = std::env::var("ROXLIT_STUDIO_DISPLAY").ok();
    monitors()
        .into_iter()
        .filter(|m| !m.primary)
        .find(|m| match &forced {
            Some(dev) => m.device.eq_ignore_ascii_case(dev),
            None => adapter_of(&m.device).contains("Virtual Display Driver"),
        })
        .map(|m| m.work)
}

fn primary_area() -> Option<Rect> {
    monitors().into_iter().find(|m| m.primary).map(|m| m.work)
}

fn inside(h: Hwnd, area: Rect) -> bool {
    let mut r = [0i32; 4];
    unsafe { GetWindowRect(h, &mut r) };
    let (cx, cy) = ((r[0] + r[2]) / 2, (r[1] + r[3]) / 2);
    cx >= area.left && cx < area.right && cy >= area.top && cy < area.bottom
}

/// Fill `area` with window `h` without activating it. Returns true if it moved.
pub fn park(h: Hwnd, area: Rect) -> bool {
    if inside(h, area) {
        return false;
    }
    let mut wp = WindowPlacement {
        length: std::mem::size_of::<WindowPlacement>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(h, &mut wp) };
    wp.rc_normal = area;
    wp.show_cmd = SW_SHOWNOACTIVATE;
    unsafe { SetWindowPlacement(h, &wp) };
    true
}

/// Studio relayouts for a while after load, so a thread keeps parking its windows for
/// RELAYOUT more without holding up the caller.
pub fn keep_parked(pid: u32, area: Rect) {
    std::thread::spawn(move || {
        let t = Instant::now();
        while t.elapsed() < RELAYOUT {
            for (h, _) in win::windows_of(pid, true) {
                park(h, area);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}

/// "Take control": maximize the Studio on the primary monitor and bring it to the front.
/// Juan asked for it (viewer button), so taking the foreground is wanted here.
pub fn bring_front(pid: u32) -> Result<(), String> {
    let h = win::main_window_on(pid, true).ok_or("Studio has no window")?;
    let area = primary_area().ok_or("no primary monitor")?;
    let mut wp = WindowPlacement {
        length: std::mem::size_of::<WindowPlacement>() as u32,
        ..Default::default()
    };
    unsafe {
        GetWindowPlacement(h, &mut wp);
        wp.rc_normal = area;
        wp.show_cmd = SW_SHOWMAXIMIZED;
        SetWindowPlacement(h, &wp);
        // The browser holds the foreground; joining its input queue lets us pass it on.
        let fg_tid = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let me = GetCurrentThreadId();
        AttachThreadInput(me, fg_tid, 1);
        SetForegroundWindow(h);
        AttachThreadInput(me, fg_tid, 0);
    }
    Ok(())
}

/// "Give back": park the Studio on the Studio display again, without activating it.
pub fn send_back(pid: u32) -> Result<(), String> {
    let area = studio_area().ok_or("Studio display not found")?;
    for (h, _) in win::windows_of(pid, true) {
        park(h, area);
    }
    Ok(())
}
