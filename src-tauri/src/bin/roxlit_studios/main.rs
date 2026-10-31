//! roxlit-studios: session manager for parallel Roblox Studios (IDEA-067).
//! HTTP on 127.0.0.1:19557, separate from the launcher so it can be restarted alone.
//!
//!   GET  /session/<token>  -> {"rojoPort":N}   (asked by the Roxlit session plugin)
//!   GET  /sessions         -> [Session]
//!   POST /open  {"worktree":"C:/..."} -> Session (reuses a live one)
//!   POST /close {"token":"..."}       -> {"ok":true}
//!   GET  /                 -> viewer page (thumbnails, Take control / Give back)
//!   GET  /shot/<token>     -> BMP of that session's Studio
//!   POST /show|/hide {"token":"..."}  -> Session, reopened on Juan's / the hidden desktop

#![windows_subsystem = "windows"]

mod session;
mod view;
mod win;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

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

fn handle(stream: TcpStream, mgr: &session::Manager) {
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
                reply(stream, 200, json!({"rojoPort": s.rojo_port}))
            }
            _ => reply(stream, 404, json!({"error": "no live session"})),
        },
        ("GET", "/sessions") => reply(stream, 200, json!(mgr.list())),
        ("POST", "/open") => match body["worktree"].as_str() {
            Some(wt) => match mgr.open(wt) {
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
            let token = p["/shot/".len()..].split('?').next().unwrap_or("");
            match mgr.get(token).and_then(|s| view::shot(s.studio_pid)) {
                Some(bmp) => reply_raw(stream, 200, "image/bmp", &bmp),
                None => reply(stream, 404, json!({"error": "no window to capture"})),
            }
        }
        ("POST", "/show") | ("POST", "/hide") => match body["token"].as_str() {
            Some(t) => match mgr.set_visible(t, path == "/show") {
                Ok(s) => reply(stream, 200, json!(s)),
                Err(e) => reply(stream, 400, json!({"error": e})),
            },
            None => reply(stream, 400, json!({"error": "token required"})),
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
    let mgr = Arc::new(session::Manager::new());
    for stream in listener.incoming().flatten() {
        let mgr = mgr.clone();
        std::thread::spawn(move || handle(stream, &mgr));
    }
}
