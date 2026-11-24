//! On-demand dev services (IDEA-069). A game under test in Studio calls local services
//! (Driply's feed on 8770). Kept up 24/7 they cost GBs of RAM; kept down, a playtest
//! shows empty data that looks like a real answer. So the manager listens on each
//! service's port itself and starts the real process on the first request, on an inner
//! port, then forwards. No request for `idleMinutes` (and none in flight): it stops.
//!
//! Tiers: a service starts in its cheapest tier (the common case, fast). A heavier tier
//! (`tiers` in the registry, extra args) is asked for explicitly with POST
//! /services/start {name, tier}; only that caller pays its start time.
//!
//! Registry: ~/.roxlit/services.json, re-read when it changes:
//!   {"services":[{"name":"driply-feed","port":8770,"cwd":"C:/...","command":["py.exe",
//!     "feed_service.py","--port","{port}"],"ready":"/ping","idleMinutes":20,
//!     "ramMb":2000,"warmSeconds":20,"projects":["C:/.../Driply"],
//!     "tiers":{"full":["--ensamblador"]},"attribute":"DriplyFeedService"}]}
//! A port held by a process the manager did not start (a service run by hand) is left
//! alone; the manager takes it over once it is free.
//!
//! Survives its own restart: every running process is saved (pid + creation time) in
//! services/state.json and adopted by the next manager instead of started twice.
//!
//! Code changes: the shared instance restarts when a commit touches its cwd; a private
//! one when a source file in its cwd changes. The new process starts beside the old one
//! and takes the traffic once it answers (no gap); if it fails, the old one keeps serving.
//!
//! Private instances: a chat changing a service's code points its game worktree at its
//! own copy (POST /services/private {worktree, name, cwd}, saved in services/private.json).
//! That copy gets its own port; the session Studio of that worktree learns the URL from
//! GET /session/<token> and sets the service's `attribute` on ReplicatedStorage.

use crate::win::{self, NoWindow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const RAM_MARGIN_MB: u64 = 1024;
/// How long a (re)started process may take to answer before it counts as failed.
const START_CAP: Duration = Duration::from_secs(300);
/// How long a replaced process keeps serving requests it already had.
const DRAIN_CAP: Duration = Duration::from_secs(120);
/// A service counts as busy while bytes moved through it this recently. Open connections
/// say nothing: HTTP/1.1 clients (HttpService, the feed) keep pooled ones open for good.
const QUIET: Duration = Duration::from_secs(2);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub name: String,
    pub port: u16,
    pub cwd: String,
    pub command: Vec<String>,
    #[serde(default = "default_ready")]
    pub ready: String,
    #[serde(default = "default_idle")]
    pub idle_minutes: u64,
    #[serde(default)]
    pub ram_mb: u64,
    #[serde(default = "default_warm")]
    pub warm_seconds: u64,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub tiers: HashMap<String, Vec<String>>,
    /// ReplicatedStorage attribute a session Studio sets to a private instance's URL.
    #[serde(default)]
    pub attribute: Option<String>,
    /// When to restart on code changes: "commit" (a commit touches cwd; default for the
    /// shared instance), "files" (a source file in cwd changes; private instances), "off".
    #[serde(default)]
    pub reload: Option<String>,
    /// Source files that count as code for "files" (extensions). Default: py.
    #[serde(default)]
    pub watch: Vec<String>,
}

fn default_ready() -> String {
    "/".into()
}
fn default_idle() -> u64 {
    20
}
fn default_warm() -> u64 {
    20
}

#[derive(Deserialize, Default)]
struct Registry {
    #[serde(default)]
    services: Vec<Spec>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub name: String,
    pub port: u16,
    /// stopped | starting | ready | external (port held by a process we did not start) | failed
    pub state: String,
    pub tier: String,
    pub pid: u32,
    pub inner_port: u16,
    /// Open client connections (pooled ones included, so not a measure of work).
    pub in_flight: usize,
    pub idle_secs: u64,
    /// Last cold start, seconds from spawn to answering its ready path.
    pub last_start_secs: Option<f64>,
    pub error: Option<String>,
    /// Private instance: the game worktree it serves.
    pub owner: Option<String>,
    /// Code the running process started from (commit or file stamp).
    pub code: Option<String>,
}

/// One process, named for good by pid + creation time (see win::started_at).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
struct Proc {
    pid: u32,
    created: u64,
    inner: u16,
}

