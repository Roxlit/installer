//! roxlit-studios: session manager for parallel Roblox Studios (IDEA-067).
//! HTTP on 127.0.0.1:19557, separate from the launcher so it can be restarted alone.
//!
//!   GET  /session/<token>  -> {"rojoPort":N,"attributes":{attr:url},"managed":[attr]}
//!                             (asked by the Roxlit session plugin; attributes go on
//!                             ReplicatedStorage, managed ones without a URL are cleared)
//!   GET  /sessions         -> [Session]
//!   POST /open  {"worktree":"C:/..."} -> Session (reuses a live one)
//!   POST /close {"token":"..."}       -> {"ok":true}
//!   GET  /projects         -> [{name,path,branch,ok,token}] from ~/.roxlit/config.json; the
//!                             viewer's "Open & play" opens the main checkout with key "juan"
//!   GET  /                 -> viewer page (live captures, Take control / Give back)
//!   GET  /shot/<token>[?w=N] -> PNG of that session's Studio, at most N px wide
//!   POST /show|/hide {"token":"..."}  -> Session, moved to the primary monitor / back
//!   GET  /services         -> [Status] of the on-demand dev services (IDEA-069)
//!   POST /services/start {"name":"...","tier":"..."?} -> start / change tier, waits until ready
//!   POST /services/stop  {"name":"..."}
//!   POST /services/warm  {"cwd":"C:/..."} -> start the services of that project (playtest start)
//!   POST /services/private {"worktree":"C:/...","name":"...","cwd":"C:/..."|null}
//!        -> point that game worktree at a private copy of the service (null: back to shared)

#![windows_subsystem = "windows"]

mod audio;
mod guard;
mod recent;
mod screen;
mod services;
mod session;
#[cfg(test)]
mod shotbench;
mod view;
mod win;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
}

const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
const ADDR: &str = "127.0.0.1:19557";
const MAX_BODY: usize = 64 * 1024;

fn reply(stream: TcpStream, code: u16, body: Value) {
    reply_raw(
        stream,
        code,
        "application/json",
        body.to_string().as_bytes(),
    )
}

fn reply_raw(mut stream: TcpStream, code: u16, kind: &str, body: &[u8]) {
    let status = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {code} {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(body);
}

/// Session key of the Studios Juan opens from the viewer, apart from any chat's.
const JUAN_KEY: &str = "juan";

/// Subfolders of `parent` that hold a Rojo project, sorted by name.
fn rojo_dirs(parent: &std::path::Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("default.project.json").exists())
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            Some((name, p.to_string_lossy().into_owned()))
        })
        .collect();
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out
}

