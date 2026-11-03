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
//!     "tiers":{"full":["--ensamblador"]}}]}
//! A port held by a process the manager did not start (a service run by hand) is left
//! alone; the manager takes it over once it is free.

use crate::win::{self, NoWindow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const RAM_MARGIN_MB: u64 = 1024;

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
    pub in_flight: usize,
    pub idle_secs: u64,
    /// Last cold start, seconds from spawn to answering its ready path.
    pub last_start_secs: Option<f64>,
    pub error: Option<String>,
}

struct Svc {
    spec: Spec,
    st: Status,
    child: Option<Child>,
    spawned: Option<Instant>,
    last_used: Instant,
    listening: bool,
}

pub struct Services {
    map: Mutex<HashMap<String, Svc>>,
    file: PathBuf,
    dir: PathBuf,
    loaded: Mutex<Option<SystemTime>>,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".roxlit")
}

fn log(dir: &PathBuf, line: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("events.log"))
    {
        let _ = writeln!(f, "{} {line}", crate::session::now());
    }
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

fn pipe(mut from: TcpStream, mut to: TcpStream) {
    let _ = std::io::copy(&mut from, &mut to);
    let _ = to.shutdown(Shutdown::Write);
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
        let r = me.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(15));
            r.reload();
            r.reap();
        });
        me
    }

    /// Re-read the registry when it changed; start a listener for each new service.
    fn reload(self: &Arc<Self>) {
        let mtime = fs::metadata(&self.file).and_then(|m| m.modified()).ok();
        {
            let mut l = self.loaded.lock().unwrap();
            if *l == mtime && mtime.is_some() {
                return;
            }
            *l = mtime;
        }
        let reg: Registry = fs::read_to_string(&self.file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut map = self.map.lock().unwrap();
        for spec in reg.services {
            match map.get_mut(&spec.name) {
                Some(svc) => svc.spec = spec, // applies on the next start
                None => {
                    let name = spec.name.clone();
                    map.insert(
                        name.clone(),
                        Svc {
                            st: Status {
                                name: name.clone(),
                                port: spec.port,
                                state: "stopped".into(),
                                tier: "base".into(),
                                ..Default::default()
                            },
                            spec,
                            child: None,
                            spawned: None,
                            last_used: Instant::now(),
                            listening: false,
                        },
                    );
                    let me = self.clone();
                    std::thread::spawn(move || me.listen(&name));
                }
            }
        }
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
                    if let Some(s) = self.map.lock().unwrap().get_mut(name) {
                        if s.child.is_none() {
                            s.st.state = "external".into();
                        }
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
        self.touch(name, 1);
        let (c2, u2) = match (client.try_clone(), upstream.try_clone()) {
            (Ok(c), Ok(u)) => (c, u),
            _ => {
                self.touch(name, -1);
                return;
            }
        };
        let up = std::thread::spawn(move || pipe(c2, u2));
        pipe(upstream, client);
        let _ = up.join();
        self.touch(name, -1);
    }

    fn touch(&self, name: &str, delta: i64) {
        if let Some(s) = self.map.lock().unwrap().get_mut(name) {
            s.st.in_flight = (s.st.in_flight as i64 + delta).max(0) as usize;
            s.last_used = Instant::now();
        }
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
        let alive = svc
            .child
            .as_mut()
            .map(|c| matches!(c.try_wait(), Ok(None)))
            .unwrap_or(false);
        if alive && want == svc.st.tier {
            svc.last_used = Instant::now();
            return Ok(svc.spec.warm_seconds);
        }
        if alive {
            // Tier change: only when nothing is in flight, so no caller is cut mid-request.
            if svc.st.in_flight > 0 {
                return Err(format!("{name} is busy; retry the tier change"));
            }
            win::kill_tree(svc.st.pid);
            svc.child = None;
        }
        let extra = match want.as_str() {
            "base" => vec![],
            t => svc
                .spec
                .tiers
                .get(t)
                .cloned()
                .ok_or_else(|| format!("{name} has no tier {t}"))?,
        };
        let free = free_ram_mb();
        if svc.spec.ram_mb > 0 && free < svc.spec.ram_mb + RAM_MARGIN_MB {
            return Err(format!(
                "not enough free RAM to start {name}: {free} MB free, needs ~{} MB + {RAM_MARGIN_MB} MB margin",
                svc.spec.ram_mb
            ));
        }
        let inner = free_port();
        let mut args: Vec<String> = svc
            .spec
            .command
            .iter()
            .map(|a| a.replace("{port}", &inner.to_string()))
            .collect();
        args.extend(extra);
        let out =
            fs::File::create(self.dir.join(format!("{name}.log"))).map_err(|e| e.to_string())?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let child = Command::new(&args[0])
            .args(&args[1..])
            .current_dir(&svc.spec.cwd)
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .creation_flags_no_window()
            .spawn()
            .map_err(|e| format!("could not start {name}: {e}"))?;
        svc.st.pid = child.id();
        svc.st.inner_port = inner;
        svc.st.state = "starting".into();
        svc.st.tier = want.clone();
        svc.st.error = None;
        svc.child = Some(child);
        svc.spawned = Some(Instant::now());
        svc.last_used = Instant::now();
        log(
            &self.dir,
            &format!(
                "{name}: start tier={want} pid={} inner={inner} free_ram={free}MB",
                svc.st.pid
            ),
        );
        Ok(svc.spec.warm_seconds)
    }

    /// Wait up to `cap` for the service to answer its ready path. None = not ready yet
    /// (still warming, or failed: see status).
    fn wait_ready(&self, name: &str, cap: Duration) -> Option<u16> {
        let start = Instant::now();
        loop {
            let (port, ready_path, state) = {
                let mut map = self.map.lock().unwrap();
                let svc = map.get_mut(name)?;
                if let Some(c) = svc.child.as_mut() {
                    if let Ok(Some(code)) = c.try_wait() {
                        let tail = fs::read_to_string(self.dir.join(format!("{name}.log")))
                            .unwrap_or_default();
                        let tail: String = tail
                            .lines()
                            .rev()
                            .take(8)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<Vec<_>>()
                            .join("\n");
                        svc.st.state = "failed".into();
                        svc.st.error = Some(format!("exited with {code}: {tail}"));
                        svc.child = None;
                        log(&self.dir, &format!("{name}: died during start ({code})"));
                        return None;
                    }
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
                    let secs = svc
                        .spawned
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0);
                    svc.st.state = "ready".into();
                    svc.st.last_start_secs = Some((secs * 10.0).round() / 10.0);
                    log(
                        &self.dir,
                        &format!("{name}: ready tier={} in {secs:.1}s", svc.st.tier),
                    );
                }
                return Some(port);
            }
            if start.elapsed() >= cap {
                return None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Stop idle services, notice dead ones. A warming service counts as in use.
    fn reap(&self) {
        let mut map = self.map.lock().unwrap();
        for svc in map.values_mut() {
            let Some(c) = svc.child.as_mut() else {
                continue;
            };
            if let Ok(Some(code)) = c.try_wait() {
                svc.child = None;
                svc.st.state = "failed".into();
                svc.st.error = Some(format!("exited with {code}"));
                log(&self.dir, &format!("{}: exited ({code})", svc.st.name));
                continue;
            }
            let idle = svc.last_used.elapsed();
            if svc.st.in_flight == 0
                && svc.st.state == "ready"
                && idle > Duration::from_secs(svc.spec.idle_minutes * 60)
            {
                win::kill_tree(svc.st.pid);
                svc.child = None;
                svc.st.state = "stopped".into();
                svc.st.tier = "base".into();
                log(
                    &self.dir,
                    &format!("{}: stopped after {}s idle", svc.st.name, idle.as_secs()),
                );
            }
        }
    }

    /// Explicit start (a heavier tier, or a pre-warm): waits up to 5 min for ready.
    pub fn start_and_wait(&self, name: &str, tier: Option<&str>) -> Result<Status, String> {
        self.ensure(name, tier)?;
        self.wait_ready(name, Duration::from_secs(300));
        self.touch(name, 0);
        let st = self
            .status(name)
            .ok_or_else(|| format!("no service {name}"))?;
        match st.state.as_str() {
            "ready" => Ok(st),
            "failed" => Err(st.error.unwrap_or_else(|| "failed".into())),
            s => Err(format!("{name} still {s} after 300 s")),
        }
    }

    pub fn stop(&self, name: &str) -> Result<(), String> {
        let mut map = self.map.lock().unwrap();
        let svc = map
            .get_mut(name)
            .ok_or_else(|| format!("no service {name}"))?;
        if svc.child.take().is_some() {
            win::kill_tree(svc.st.pid);
            log(&self.dir, &format!("{name}: stopped on request"));
        }
        svc.st.state = if svc.listening { "stopped" } else { "external" }.into();
        svc.st.tier = "base".into();
        Ok(())
    }

    /// Start (pre-warm) every service registered for the project that holds `cwd`.
    /// Called when a playtest starts, so the first request finds it up or close to it.
    pub fn warm_for(&self, cwd: &str) -> Vec<String> {
        let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_lowercase();
        let cwd = norm(cwd);
        let names: Vec<String> = self
            .map
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.spec.projects.iter().any(|p| cwd.starts_with(&norm(p))))
            .map(|s| s.st.name.clone())
            .collect();
        names
            .into_iter()
            .filter(|n| self.ensure(n, None).is_ok())
            .collect()
    }

    fn status(&self, name: &str) -> Option<Status> {
        self.map.lock().unwrap().get(name).map(|s| s.st.clone())
    }

    pub fn list(&self) -> Vec<Status> {
        let mut map = self.map.lock().unwrap();
        let mut out: Vec<Status> = map
            .values_mut()
            .map(|s| {
                s.st.idle_secs = s.last_used.elapsed().as_secs();
                s.st.clone()
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}
