//! Viewer for session Studios: a thumbnail of each Studio's main window, captured with
//! `PrintWindow(PW_RENDERFULLCONTENT)` from a thread attached to the hidden desktop (no
//! focus, nothing shown on Juan's screens), served as a BMP, plus the viewer page.
//! Measured 2026-10-31: the capture holds the frame, menus and top-level dialogs, but the
//! 3D viewport and docked panels come out blank (no DWM on a non-input desktop).

use crate::session::{self, Manager, Session};
use crate::win;
use std::ffi::c_void;

type Hwnd = *mut c_void;
type Hdc = *mut c_void;
type Hbitmap = *mut c_void;

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    clr_used: u32,
    clr_important: u32,
}

#[link(name = "user32")]
extern "system" {
    fn GetWindowRect(hwnd: Hwnd, rect: *mut [i32; 4]) -> i32;
    fn PrintWindow(hwnd: Hwnd, hdc: Hdc, flags: u32) -> i32;
    fn GetDC(hwnd: Hwnd) -> Hdc;
    fn ReleaseDC(hwnd: Hwnd, hdc: Hdc) -> i32;
    fn SetThreadDesktop(desk: *mut c_void) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn CreateCompatibleBitmap(hdc: Hdc, w: i32, h: i32) -> Hbitmap;
    fn SelectObject(hdc: Hdc, obj: *mut c_void) -> *mut c_void;
    fn DeleteObject(obj: *mut c_void) -> i32;
    fn DeleteDC(hdc: Hdc) -> i32;
    fn SetStretchBltMode(hdc: Hdc, mode: i32) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn StretchBlt(
        dst: Hdc,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: Hdc,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        rop: u32,
    ) -> i32;
    fn GetDIBits(
        hdc: Hdc,
        bmp: Hbitmap,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BitmapInfoHeader,
        usage: u32,
    ) -> i32;
}

const PW_RENDERFULLCONTENT: u32 = 2;
const HALFTONE: i32 = 4;
const SRCCOPY: u32 = 0x00CC_0020;
const MAX_W: i32 = 960;

/// BMP of the main window of `pid`, at most MAX_W wide. None when it has no window.
pub fn shot(pid: u32) -> Option<Vec<u8>> {
    // SetThreadDesktop needs a thread without windows, so capture on a fresh one.
    std::thread::spawn(move || unsafe {
        SetThreadDesktop(win::desktop().ok()?);
        let hwnd = win::main_window(pid)?;
        let mut r = [0i32; 4];
        GetWindowRect(hwnd, &mut r);
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        if w <= 0 || h <= 0 {
            return None;
        }
        let (tw, th) = if w > MAX_W {
            (MAX_W, h * MAX_W / w)
        } else {
            (w, h)
        };
        let screen = GetDC(std::ptr::null_mut());
        let full_dc = CreateCompatibleDC(screen);
        let full = CreateCompatibleBitmap(screen, w, h);
        let thumb_dc = CreateCompatibleDC(screen);
        let thumb = CreateCompatibleBitmap(screen, tw, th);
        let old_full = SelectObject(full_dc, full);
        let old_thumb = SelectObject(thumb_dc, thumb);
        let ok = PrintWindow(hwnd, full_dc, PW_RENDERFULLCONTENT) != 0;
        SetStretchBltMode(thumb_dc, HALFTONE);
        StretchBlt(thumb_dc, 0, 0, tw, th, full_dc, 0, 0, w, h, SRCCOPY);
        SelectObject(thumb_dc, old_thumb);
        let mut info = BitmapInfoHeader {
            size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            width: tw,
            height: -th,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_ppm: 0,
            y_ppm: 0,
            clr_used: 0,
            clr_important: 0,
        };
        let mut px = vec![0u8; (tw * th * 4) as usize];
        let lines = GetDIBits(
            thumb_dc,
            thumb,
            0,
            th as u32,
            px.as_mut_ptr() as _,
            &mut info,
            0,
        );
        SelectObject(full_dc, old_full);
        DeleteObject(full);
        DeleteObject(thumb);
        DeleteDC(full_dc);
        DeleteDC(thumb_dc);
        ReleaseDC(std::ptr::null_mut(), screen);
        (ok && lines > 0).then(|| bmp(tw, th, &px))
    })
    .join()
    .ok()
    .flatten()
}