impl Proc {
    fn alive(&self) -> bool {
        win::started_at(self.pid) == Some(self.created)
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct Saved {
    port: u16,
    tier: String,
    proc: Option<Proc>,
    code: Option<String>,
}

struct Svc {
    spec: Spec,
    st: Status,
    proc: Option<Proc>,
    spawned: Option<Instant>,
    last_used: Instant,
    listening: bool,
    reloading: bool,
    /// now_ms() of the last byte proxied either way.
    traffic: Arc<AtomicU64>,
}

pub struct Services {
    map: Mutex<HashMap<String, Svc>>,
    file: PathBuf,
    dir: PathBuf,
    loaded: Mutex<Option<(Option<SystemTime>, Option<SystemTime>)>>,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".roxlit")
}

fn log(dir: &Path, line: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("events.log"))
    {
        // One write per line: concurrent threads append whole lines.
        let _ = f.write_all(format!("{} {line}\n", crate::session::now()).as_bytes());
    }
}

pub fn norm(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

#[repr(C)]
struct MemStatus {
    len: u32,
    load: u32,
    total_phys: u64,
    avail_phys: u64,
    rest: [u64; 5],
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalMemoryStatusEx(m: *mut MemStatus) -> i32;
}

fn free_ram_mb() -> u64 {
    let mut m = MemStatus {
        len: std::mem::size_of::<MemStatus>() as u32,
        load: 0,
        total_phys: 0,
        avail_phys: 0,
        rest: [0; 5],
    };
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
        return u64::MAX;
    }
    m.avail_phys / (1024 * 1024)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(0)
}

/// One HTTP GET to the inner port; true on any 2xx.
fn answers(port: u16, path: &str) -> bool {
    let Ok(mut s) =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300))
    else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    if write!(s, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").is_err() {
        return false;
    }
    let mut head = [0u8; 12];
    s.read_exact(&mut head).is_ok() && head.starts_with(b"HTTP/") && head[9] == b'2'
}

/// Reply on the public port without the service: drain the request head first so the
/// client sees the answer instead of a reset.
fn refuse(mut s: TcpStream, code: u16, body: Value) {
    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
    let mut buf = [0u8; 8192];
    let _ = s.read(&mut buf);
    let text = body.to_string();
    let retry = body["retryAfter"].as_u64().unwrap_or(5);
    let _ = write!(
        s,
        "HTTP/1.1 {code} Service Unavailable\r\nContent-Type: application/json\r\nRetry-After: {retry}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
}

/// Rewrite an HTTP head to `Connection: close`, unless it is a protocol upgrade.
/// Returns None when `buf` holds no complete head yet.
fn close_head(buf: &[u8]) -> Option<Vec<u8>> {
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]);
    if head.to_ascii_lowercase().contains("\r\nupgrade:") {
        return Some(buf.to_vec());
    }
    let mut out = String::with_capacity(end + 24);
    for (i, line) in head.split("\r\n").enumerate() {
        let l = line.to_ascii_lowercase();
        if i > 0 && (l.starts_with("connection:") || l.starts_with("keep-alive:")) {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    let mut v = out.into_bytes();
    v.extend_from_slice(&buf[end + 4..]);
    Some(v)
}

/// Copy bytes one way, stamping `traffic` on every chunk. The first HTTP head that
/// passes is rewritten to `Connection: close`, so each proxied connection carries one
/// request: a keep-alive pool would otherwise stay on a process after a reload
/// replaced it, and lose a request when that process is killed.
fn pipe(mut from: TcpStream, mut to: TcpStream, traffic: &AtomicU64) {
    const HEAD_CAP: usize = 64 * 1024;
    let mut buf = [0u8; 16 * 1024];
    let mut head: Vec<u8> = Vec::new();
    let mut in_head = true;
    while in_head {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => {
                let _ = to.write_all(&head);
                let _ = to.shutdown(Shutdown::Write);
                return;
            }
            Ok(n) => {
                traffic.store(now_ms(), Ordering::Relaxed);
                head.extend_from_slice(&buf[..n]);
                let out = match close_head(&head) {
                    Some(v) => v,
                    None if head.len() > HEAD_CAP => std::mem::take(&mut head),
                    None => continue,
                };
                if to.write_all(&out).is_err() {
                    return;
                }
                in_head = false;
            }
        }
    }
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                traffic.store(now_ms(), Ordering::Relaxed);
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
    let _ = to.shutdown(Shutdown::Write);
}

fn tail(path: &Path, n: usize) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().rev().take(n).collect();
    lines.reverse();
    lines.join("\n")
}

