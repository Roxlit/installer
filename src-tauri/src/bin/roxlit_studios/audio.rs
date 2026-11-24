//! Session Studios are silent unless Juan took control of one: a parked or hidden Studio
//! playing its music is noise from a window he isn't looking at. This mutes the Windows
//! audio session of the process (volume mixer), not the game, so Sound.IsPlaying and
//! PlaybackLoudness still work for tests. ROXLIT_STUDIO_SOUND=1 turns it off.
//!
//! Windows saves a session's mute per executable when the process ends (measured
//! 2026-11-04: mute pid A, A exits, the next process of the same exe starts muted;
//! a process running at the same time is not affected). So a Studio is unmuted before
//! it is closed, and if a muted one dies anyway (crash, update), the next Studio that is
//! not a session's gets unmuted once.

use crate::session::Manager;
use crate::win;
use std::collections::HashSet;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

type P = *mut c_void;

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

const CLSID_ENUMERATOR: Guid = Guid(
    0xBCDE0395,
    0xE52F,
    0x467C,
    [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
);
const IID_ENUMERATOR: Guid = Guid(
    0xA95664D2,
    0x9614,
    0x4F35,
    [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
);
const IID_MANAGER2: Guid = Guid(
    0x77AA99A0,
    0x1BD6,
    0x484F,
    [0x8B, 0xC7, 0x2C, 0x65, 0x4C, 0x9A, 0x9B, 0x6F],
);
const IID_CONTROL2: Guid = Guid(
    0xBFB7FF88,
    0x7239,
    0x4FC9,
    [0x8F, 0xA2, 0x07, 0xC9, 0x50, 0xBE, 0x9C, 0x6D],
);
const IID_VOLUME: Guid = Guid(
    0x87CE5498,
    0x68D6,
    0x44E5,
    [0x92, 0x15, 0x6D, 0xA4, 0x7E, 0xF8, 0x83, 0xD8],
);

const CLSCTX_ALL: u32 = 0x17;
const RENDER: u32 = 0;
const ACTIVE: u32 = 1;
const STUDIO: &str = "RobloxStudioBeta.exe";
const TICK: Duration = Duration::from_secs(1);

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: P, flags: u32) -> i32;
    fn CoTaskMemFree(p: P);
    fn CoCreateInstance(
        clsid: *const Guid,
        outer: P,
        ctx: u32,
        iid: *const Guid,
        out: *mut P,
    ) -> i32;
}

/// Method `i` of a COM object's vtable.
unsafe fn slot(obj: P, i: usize) -> *const c_void {
    *(*(obj as *const *const *const c_void)).add(i)
}

unsafe fn release(obj: P) {
    let f: extern "system" fn(P) -> u32 = std::mem::transmute(slot(obj, 2));
    f(obj);
}

unsafe fn query(obj: P, iid: &Guid) -> Option<P> {
    let f: extern "system" fn(P, *const Guid, *mut P) -> i32 = std::mem::transmute(slot(obj, 0));
    let mut out = std::ptr::null_mut();
    (f(obj, iid, &mut out) >= 0 && !out.is_null()).then_some(out)
}

/// Out-pointer call: `method(obj, ..., &mut out)` returning an object or count.
macro_rules! out {
    ($obj:expr, $i:expr, $t:ty $(, $arg:expr => $at:ty)*) => {{
        let f: extern "system" fn(P $(, $at)*, *mut $t) -> i32 = std::mem::transmute(slot($obj, $i));
        let mut v: $t = std::mem::zeroed();
        (f($obj $(, $arg)*, &mut v) >= 0).then_some(v)
    }};
}

const IID_UNKNOWN: Guid = Guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_NOTIFY: Guid = Guid(
    0x641DD20B,
    0x4D41,
    0x49CC,
    [0xAB, 0xA3, 0x17, 0x4B, 0x94, 0x77, 0xBB, 0x08],
);

/// Set when any registered session manager reports a new session.
static NEW_SESSION: AtomicBool = AtomicBool::new(false);