/// Every Rojo project in ~/RobloxProjects ("projects") and ~/RobloxProjects/Idea
/// ("prototypes"), plus launcher-config entries living elsewhere; one per path, with
/// the branch checked out. A new folder shows up without registering it anywhere.
fn projects() -> Value {
    let home = dirs::home_dir().unwrap_or_default();
    let root = home.join("RobloxProjects");
    let cfg: Value = std::fs::read(home.join(".roxlit").join("config.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    let mut found: Vec<(String, String, &str)> = Vec::new();
    found.extend(
        rojo_dirs(&root)
            .into_iter()
            .map(|(n, p)| (n, p, "projects")),
    );
    found.extend(
        rojo_dirs(&root.join("Idea"))
            .into_iter()
            .map(|(n, p)| (n, p, "prototypes")),
    );
    for p in cfg["projects"].as_array().into_iter().flatten() {
        if let Some(path) = p["path"].as_str() {
            let name = p["name"].as_str().unwrap_or(path).to_string();
            found.push((name, path.to_string(), "projects"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (name, path, group) in &found {
        if !seen.insert(path.replace('\\', "/").to_lowercase()) {
            continue;
        }
        let path = path.as_str();
        let dir = std::path::Path::new(path);
        let branch = std::fs::read_to_string(dir.join(".git").join("HEAD"))
            .ok()
            .map(|h| {
                let h = h.trim();
                h.strip_prefix("ref: refs/heads/")
                    .unwrap_or(&h[..h.len().min(8)])
                    .to_string()
            });
        let ok = dir.join("default.project.json").exists();
        let token = std::fs::canonicalize(dir)
            .map(|c| {
                let c = std::path::PathBuf::from(c.to_string_lossy().trim_start_matches(r"\\?\"));
                session::token_for(&c, Some(JUAN_KEY))
            })
            .ok();
        out.push(json!({
            "name": name,
            "path": path,
            "group": group,
            "branch": branch,
            "ok": ok,
            "token": token,
        }));
    }
    json!(out)
}

fn handle(stream: TcpStream, mgr: &session::Manager, svcs: &Arc<services::Services>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0).min(MAX_BODY);
            }
        }
    }
    let mut body = vec![0u8; len];
    if len > 0 && reader.read_exact(&mut body).is_err() {
        return reply(stream, 400, json!({"error": "short body"}));
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

    match (method, path) {
        ("GET", p) if p.starts_with("/session/") => match mgr.get(&p["/session/".len()..]) {
            Some(s) if s.state == "starting" || s.state == "ready" => {
                // JSON null is dropped by Luau's JSONDecode, so the attributes to clear
                // travel as a list: `managed` = every attribute, `attributes` = set ones.
                let all = svcs.attributes_for(&s.worktree);
                let all = all.as_object().cloned().unwrap_or_default();
                let managed: Vec<&String> = all.keys().collect();
                let set: serde_json::Map<String, Value> = all
                    .iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                reply(
                    stream,
                    200,
                    json!({"rojoPort": s.rojo_port, "attributes": set, "managed": managed}),
                )
            }
            _ => reply(stream, 404, json!({"error": "no live session"})),
        },
        ("GET", "/sessions") => reply(stream, 200, json!(mgr.list())),
        ("GET", "/projects") => reply(stream, 200, projects()),
        ("POST", "/open") => match body["worktree"].as_str() {
            Some(wt) => match mgr.open(wt, body["key"].as_str()) {
                Ok(s) => reply(stream, 200, json!(s)),
                Err(e) => reply(stream, 400, json!({"error": e})),
            },
            None => reply(stream, 400, json!({"error": "worktree required"})),
        },
        ("POST", "/close") => match body["token"].as_str() {
            Some(t) => match mgr.close(t) {
                Ok(()) => reply(stream, 200, json!({"ok": true})),
                Err(e) => reply(stream, 404, json!({"error": e})),
            },
            None => reply(stream, 400, json!({"error": "token required"})),
        },
        ("GET", "/") => reply_raw(
            stream,
            200,
            "text/html; charset=utf-8",
            view::PAGE.as_bytes(),
        ),
        ("GET", p) if p.starts_with("/shot/") => {
            let (token, query) = p["/shot/".len()..].split_once('?').unwrap_or((&p[6..], ""));
            let width = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("w=")?.parse().ok())
                .unwrap_or(960);
            match mgr
                .get(token)
                .and_then(|s| view::shot(s.studio_pid, s.visible, width))
            {
                Some(png) => reply_raw(stream, 200, "image/png", &png[..]),
                None => reply(stream, 404, json!({"error": "no window to capture"})),
            }
        }
        ("POST", "/show") | ("POST", "/hide") => match body["token"].as_str() {
            Some(t) => match mgr.set_front(t, path == "/show") {
                Ok(s) => reply(stream, 200, json!(s)),
                Err(e) => reply(stream, 400, json!({"error": e})),
            },
            None => reply(stream, 400, json!({"error": "token required"})),
        },
        ("GET", "/services") => reply(stream, 200, json!(svcs.list())),
        ("POST", "/services/start") => match body["name"].as_str() {
            Some(n) => match svcs.start_and_wait(n, body["tier"].as_str()) {
                Ok(s) => reply(stream, 200, json!(s)),
                Err(e) => reply(stream, 400, json!({"error": e})),
            },
            None => reply(stream, 400, json!({"error": "name required"})),
        },
        ("POST", "/services/stop") => match body["name"].as_str() {
            Some(n) => match svcs.stop(n) {
                Ok(()) => reply(stream, 200, json!({"ok": true})),
                Err(e) => reply(stream, 404, json!({"error": e})),
            },
            None => reply(stream, 400, json!({"error": "name required"})),
        },
        ("POST", "/services/warm") => match body["cwd"].as_str() {
            Some(c) => reply(stream, 200, json!({"warming": svcs.warm_for(c)})),
            None => reply(stream, 400, json!({"error": "cwd required"})),
        },
        ("POST", "/services/private") => match (body["worktree"].as_str(), body["name"].as_str()) {
            (Some(wt), Some(n)) => match svcs.set_private(wt, n, body["cwd"].as_str()) {
                Ok(attrs) => reply(stream, 200, json!({"attributes": attrs})),
                Err(e) => reply(stream, 400, json!({"error": e})),
            },
            _ => reply(stream, 400, json!({"error": "worktree and name required"})),
        },
        _ => reply(stream, 404, json!({"error": "unknown route"})),
    }
}

fn main() {
    // A second instance fails to bind and exits: one manager per machine.
    let listener = match TcpListener::bind(ADDR) {
        Ok(l) => l,
        Err(_) => return,
    };
    // Without this, Windows scales every window rect to 96 dpi for us (Juan's displays are
    // at 125%) and captures come out cropped to 80% of the window.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let mgr = Arc::new(session::Manager::new());
    let svcs = services::Services::new();
    guard::start();
    recent::start();
    audio::start(mgr.clone());
    // Parked Studios still grab focus at times (load, play); taken ones are Juan's.
    for s in mgr.list().iter().filter(|s| s.visible && !s.front) {
        guard::watch(s.studio_pid);
    }
    for stream in listener.incoming().flatten() {
        let (mgr, svcs) = (mgr.clone(), svcs.clone());
        std::thread::spawn(move || handle(stream, &mgr, &svcs));
    }
}