fn bmp(w: i32, h: i32, px: &[u8]) -> Vec<u8> {
    let header = 14 + 40;
    let mut out = Vec::with_capacity(header + px.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((header + px.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(header as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&(-h).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    out.extend_from_slice(px);
    out
}

impl Manager {
    /// Viewer's "Take control" (`visible`) / "Give back": reopen the session's Studio on
    /// Juan's desktop or back on the hidden one. Rojo keeps serving, so the reopened Studio
    /// resyncs from the worktree; what was only in the old Studio (unsaved `execute_luau`
    /// instances, play state) is gone. `opened` moves forward so the routing hook relists
    /// the new MCP id.
    pub fn set_visible(&self, token: &str, visible: bool) -> Result<Session, String> {
        let (pid, name) = {
            let mut s = self.sessions.lock().unwrap();
            let sess = s
                .get_mut(token)
                .ok_or_else(|| format!("no session {token}"))?;
            if sess.state != "ready" {
                return Err(format!("session {token} is {}", sess.state));
            }
            if sess.visible == visible {
                return Ok(sess.clone());
            }
            sess.state = "starting".into();
            let out = (sess.studio_pid, sess.place.clone());
            self.save(&s);
            out
        };
        win::kill_tree(pid);
        let start = std::time::Instant::now();
        while win::pid_alive(pid) && start.elapsed().as_secs() < 10 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let result = session::launch_studio(&self.place_path(&name), &name, visible);
        let mut s = self.sessions.lock().unwrap();
        let sess = s.get_mut(token).ok_or("session vanished")?;
        match result {
            Ok(pid) => {
                sess.studio_pid = pid;
                sess.visible = visible;
                sess.opened = session::now();
                sess.state = "ready".into();
            }
            Err(e) => {
                win::kill_tree(sess.rojo_pid);
                sess.state = "failed".into();
                sess.error = Some(e);
            }
        }
        let out = sess.clone();
        self.save(&s);
        Ok(out)
    }
}

/// The viewer page: one card per session, thumbnail refreshed every 2 s.
pub const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Roxlit Studios</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
:root{--bg:#101211;--card:#1a1d1b;--fg:#e8ece9;--dim:#8b958f;--accent:#34d399}
body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font-family:"Bricolage Grotesque",ui-monospace,monospace}
h1{font-size:18px;margin:0 0 12px}
#list{display:flex;flex-direction:column;gap:16px}
.card{background:var(--card);padding:12px;border-left:3px solid var(--accent)}
.card img{max-width:100%;display:block;margin:8px 0;background:#000}
.meta{color:var(--dim);font-size:13px;word-break:break-all}
button{background:none;color:var(--fg);border:1px solid var(--dim);padding:6px 10px;margin-right:8px;cursor:pointer;font:inherit}
button:hover{border-color:var(--accent)}
</style></head><body>
<h1>Session Studios</h1>
<p class="meta">Thumbnails show the window frame and dialogs only (a stuck prompt shows up here); the
3D view and panels do not render off-screen. Take control to see or use one.</p>
<div id="list">loading...</div>
<script>
async function act(path, token){
  await fetch(path,{method:"POST",body:JSON.stringify({token})});
  load();
}
async function load(){
  const s = await (await fetch("/sessions")).json();
  const list = document.getElementById("list");
  if(!s.length){ list.textContent = "No session Studios open."; return; }
  list.innerHTML = s.map(x => `<div class="card">
    <b>${x.place}</b> <span class="meta">${x.state}${x.visible ? " - on your desktop" : ""}</span>
    <div class="meta">${x.worktree} - Rojo ${x.rojoPort}</div>
    ${x.visible ? "" : `<img src="/shot/${x.token}?t=${Date.now()}" alt="">`}
    <button onclick="act('/show','${x.token}')">Take control</button>
    <button onclick="act('/hide','${x.token}')">Give back</button>
    <button onclick="act('/close','${x.token}')">Close</button></div>`).join("");
}
load(); setInterval(() => document.querySelectorAll(".card img").forEach(i => {
  i.src = i.src.replace(/t=\d+/, "t=" + Date.now());
}), 2000); setInterval(load, 10000);
</script></body></html>"#;