/// IAudioSessionNotification that raises NEW_SESSION. Static, never freed, so
/// AddRef/Release are no-ops.
#[repr(C)]
struct Notify {
    vtbl: &'static NotifyVtbl,
}

// Only its address is shared; the vtable is immutable.
unsafe impl Sync for Notify {}

extern "system" fn n_query(this: P, iid: *const Guid, out: *mut P) -> i32 {
    const E_NOINTERFACE: i32 = 0x80004002u32 as i32;
    let eq = |a: &Guid, b: &Guid| a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && a.3 == b.3;
    unsafe {
        if eq(&*iid, &IID_UNKNOWN) || eq(&*iid, &IID_NOTIFY) {
            *out = this;
            0
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}
extern "system" fn n_ref(_: P) -> u32 {
    1
}
extern "system" fn n_created(_: P, _: P) -> i32 {
    NEW_SESSION.store(true, Ordering::SeqCst);
    0
}

#[repr(C)]
struct NotifyVtbl {
    query: extern "system" fn(P, *const Guid, *mut P) -> i32,
    add_ref: extern "system" fn(P) -> u32,
    release: extern "system" fn(P) -> u32,
    created: extern "system" fn(P, P) -> i32,
}

static NOTIFY_VTBL: NotifyVtbl = NotifyVtbl {
    query: n_query,
    add_ref: n_ref,
    release: n_ref,
    created: n_created,
};
static NOTIFY: Notify = Notify { vtbl: &NOTIFY_VTBL };

/// (Un)register NOTIFY on a session manager: RegisterSessionNotification is slot 6,
/// UnregisterSessionNotification slot 7.
unsafe fn notify(mgr: P, on: bool) {
    let f: extern "system" fn(P, P) -> i32 = std::mem::transmute(slot(mgr, if on { 6 } else { 7 }));
    f(mgr, &NOTIFY as *const Notify as P);
}

/// Ids of the active output devices, in enumeration order.
unsafe fn device_ids(en: P) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(devs) = out!(en, 3, P, RENDER => u32, ACTIVE => u32) {
        for d in 0..out!(devs, 3, u32).unwrap_or(0) {
            let Some(dev) = out!(devs, 4, P, d => u32) else {
                continue;
            };
            if let Some(id) = out!(dev, 5, *mut u16) {
                if !id.is_null() {
                    let n = (0..).take_while(|&i| *id.add(i) != 0).count();
                    ids.push(String::from_utf16_lossy(std::slice::from_raw_parts(id, n)));
                    CoTaskMemFree(id as P);
                }
            }
            release(dev);
        }
        release(devs);
    }
    ids
}

/// The audio loop's view of the mixer, kept across ticks.
///
/// IAudioSessionManager2::GetSessionEnumerator leaks inside Windows' audio client on
/// every call: ~20 KB with a freshly activated manager, ~3 KB with a kept one, with or
/// without a registered notification (bench_audio_tick, 2026-11-24). Calling it every
/// second grew roxlit-studios by ~1.2 MB/min, ~1.8 GB in a day. So the managers are kept,
/// sessions are enumerated only when a manager reports a new one or the devices change,
/// and in between the kept ISimpleAudioVolume objects are reused.
struct Mixer {
    en: P,
    ids: Vec<String>,
    mgrs: Vec<P>,
    vols: Vec<Volume>,
}

impl Mixer {
    fn new() -> Option<Self> {
        let en = unsafe { enumerator()? };
        Some(Mixer {
            en,
            ids: Vec::new(),
            mgrs: Vec::new(),
            vols: Vec::new(),
        })
    }

    fn drop_managers(&mut self) {
        self.vols.clear();
        for m in self.mgrs.drain(..) {
            unsafe {
                notify(m, false);
                release(m);
            }
        }
    }

    /// Sessions of processes in `alive`, re-enumerated only when something changed.
    fn sessions(&mut self, alive: &HashSet<u32>) -> &[Volume] {
        unsafe {
            let ids = device_ids(self.en);
            if ids != self.ids {
                self.drop_managers();
                // Registered before the first enumeration, so no session is missed.
                self.mgrs = managers(self.en);
                for &m in &self.mgrs {
                    notify(m, true);
                }
                self.ids = ids;
                NEW_SESSION.store(true, Ordering::SeqCst);
            }
            if NEW_SESSION.swap(false, Ordering::SeqCst) {
                self.vols.clear();
                for &m in &self.mgrs {
                    sessions_of(m, &mut self.vols);
                }
            }
        }
        self.vols.retain(|v| alive.contains(&v.pid));
        &self.vols
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        self.drop_managers();
        unsafe { release(self.en) };
    }
}

/// One audio session of a process: its pid and its ISimpleAudioVolume (released on drop).
struct Volume {
    pid: u32,
    vol: P,
}

impl Volume {
    fn muted(&self) -> bool {
        unsafe { out!(self.vol, 6, i32) }.unwrap_or(0) != 0
    }

    fn set(&self, mute: bool) {
        let f: extern "system" fn(P, i32, *const Guid) -> i32 =
            unsafe { std::mem::transmute(slot(self.vol, 5)) };
        f(self.vol, mute as i32, std::ptr::null());
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        unsafe { release(self.vol) };
    }
}

unsafe fn enumerator() -> Option<P> {
    let mut en: P = std::ptr::null_mut();
    (CoCreateInstance(
        &CLSID_ENUMERATOR,
        std::ptr::null_mut(),
        CLSCTX_ALL,
        &IID_ENUMERATOR,
        &mut en,
    ) >= 0
        && !en.is_null())
    .then_some(en)
}

/// Session manager of every active output device.
unsafe fn managers(en: P) -> Vec<P> {
    let mut out = Vec::new();
    if let Some(devs) = out!(en, 3, P, RENDER => u32, ACTIVE => u32) {
        for d in 0..out!(devs, 3, u32).unwrap_or(0) {
            let Some(dev) = out!(devs, 4, P, d => u32) else {
                continue;
            };
            let iid: *const Guid = &IID_MANAGER2;
            if let Some(mgr) =
                out!(dev, 3, P, iid => *const Guid, CLSCTX_ALL => u32, std::ptr::null_mut() => P)
            {
                out.push(mgr);
            }
            release(dev);
        }
        release(devs);
    }
    out
}

/// Every audio session of one device's session manager, appended to `all`.
unsafe fn sessions_of(mgr: P, all: &mut Vec<Volume>) {
    let Some(list) = out!(mgr, 5, P) else {
        return;
    };
    for i in 0..out!(list, 3, i32).unwrap_or(0) {
        let Some(ctl) = out!(list, 4, P, i => i32) else {
            continue;
        };
        if let Some(ctl2) = query(ctl, &IID_CONTROL2) {
            let pid = out!(ctl2, 14, u32).unwrap_or(0);
            if let (true, Some(vol)) = (pid != 0, query(ctl, &IID_VOLUME)) {
                all.push(Volume { pid, vol });
            }
            release(ctl2);
        }
        release(ctl);
    }
    release(list);
}

/// Every audio session on every active output device.
fn sessions() -> Vec<Volume> {
    let mut all = Vec::new();
    unsafe {
        let Some(en) = enumerator() else {
            return all;
        };
        for mgr in managers(en) {
            sessions_of(mgr, &mut all);
            release(mgr);
        }
        release(en);
    }
    all
}

fn dirty_flag() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".roxlit")
        .join("sessions")
        .join("audio-muted-exit")
}

