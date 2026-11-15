//! Viewer for session Studios: live captures of each Studio's main window with
//! `PrintWindow(PW_RENDERFULLCONTENT)` (no focus taken), served as PNG, plus the viewer
//! page. Sessions run parked on the Studio display (screen.rs), where DWM composes them
//! and the capture is the full window, 3D included. Measured 2026-10-31: on the hidden
//! desktop (fallback) only the frame, menus and dialogs come out (no DWM there).

use crate::session::{Manager, Session};
use crate::{guard, screen, win};
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
const MAX_W: i32 = 2560;

/// PNG of the main window of `pid`, at most `max_w` wide. None when it has no window.
pub fn shot(pid: u32, visible: bool, max_w: i32) -> Option<Vec<u8>> {
    // SetThreadDesktop needs a thread without windows, so capture on a fresh one.
    std::thread::spawn(move || unsafe {
        if !visible {
            SetThreadDesktop(win::desktop().ok()?);
        }
        let hwnd = win::main_window_on(pid, visible)?;
        let mut r = [0i32; 4];
        GetWindowRect(hwnd, &mut r);
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        if w <= 0 || h <= 0 {
            return None;
        }
        let max_w = max_w.clamp(160, MAX_W);
        let (tw, th) = if w > max_w {
            (max_w, h * max_w / w)
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
        if !ok || lines <= 0 {
            return None;
        }
        encode_png(tw as u32, th as u32, &px)
    })
    .join()
    .ok()
    .flatten()
}

/// GDI's BGRA to an RGB PNG with fast compression, so frames are cheap enough to poll.
fn encode_png(w: u32, h: u32, bgra: &[u8]) -> Option<Vec<u8>> {
    let rgb: Vec<u8> = bgra
        .chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0]])
        .collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().ok()?.write_image_data(&rgb).ok()?;
    Some(out)
}

impl Manager {
    /// "Take control" (`front`): move the session's Studio from the Studio display to the
    /// primary monitor and focus it. "Give back": park it on the Studio display again.
    /// Instant, nothing reloads. Sessions on the hidden desktop (Studio display missing
    /// when they opened) can't move: windows don't change desktops.
    pub fn set_front(&self, token: &str, front: bool) -> Result<Session, String> {
        let mut s = self.sessions.lock().unwrap();
        let sess = s
            .get_mut(token)
            .ok_or_else(|| format!("no session {token}"))?;
        if sess.state != "ready" {
            return Err(format!("session {token} is {}", sess.state));
        }
        if !sess.visible {
            return Err("this Studio runs on the hidden desktop (no Studio display when it opened); close and reopen it".into());
        }
        if front {
            guard::unwatch(sess.studio_pid);
            screen::bring_front(sess.studio_pid)?;
        } else {
            screen::send_back(sess.studio_pid)?;
            guard::watch(sess.studio_pid);
        }
        sess.front = front;
        let out = sess.clone();
        self.save(&s);
        Ok(out)
    }
}