/// What code a process started from would run, per the spec's reload mode. None = not
/// tracked (reload off, or not a git checkout).
fn fingerprint(spec: &Spec) -> Option<String> {
    match spec.reload.as_deref().unwrap_or("commit") {
        "off" => None,
        "files" => {
            let exts: Vec<String> = if spec.watch.is_empty() {
                vec!["py".into()]
            } else {
                spec.watch.clone()
            };
            let (mut newest, mut count) = (0u128, 0usize);
            for e in fs::read_dir(&spec.cwd).ok()?.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
                if !exts.iter().any(|w| w == ext) {
                    continue;
                }
                count += 1;
                let t = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                newest = newest.max(t);
            }
            Some(format!("files:{count}:{newest}"))
        }
        _ => {
            let out = Command::new("git")
                .args(["-C", &spec.cwd, "log", "-1", "--format=%h", "--", "."])
                .creation_flags_no_window()
                .output()
                .ok()?;
            let h = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !h.is_empty()).then(|| format!("commit:{h}"))
        }
    }
}

/// Spawn one process of `spec` in `tier` on a fresh inner port, logging to `log_path`.
fn spawn(spec: &Spec, tier: &str, log_path: &Path) -> Result<Proc, String> {
    let extra = match tier {
        "base" => vec![],
        t => spec
            .tiers
            .get(t)
            .cloned()
            .ok_or_else(|| format!("{} has no tier {t}", spec.name))?,
    };
    let inner = free_port();
    let mut args: Vec<String> = spec
        .command
        .iter()
        .map(|a| a.replace("{port}", &inner.to_string()))
        .collect();
    args.extend(extra);
    let out = fs::File::create(log_path).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let child = Command::new(&args[0])
        .args(&args[1..])
        .current_dir(&spec.cwd)
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .creation_flags_no_window()
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", spec.name))?;
    let pid = child.id();
    let created = win::started_at(pid).ok_or_else(|| format!("{} exited at once", spec.name))?;
    Ok(Proc {
        pid,
        created,
        inner,
    })
}

impl Svc {
    fn new(spec: Spec, owner: Option<String>, saved: Option<&Saved>) -> Svc {
        let mut st = Status {
            name: spec.name.clone(),
            port: spec.port,
            state: "stopped".into(),
            tier: "base".into(),
            owner,
            ..Default::default()
        };
        let proc = saved.and_then(|s| s.proc).filter(|p| p.alive());
        if let (Some(p), Some(s)) = (proc, saved) {
            st.pid = p.pid;
            st.inner_port = p.inner;
            st.tier = s.tier.clone();
            st.code = s.code.clone();
            st.state = "starting".into(); // promoted to ready once it answers
        }
        Svc {
            spec,
            st,
            proc,
            spawned: None,
            last_used: Instant::now(),
            listening: false,
            reloading: false,
            traffic: Arc::new(AtomicU64::new(0)),
        }
    }

    fn busy(&self) -> bool {
        now_ms().saturating_sub(self.traffic.load(Ordering::Relaxed)) < QUIET.as_millis() as u64
    }

    fn idle(&self) -> Duration {
        let t =
            Duration::from_millis(now_ms().saturating_sub(self.traffic.load(Ordering::Relaxed)));
        t.min(self.last_used.elapsed())
    }

    fn running(&self) -> bool {
        self.proc.map(|p| p.alive()).unwrap_or(false)
    }

    fn kill(&mut self) {
        if let Some(p) = self.proc.take() {
            if p.alive() {
                win::kill_tree(p.pid);
            }
        }
        self.st.pid = 0;
        self.st.inner_port = 0;
    }
}