/// Unmute `pid` now (before a session Studio is killed, so Windows saves it unmuted).
pub fn unmute(pid: u32) {
    for v in sessions().into_iter().filter(|v| v.pid == pid && v.muted()) {
        v.set(false);
    }
}

/// Mute session Studios that aren't in front, unmute the ones Juan took control of.
pub fn start(mgr: Arc<Manager>) {
    if std::env::var("ROXLIT_STUDIO_SOUND").as_deref() == Ok("1") {
        return;
    }
    std::thread::spawn(move || {
        unsafe { CoInitializeEx(std::ptr::null_mut(), 0) };
        let mut muted: HashSet<u32> = HashSet::new();
        let mut others: HashSet<u32> = HashSet::new();
        let Some(mut mixer) = Mixer::new() else {
            return;
        };
        loop {
            let (quiet, front): (HashSet<u32>, HashSet<u32>) = {
                let s = mgr.sessions.lock().unwrap();
                let ready = s
                    .values()
                    .filter(|s| s.state == "ready" && s.studio_pid != 0);
                let (f, q): (Vec<_>, Vec<_>) = ready.partition(|s| s.front);
                (
                    q.iter().map(|s| s.studio_pid).collect(),
                    f.iter().map(|s| s.studio_pid).collect(),
                )
            };
            // A Studio we muted that is gone left its mute saved for the next one.
            let gone: Vec<u32> = muted
                .iter()
                .copied()
                .filter(|p| win::started_at(*p).is_none())
                .collect();
            if !gone.is_empty() {
                let _ = std::fs::write(dirty_flag(), b"");
                muted.retain(|p| !gone.contains(p));
            }
            let studios: HashSet<u32> = win::processes()
                .into_iter()
                .filter(|p| p.2.eq_ignore_ascii_case(STUDIO))
                .map(|p| p.0)
                .collect();
            others.retain(|p| studios.contains(p));
            for v in mixer.sessions(&studios) {
                if quiet.contains(&v.pid) {
                    if !v.muted() {
                        v.set(true);
                    }
                    muted.insert(v.pid);
                } else if front.contains(&v.pid) {
                    if muted.remove(&v.pid) && v.muted() {
                        v.set(false);
                    }
                } else if others.insert(v.pid) && dirty_flag().exists() {
                    // First sight of a Studio that isn't a session's (Juan's own).
                    if v.muted() {
                        v.set(false);
                    }
                    let _ = std::fs::remove_file(dirty_flag());
                }
            }
            std::thread::sleep(TICK);
        }
    });
}