/// The viewer page: one card per session with a live capture; click it for full view.
pub const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Roxlit Studios</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
:root{--bg:#101211;--card:#1a1d1b;--fg:#e8ece9;--dim:#8b958f;--accent:#34d399}
body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font-family:"Bricolage Grotesque",ui-monospace,monospace}
h1{font-size:18px;margin:0 0 12px}
#list{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,520px),1fr));gap:16px}
.card{background:var(--card);padding:12px;border-left:3px solid var(--accent)}
.card img{width:100%;display:block;margin:8px 0;background:#000;cursor:zoom-in}
.meta{color:var(--dim);font-size:13px;word-break:break-all}
button{background:none;color:var(--fg);border:1px solid var(--dim);padding:6px 10px;margin:0 8px 0 0;cursor:pointer;font:inherit}
button:hover{border-color:var(--accent)}
#projects{display:flex;flex-direction:column;gap:6px;margin:0 0 20px}
.proj{display:flex;flex-wrap:wrap;align-items:center;gap:4px 12px;background:var(--card);padding:8px 12px}
.proj b{min-width:140px}
#projects h2{font-size:15px;color:var(--dim);margin:10px 0 0}
.proj .meta{flex:1;min-width:200px}
.proj .err{color:#f87171;width:100%;font-size:13px}
button:disabled{opacity:.5;cursor:default}
#full{position:fixed;inset:0;background:#000;display:none;z-index:9}
#full img{width:100%;height:100%;object-fit:contain;cursor:zoom-out}
#full .bar{position:absolute;top:8px;right:8px}
#full .bar button{background:#000a}
</style></head><body>
<h1>Projects</h1>
<div id="projects">loading...</div>
<h1>Session Studios</h1>
<div id="list">loading...</div>
<div id="full"><img alt=""><div class="bar">
<button id="fctl">Take control</button><button onclick="unzoom()">Close view (Esc)</button></div></div>
<script>
let sessions = [], zoomed = null;
const full = document.getElementById("full"), fimg = full.querySelector("img");
async function act(path, token){
  const r = await fetch(path,{method:"POST",body:JSON.stringify({token})});
  if(!r.ok) alert((await r.json()).error || r.status);
  load();
}
// Each image asks for its next frame as soon as the last one arrived (or failed).
function live(img, token, width){
  const next = () => { if(img.isConnected || img === fimg) img.src = `/shot/${token}?w=${width()}&t=${Date.now()}`; };
  img.onload = () => setTimeout(next, 50);
  img.onerror = () => setTimeout(next, 1000);
  next();
}
function zoom(token){
  zoomed = token; full.style.display = "block";
  document.getElementById("fctl").onclick = () => { unzoom(); act("/show", token); };
  live(fimg, token, () => Math.round(innerWidth * devicePixelRatio));
}
function unzoom(){ zoomed = null; full.style.display = "none"; fimg.onload = fimg.onerror = null; fimg.removeAttribute("src"); }
addEventListener("keydown", e => { if(e.key === "Escape") unzoom(); });
fimg.onclick = unzoom;
async function load(){
  sessions = await (await fetch("/sessions")).json();
  if(projects.length) drawProjects();
  const list = document.getElementById("list");
  const keep = new Set(sessions.map(x => x.token + x.state + x.visible + x.front));
  if(list.dataset.keys === [...keep].join()) return;
  list.dataset.keys = [...keep].join();
  if(!sessions.length){ list.textContent = "No session Studios open."; return; }
  list.innerHTML = sessions.map(x => `<div class="card">
    <b>${x.place}</b> <span class="meta">${x.state}${x.front ? " - on your screen" : x.visible ? "" : " - hidden desktop, frame only"}</span>
    <div class="meta">${x.worktree} - Rojo ${x.rojoPort}</div>
    ${x.state === "ready" ? `<img data-token="${x.token}" alt="">` : ""}
    ${x.front ? `<button onclick="act('/hide','${x.token}')">Give back</button>`
              : x.visible ? `<button onclick="act('/show','${x.token}')">Take control</button>` : ""}
    <button onclick="act('/close','${x.token}')">Close</button></div>`).join("");
  list.querySelectorAll("img").forEach(i => {
    i.onclick = () => zoom(i.dataset.token);
    live(i, i.dataset.token, () => Math.round(i.clientWidth * devicePixelRatio));
  });
}
// One Studio per project on its main checkout, for Juan's own playtests (key "juan",
// apart from the chats' sessions). Open = build + load, then straight to his screen.
let projects = [], busy = {}, errs = {};
async function play(i){
  const p = projects[i]; busy[p.path] = true; delete errs[p.path]; drawProjects();
  try {
    let s = sessions.find(x => x.token === p.token && x.state === "ready");
    if(!s){
      const r = await fetch("/open",{method:"POST",body:JSON.stringify({worktree:p.path,key:"juan"})});
      s = await r.json();
      if(!r.ok || s.state !== "ready") throw new Error(s.error || `open failed (${r.status})`);
    }
    if(!s.front){
      const r = await fetch("/show",{method:"POST",body:JSON.stringify({token:s.token})});
      if(!r.ok) throw new Error((await r.json()).error || r.status);
    }
  } catch(e){ errs[p.path] = String(e.message || e); }
  delete busy[p.path]; load();
}
function drawProjects(){
  const el = document.getElementById("projects");
  if(!projects.length){ el.textContent = "No Rojo projects in ~/RobloxProjects."; return; }
  const row = (p, i) => {
    const s = sessions.find(x => x.token === p.token);
    const label = busy[p.path] ? "Opening... (build + load, up to a minute)"
      : s && s.front ? "On your screen"
      : s && s.state === "ready" ? "Take control" : "Open & play";
    const off = !p.ok || busy[p.path] || (s && s.front);
    return `<div class="proj"><b>${p.name}</b>
      <span class="meta">${p.branch ? p.branch + " - " : ""}${p.path}${p.ok ? "" : " - no default.project.json"}</span>
      <button ${off ? "disabled" : ""} onclick="play(${i})">${label}</button>
      ${errs[p.path] ? `<div class="err">${errs[p.path]}</div>` : ""}</div>`;
  };
  const rows = g => projects.map(row).filter((_, i) => (projects[i].group || "projects") === g).join("");
  const protos = rows("prototypes");
  el.innerHTML = rows("projects") + (protos ? `<h2>Prototypes</h2>${protos}` : "");
}
// Config entries whose folder has no Rojo project (old WSL paths) cannot be opened.
fetch("/projects").then(r => r.json()).then(p => { projects = p.filter(x => x.ok); drawProjects(); });
load(); setInterval(load, 3000);
</script></body></html>"#;