impl Services {
    pub fn new() -> Arc<Self> {
        let dir = home().join("services");
        let _ = fs::create_dir_all(&dir);
        let me = Arc::new(Services {
            map: Mutex::new(HashMap::new()),
            file: home().join("services.json"),
            dir,
            loaded: Mutex::new(None),
        });
        me.reload();
        me.kill_orphans();
        let r = me.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(15));
            r.reload();
            r.reap();
        });
        me
    }

    fn private_file(&self) -> PathBuf {
        self.dir.join("private.json")
    }

    fn read_saved(&self) -> HashMap<String, Saved> {
        fs::read(self.dir.join("state.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Save what a restarted manager needs to adopt instead of start twice.
    fn persist(&self, map: &HashMap<String, Svc>) {
        let saved: HashMap<&String, Saved> = map
            .iter()
            .map(|(k, s)| {
                (
                    k,
                    Saved {
                        port: s.spec.port,
                        tier: s.st.tier.clone(),
                        proc: s.proc,
                        code: s.st.code.clone(),
                    },
                )
            })
            .collect();
        let path = self.dir.join("state.json");
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, serde_json::to_vec_pretty(&saved).unwrap_or_default()).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    /// Processes saved by a previous manager that no service claims any more.
    fn kill_orphans(&self) {
        let map = self.map.lock().unwrap();
        for (name, s) in self.read_saved() {
            let Some(p) = s.proc else { continue };
            let claimed = map.get(&name).and_then(|x| x.proc) == Some(p);
            if !claimed && p.alive() {
                win::kill_tree(p.pid);
                log(&self.dir, &format!("{name}: killed orphan pid={}", p.pid));
            }
        }
        self.persist(&map);
    }

    /// worktree (normalized) -> service name -> cwd of its private copy.
    fn read_private(&self) -> HashMap<String, HashMap<String, String>> {
        fs::read(self.private_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn private_name(name: &str, worktree: &str) -> String {
        format!(
            "{name}@{}",
            crate::session::token_for(Path::new(worktree), None)
        )
    }

    /// Re-read the registry and the private table when either changed; start a listener
    /// for each new service, stop and drop the ones that are gone.
    fn reload(self: &Arc<Self>) {
        let mt = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
        let stamp = (mt(&self.file), mt(&self.private_file()));
        {
            let mut l = self.loaded.lock().unwrap();
            if *l == Some(stamp) {
                return;
            }
            *l = Some(stamp);
        }
        let reg: Registry = fs::read_to_string(&self.file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let saved = self.read_saved();
        let mut map = self.map.lock().unwrap();

        // Desired set: registry services plus private copies of them.
        let mut want: Vec<(Spec, Option<String>)> =
            reg.services.iter().map(|s| (s.clone(), None)).collect();
        for (wt, svcs) in self.read_private() {
            for (base, cwd) in svcs {
                let Some(b) = reg.services.iter().find(|s| s.name == base) else {
                    continue;
                };
                let name = Self::private_name(&base, &wt);
                let port = map
                    .get(&name)
                    .map(|s| s.spec.port)
                    .or_else(|| saved.get(&name).map(|s| s.port))
                    .filter(|p| *p != 0)
                    .unwrap_or_else(free_port);
                let mut spec = b.clone();
                spec.name = name;
                spec.port = port;
                spec.cwd = cwd;
                spec.projects = vec![];
                if spec.reload.is_none() {
                    spec.reload = Some("files".into());
                }
                want.push((spec, Some(wt.clone())));
            }
        }

        // Gone, moved to another port, or a private copy pointed at another cwd.
        let gone: Vec<String> = map
            .values()
            .filter(|s| {
                !want.iter().any(|(w, _)| {
                    w.name == s.st.name
                        && w.port == s.spec.port
                        && (s.st.owner.is_none() || norm(&w.cwd) == norm(&s.spec.cwd))
                })
            })
            .map(|s| s.st.name.clone())
            .collect();
        for name in gone {
            if let Some(mut svc) = map.remove(&name) {
                svc.kill();
                if svc.listening {
                    // Wake the blocked accept so the listener sees it is gone.
                    let _ = TcpStream::connect(("127.0.0.1", svc.spec.port));
                }
                log(&self.dir, &format!("{name}: removed"));
            }
        }
        for (spec, owner) in want {
            match map.get_mut(&spec.name) {
                Some(svc) => svc.spec = spec, // applies on the next start
                None => {
                    let name = spec.name.clone();
                    let svc = Svc::new(spec, owner, saved.get(&name));
                    if let Some(p) = svc.proc {
                        log(
                            &self.dir,
                            &format!("{name}: adopted pid={} inner={}", p.pid, p.inner),
                        );
                    }
                    map.insert(name.clone(), svc);
                    let me = self.clone();
                    std::thread::spawn(move || me.listen(&name));
                }
            }
        }
        self.persist(&map);
    }

    /// Own the public port. While someone else holds it, report `external` and retry.
    fn listen(self: Arc<Self>, name: &str) {
        let port = match self.map.lock().unwrap().get(name) {
            Some(s) => s.spec.port,
            None => return,
        };
        let listener = loop {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(l) => break l,
                Err(_) => {
                    match self.map.lock().unwrap().get_mut(name) {
                        Some(s) if s.spec.port != port => return,
                        Some(s) if s.proc.is_none() => s.st.state = "external".into(),
                        Some(_) => {}
                        None => return,
                    }
                    std::thread::sleep(Duration::from_secs(15));
                }
            }
        };
        if let Some(s) = self.map.lock().unwrap().get_mut(name) {
            s.listening = true;
            if s.st.state == "external" {
                s.st.state = "stopped".into();
            }
        }
        log(&self.dir, &format!("{name}: listening on {port}"));
        for stream in listener.incoming().flatten() {
            let current = self.map.lock().unwrap().get(name).map(|s| s.spec.port);
            if current != Some(port) {
                log(&self.dir, &format!("{name}: released port {port}"));
                return;
            }
            let me = self.clone();
            let n = name.to_string();
            std::thread::spawn(move || me.serve(&n, stream));
        }
    }

    fn serve(&self, name: &str, client: TcpStream) {
        let warm = match self.ensure(name, None) {
            Ok(w) => w,
            Err(e) => return refuse(client, 503, json!({"service": name, "error": e})),
        };
        let Some(inner) = self.wait_ready(name, Duration::from_secs(warm)) else {
            let st = self.status(name);
            return match st.as_ref().map(|s| s.state.as_str()) {
                Some("failed") => refuse(
                    client,
                    503,
                    json!({"service": name, "error": st.and_then(|s| s.error)}),
                ),
                _ => refuse(
                    client,
                    503,
                    json!({"service": name, "warming": true, "retryAfter": 5}),
                ),
            };
        };
        let Ok(upstream) =
            TcpStream::connect_timeout(&([127, 0, 0, 1], inner).into(), Duration::from_secs(2))
        else {
            return refuse(
                client,
                503,
                json!({"service": name, "warming": true, "retryAfter": 2}),
            );
        };
        let Some(traffic) = self.touch(name, 1) else {
            return;
        };
        let t2 = traffic.clone();
        let (c2, u2) = match (client.try_clone(), upstream.try_clone()) {
            (Ok(c), Ok(u)) => (c, u),
            _ => {
                self.touch(name, -1);
                return;
            }
        };
        let (c3, u3) = (client.try_clone(), upstream.try_clone());
        let up = std::thread::spawn(move || pipe(c2, u2, &t2));
        pipe(upstream, client, &traffic);
        // The service closed: end the client side too, or a client that keeps its half
        // open would block the other copy forever.
        for s in [c3, u3].into_iter().flatten() {
            let _ = s.shutdown(Shutdown::Both);
        }
        let _ = up.join();
        self.touch(name, -1);
    }

    fn touch(&self, name: &str, delta: i64) -> Option<Arc<AtomicU64>> {
        let mut map = self.map.lock().unwrap();
        let s = map.get_mut(name)?;
        s.st.in_flight = (s.st.in_flight as i64 + delta).max(0) as usize;
        s.last_used = Instant::now();
        Some(s.traffic.clone())
    }

    /// Start the service if it is down (or in another tier). Returns how long a caller
    /// may wait for it.
    pub fn ensure(&self, name: &str, tier: Option<&str>) -> Result<u64, String> {
        let mut map = self.map.lock().unwrap();
        let svc = map
            .get_mut(name)
            .ok_or_else(|| format!("no service {name}"))?;
        if !svc.listening {
            return Err(format!(
                "port {} is held by a process the manager did not start",
                svc.spec.port
            ));
        }
        let want = tier.unwrap_or(svc.st.tier.as_str()).to_string();
        let alive = svc.running();
        if alive && want == svc.st.tier {
            svc.last_used = Instant::now();
            return Ok(svc.spec.warm_seconds);
        }
        if alive {
            // Tier change: only when quiet, so no caller is cut mid-request.
            if svc.busy() || svc.reloading {
                return Err(format!("{name} is busy; retry the tier change"));
            }
            svc.kill();
        }
        let free = free_ram_mb();
        if svc.spec.ram_mb > 0 && free < svc.spec.ram_mb + RAM_MARGIN_MB {
            return Err(format!(
                "not enough free RAM to start {name}: {free} MB free, needs ~{} MB + {RAM_MARGIN_MB} MB margin",
                svc.spec.ram_mb
            ));
        }
        let code = fingerprint(&svc.spec);
        let p = spawn(&svc.spec, &want, &self.dir.join(format!("{name}.log")))?;
        svc.st.pid = p.pid;
        svc.st.inner_port = p.inner;
        svc.st.state = "starting".into();
        svc.st.tier = want.clone();
        svc.st.error = None;
        svc.st.code = code;
        svc.proc = Some(p);
        svc.spawned = Some(Instant::now());
        svc.last_used = Instant::now();
        log(
            &self.dir,
            &format!(
                "{name}: start tier={want} pid={} inner={} free_ram={free}MB",
                p.pid, p.inner
            ),
        );
        let warm = svc.spec.warm_seconds;
        self.persist(&map);
        Ok(warm)
    }

    /// Wait up to `cap` for the service to answer its ready path. None = not ready yet
    /// (still warming, or failed: see status).
    fn wait_ready(&self, name: &str, cap: Duration) -> Option<u16> {
        let start = Instant::now();
        loop {
            let (port, ready_path, state) = {
                let mut map = self.map.lock().unwrap();
                let svc = map.get_mut(name)?;
                if svc.proc.is_some() && !svc.running() {
                    svc.proc = None;
                    svc.st.state = "failed".into();
                    svc.st.error = Some(format!(
                        "exited: {}",
                        tail(&self.dir.join(format!("{name}.log")), 8)
                    ));
                    log(&self.dir, &format!("{name}: died during start"));
                    self.persist(&map);
                    return None;
                }
                (
                    svc.st.inner_port,
                    svc.spec.ready.clone(),
                    svc.st.state.clone(),
                )
            };
            if state == "ready" {
                return Some(port);
            }
            if state != "starting" {
                return None;
            }
            if answers(port, &ready_path) {
                let mut map = self.map.lock().unwrap();
                let svc = map.get_mut(name)?;
                if svc.st.state == "starting" {
                    svc.st.state = "ready".into();
                    if let Some(t) = svc.spawned {
                        let secs = t.elapsed().as_secs_f64();
                        svc.st.last_start_secs = Some((secs * 10.0).round() / 10.0);
                        log(
                            &self.dir,
                            &format!("{name}: ready tier={} in {secs:.1}s", svc.st.tier),
                        );
                    } else {
                        log(&self.dir, &format!("{name}: adopted process answers"));
                    }
                }
                return Some(port);
            }
            if start.elapsed() >= cap {
                return None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Stop idle services, notice dead ones, promote adopted ones, restart on new code.
    fn reap(self: &Arc<Self>) {
        let mut check_ready = vec![];
        let mut check_code = vec![];
        {
            let mut map = self.map.lock().unwrap();
            for svc in map.values_mut() {
                if svc.proc.is_none() {
                    continue;
                }
                if !svc.running() {
                    svc.proc = None;
                    svc.st.state = "failed".into();
                    svc.st.error = Some(format!(
                        "exited: {}",
                        tail(&self.dir.join(format!("{}.log", svc.st.name)), 8)
                    ));
                    log(&self.dir, &format!("{}: exited", svc.st.name));
                    continue;
                }
                let idle = svc.idle();
                if !svc.busy()
                    && !svc.reloading
                    && svc.st.state == "ready"
                    && idle > Duration::from_secs(svc.spec.idle_minutes * 60)
                {
                    svc.kill();
                    svc.st.state = "stopped".into();
                    svc.st.tier = "base".into();
                    log(
                        &self.dir,
                        &format!("{}: stopped after {}s idle", svc.st.name, idle.as_secs()),
                    );
                    continue;
                }
                match svc.st.state.as_str() {
                    "starting"
                        if svc
                            .spawned
                            .map(|t| t.elapsed() > START_CAP)
                            .unwrap_or(false) =>
                    {
                        svc.kill();
                        svc.st.state = "failed".into();
                        svc.st.error = Some(format!(
                            "no answer on {} after {} s",
                            svc.spec.ready,
                            START_CAP.as_secs()
                        ));
                    }
                    "starting" => check_ready.push(svc.st.name.clone()),
                    "ready" if !svc.reloading => check_code.push((
                        svc.st.name.clone(),
                        svc.spec.clone(),
                        svc.st.code.clone(),
                    )),
                    _ => {}
                }
            }
            self.persist(&map);
        }
        for name in check_ready {
            self.wait_ready(&name, Duration::ZERO);
        }
        for (name, spec, code) in check_code {
            let now = fingerprint(&spec);
            if now.is_some() && now != code {
                if let Some(s) = self.map.lock().unwrap().get_mut(&name) {
                    s.reloading = true;
                }
                let me = self.clone();
                std::thread::spawn(move || me.hot_swap(&name, now));
            }
        }
    }

    /// New code: start a second process beside the running one and move the traffic to
    /// it once it answers. Without RAM for two, wait for a quiet moment and restart.
    /// A new process that fails leaves the old one serving, with the error in status.
    fn hot_swap(&self, name: &str, code: Option<String>) {
        let Some((spec, tier)) = self
            .map
            .lock()
            .unwrap()
            .get(name)
            .map(|s| (s.spec.clone(), s.st.tier.clone()))
        else {
            return;
        };
        let done = |err: Option<String>| {
            if let Some(s) = self.map.lock().unwrap().get_mut(name) {
                s.reloading = false;
                s.st.code = code.clone();
                if err.is_some() {
                    s.st.error = err;
                }
            }
        };
        log(
            &self.dir,
            &format!("{name}: code changed ({code:?}), reloading"),
        );
        if spec.ram_mb > 0 && free_ram_mb() < spec.ram_mb + RAM_MARGIN_MB {
            // No room for two: restart when idle, at most DRAIN_CAP from now.
            let t = Instant::now();
            while t.elapsed() < DRAIN_CAP {
                if !self.busy(name) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if let Some(s) = self.map.lock().unwrap().get_mut(name) {
                s.reloading = false;
                s.kill();
                s.st.state = "stopped".into();
            }
            let r = self.ensure(name, Some(&tier));
            log(&self.dir, &format!("{name}: restarted for new code: {r:?}"));
            return;
        }
        let log_path = self.dir.join(format!("{name}.log"));
        let prev = self.dir.join(format!("{name}.prev.log"));
        let _ = fs::remove_file(&prev);
        let _ = fs::rename(&log_path, &prev);
        let t = Instant::now();
        let new = match spawn(&spec, &tier, &log_path) {
            Ok(p) => p,
            Err(e) => {
                log(&self.dir, &format!("{name}: reload failed: {e}"));
                return done(Some(format!("reload failed, still on old code: {e}")));
            }
        };
        let mut ok = false;
        while t.elapsed() < START_CAP && new.alive() {
            if answers(new.inner, &spec.ready) {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if !ok {
            if new.alive() {
                win::kill_tree(new.pid);
            }
            let why = tail(&log_path, 8);
            log(
                &self.dir,
                &format!("{name}: reload failed, old process kept"),
            );
            return done(Some(format!("reload failed, still on old code: {why}")));
        }
        let old = {
            let mut map = self.map.lock().unwrap();
            let Some(s) = map.get_mut(name) else {
                win::kill_tree(new.pid);
                return;
            };
            let old = s.proc.replace(new);
            s.st.pid = new.pid;
            s.st.inner_port = new.inner;
            s.st.state = "ready".into();
            s.st.error = None;
            s.st.code = code.clone();
            s.st.last_start_secs = Some((t.elapsed().as_secs_f64() * 10.0).round() / 10.0);
            s.reloading = false;
            self.persist(&map);
            old
        };
        log(
            &self.dir,
            &format!(
                "{name}: reloaded in {:.1}s, pid={}",
                t.elapsed().as_secs_f64(),
                new.pid
            ),
        );
        // Requests already piped to the old process finish there.
        if let Some(old) = old {
            let t = Instant::now();
            while t.elapsed() < DRAIN_CAP && self.busy(name) {
                std::thread::sleep(Duration::from_millis(500));
            }
            if old.alive() {
                win::kill_tree(old.pid);
            }
        }
    }

    /// Explicit start (a heavier tier, or a pre-warm): waits up to 5 min for ready.
    pub fn start_and_wait(&self, name: &str, tier: Option<&str>) -> Result<Status, String> {
        self.ensure(name, tier)?;
        self.wait_ready(name, START_CAP);
        self.touch(name, 0);
        let st = self
            .status(name)
            .ok_or_else(|| format!("no service {name}"))?;
        match st.state.as_str() {
            "ready" => Ok(st),
            "failed" => Err(st.error.unwrap_or_else(|| "failed".into())),
            s => Err(format!("{name} still {s} after {} s", START_CAP.as_secs())),
        }
    }

    pub fn stop(&self, name: &str) -> Result<(), String> {
        let mut map = self.map.lock().unwrap();
        let svc = map
            .get_mut(name)
            .ok_or_else(|| format!("no service {name}"))?;
        if svc.proc.is_some() {
            svc.kill();
            log(&self.dir, &format!("{name}: stopped on request"));
        }
        svc.st.state = if svc.listening { "stopped" } else { "external" }.into();
        svc.st.tier = "base".into();
        self.persist(&map);
        Ok(())
    }

    /// The private instances (by game worktree) that `cwd` falls under.
    fn privates_for(&self, cwd: &str) -> Vec<String> {
        let cwd = norm(cwd);
        self.map
            .lock()
            .unwrap()
            .values()
            .filter(|s| {
                s.st.owner
                    .as_deref()
                    .map(|o| cwd.starts_with(&norm(o)))
                    .unwrap_or(false)
            })
            .map(|s| s.st.name.clone())
            .collect()
    }

    /// Start (pre-warm) every service registered for the project that holds `cwd`, or
    /// that worktree's private copy of it. Called when a playtest starts.
    pub fn warm_for(&self, cwd: &str) -> Vec<String> {
        let privates = self.privates_for(cwd);
        let c = norm(cwd);
        let names: Vec<String> = self
            .map
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.st.owner.is_none())
            .filter(|s| s.spec.projects.iter().any(|p| c.starts_with(&norm(p))))
            .map(|s| {
                privates
                    .iter()
                    .find(|p| p.starts_with(&format!("{}@", s.st.name)))
                    .cloned()
                    .unwrap_or_else(|| s.st.name.clone())
            })
            .collect();
        names
            .into_iter()
            .filter(|n| self.ensure(n, None).is_ok())
            .collect()
    }

    /// Point a game worktree's `name` at a private copy of the service in `cwd`, or back
    /// at the shared one (cwd None). Returns that worktree's attributes.
    pub fn set_private(
        self: &Arc<Self>,
        worktree: &str,
        name: &str,
        cwd: Option<&str>,
    ) -> Result<Value, String> {
        let wt = norm(worktree);
        {
            let map = self.map.lock().unwrap();
            let base = map
                .get(name)
                .filter(|s| s.st.owner.is_none())
                .ok_or_else(|| format!("no service {name}"))?;
            if cwd.is_some() && base.spec.attribute.is_none() {
                return Err(format!(
                    "{name} has no \"attribute\" in services.json, so a game cannot be pointed at a private copy"
                ));
            }
        }
        let mut table = self.read_private();
        match cwd {
            Some(c) => {
                if !Path::new(c).is_dir() {
                    return Err(format!("{c} is not a directory"));
                }
                table
                    .entry(wt.clone())
                    .or_default()
                    .insert(name.into(), c.replace('\\', "/"));
            }
            None => {
                if let Some(m) = table.get_mut(&wt) {
                    m.remove(name);
                }
                table.retain(|_, m| !m.is_empty());
            }
        }
        let path = self.private_file();
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&table).unwrap_or_default())
            .and_then(|_| fs::rename(&tmp, &path))
            .map_err(|e| e.to_string())?;
        log(&self.dir, &format!("{name}: private for {wt} -> {cwd:?}"));
        self.reload();
        Ok(self.attributes_for(&wt))
    }

    /// For a session Studio: every attribute the registry manages, set to the URL of that
    /// worktree's private copy, or null where the game should use its default.
    pub fn attributes_for(&self, worktree: &str) -> Value {
        let wt = norm(worktree);
        let map = self.map.lock().unwrap();
        let mut out = serde_json::Map::new();
        for s in map.values().filter(|s| s.st.owner.is_none()) {
            let Some(attr) = &s.spec.attribute else {
                continue;
            };
            let private = map.values().find(|p| {
                p.st.owner.as_deref().map(norm) == Some(wt.clone())
                    && p.st.name.starts_with(&format!("{}@", s.st.name))
            });
            out.insert(
                attr.clone(),
                private
                    .map(|p| json!(format!("http://127.0.0.1:{}", p.spec.port)))
                    .unwrap_or(Value::Null),
            );
        }
        Value::Object(out)
    }

    fn busy(&self, name: &str) -> bool {
        self.map
            .lock()
            .unwrap()
            .get(name)
            .map(|s| s.busy())
            .unwrap_or(false)
    }

    fn status(&self, name: &str) -> Option<Status> {
        self.map.lock().unwrap().get(name).map(|s| s.st.clone())
    }

    pub fn list(&self) -> Vec<Status> {
        let mut map = self.map.lock().unwrap();
        let mut out: Vec<Status> = map
            .values_mut()
            .map(|s| {
                s.st.idle_secs = s.idle().as_secs();
                s.st.clone()
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::close_head;

    #[test]
    fn request_head_gets_connection_close() {
        let req =
            b"GET /feed HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\nKeep-Alive: 5\r\n\r\nbody";
        let out = String::from_utf8(close_head(req).unwrap()).unwrap();
        assert_eq!(
            out,
            "GET /feed HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\nbody"
        );
    }

    #[test]
    fn incomplete_head_waits() {
        assert!(close_head(b"GET / HTTP/1.1\r\nHost: x\r\n").is_none());
    }

    #[test]
    fn upgrade_passes_untouched() {
        let req = b"GET /ws HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n";
        assert_eq!(close_head(req).unwrap(), req.to_vec());
    }

    /// The idle background loop (reload + reap), 1000 times (4 h of the live 15 s loop),
    /// on a copy of the live registry in a temp dir so live state is never written.
    #[test]
    #[ignore]
    fn bench_services_tick() {
        use super::*;
        let dir = std::env::temp_dir().join("roxlit-services-bench");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("services.json");
        let _ = fs::copy(home().join("services.json"), &file);
        let me = Arc::new(Services {
            map: Mutex::new(HashMap::new()),
            file,
            dir,
            loaded: Mutex::new(None),
        });
        crate::win::leak_bench("services reload+reap", 1000, || {
            me.reload();
            me.reap();
        });
    }
}