#[cfg(test)]
mod tests {
    use super::*;

    fn all_pids() -> HashSet<u32> {
        win::processes().into_iter().map(|p| p.0).collect()
    }

    /// The audio loop's per-tick mixer work, 3000 times (50 min of the live 1 s loop):
    /// the old per-tick enumeration grows, the kept Mixer must stay flat.
    /// `cargo test --release --bin roxlit-studios bench_ -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_audio_tick() {
        unsafe { CoInitializeEx(std::ptr::null_mut(), 0) };
        let alive = all_pids();
        println!("audio sessions: {}", sessions().len());
        win::leak_bench("audio OLD sessions() per tick", 3000, || {
            for s in &sessions() {
                let _ = s.muted();
            }
        });
        let mut mixer = Mixer::new().unwrap();
        println!("mixer sessions: {}", mixer.sessions(&alive).len());
        let grew = win::leak_bench("audio NEW Mixer::sessions()", 3000, || {
            for s in mixer.sessions(&alive) {
                let _ = s.muted();
            }
        });
        assert!(grew < 2.0, "Mixer::sessions() grew {grew:.2} MB");
    }

    /// A process that starts playing sound after the Mixer enumerated is still found.
    #[test]
    #[ignore]
    fn mixer_sees_new_session() {
        unsafe { CoInitializeEx(std::ptr::null_mut(), 0) };
        let mut mixer = Mixer::new().unwrap();
        mixer.sessions(&all_pids());
        let mut child = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(New-Object Media.SoundPlayer 'C:\\Windows\\Media\\Alarm01.wav').PlaySync()",
            ])
            .spawn()
            .unwrap();
        let pid = child.id();
        let t = std::time::Instant::now();
        let mut seen = false;
        while t.elapsed() < Duration::from_secs(8) && !seen {
            std::thread::sleep(Duration::from_millis(200));
            seen = mixer.sessions(&all_pids()).iter().any(|v| v.pid == pid);
        }
        let _ = child.kill();
        println!(
            "new session of pid {pid} seen: {seen} after {:?}",
            t.elapsed()
        );
        assert!(seen);
    }
}
