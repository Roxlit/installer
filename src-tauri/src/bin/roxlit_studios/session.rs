//! Session Studios: one per worktree. Open = `rojo build` the worktree to a local place
//! named `roxlit-<token>.rbxl`, `rojo serve` it on its own port, launch Studio on that
//! file on the hidden Studio desktop. The Roxlit session plugin in
//! that Studio reads the token from the place name and asks this manager for the port.
//! Every wait is capped; nothing here blocks forever on Studio or Rojo.

use crate::win::{self, NoWindow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FIRST_PORT: u16 = 34900;
const BUILD_CAP: Duration = Duration::from_secs(120);
const SERVE_CAP: Duration = Duration::from_secs(30);
const LOAD_CAP: Duration = Duration::from_secs(120);
/// Playtest fps cap. The hidden desktop has no vsync, so an uncapped Studio renders at
/// the global `FramerateCap` (240 measured, ~1.7x the GPU of a 60 fps Studio).
const FPS_CAP: i64 = 60;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub token: String,
    pub worktree: String,
    /// Place file name; also the name the Roblox Studio MCP lists for this Studio.
    pub place: String,
    pub rojo_port: u16,
    pub studio_pid: u32,
    pub rojo_pid: u32,
    pub opened: u64,
    /// starting | ready | failed | crashed
    pub state: String,
    pub error: Option<String>,
}

pub struct Manager {
    sessions: Mutex<HashMap<String, Session>>,
    dir: PathBuf,
    registry: PathBuf,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Stable 8-hex token per worktree path, so a worktree always gets the same place name.
pub fn token_for(worktree: &Path) -> String {
    let norm = worktree
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    let mut h: u32 = 0x811c9dc5;
    for b in norm.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    format!("{h:08x}")
}

fn rojo_exe() -> PathBuf {
    let shim = dirs::home_dir()
        .unwrap_or_default()
        .join(".aftman")
        .join("bin")
        .join("rojo.exe");
    if shim.exists() {
        shim
    } else {
        PathBuf::from("rojo")
    }
}

/// Newest installed RobloxStudioBeta.exe.
fn studio_exe() -> Option<PathBuf> {
    let versions = dirs::data_local_dir()?.join("Roblox").join("Versions");
    fs::read_dir(versions)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("RobloxStudioBeta.exe"))
        .filter(|p| p.exists())
        .max_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Lower Studio's global `FramerateCap` to FPS_CAP before a launch. The setting is shared
/// by every Studio (no per-process override: Studio ignores a LOCALAPPDATA override and
/// Luau cannot set it) and a running Studio writes its own value back on exit, so this
/// runs before each launch.
fn cap_framerate() {
    let Some(path) =
        dirs::data_local_dir().map(|d| d.join("Roblox").join("GlobalBasicSettings_13_Studio.xml"))
    else {
        return;
    };
    let Ok(xml) = fs::read_to_string(&path) else {
        return;
    };
    const KEY: &str = "<int name=\"FramerateCap\">";
    let Some(at) = xml.find(KEY).map(|i| i + KEY.len()) else {
        return;
    };
    let Some(len) = xml[at..].find('<') else {
        return;
    };
    let cur: i64 = xml[at..at + len].trim().parse().unwrap_or(-1);
    if cur > 0 && cur <= FPS_CAP {
        return;
    }
    let out = format!("{}{FPS_CAP}{}", &xml[..at], &xml[at + len..]);
    let _ = fs::write(&path, out);
}

/// Wait for a child with a cap; kill it at the cap.
fn wait_capped(mut child: Child, cap: Duration) -> Result<(), String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => return Ok(()),
            Ok(Some(s)) => return Err(format!("exited with {s}")),
            Ok(None) if start.elapsed() > cap => {
                win::kill_tree(child.id());
                return Err(format!("still running after {} s, killed", cap.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn wait_listening(port: u16, cap: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < cap {
        if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300))
            .is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

impl Manager {
    pub fn new() -> Self {
        let root = dirs::home_dir().unwrap_or_default().join(".roxlit");
        let dir = root.join("sessions");
        let _ = fs::create_dir_all(&dir);
        let registry = root.join("studios.json");
        let sessions: HashMap<String, Session> = fs::read(&registry)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let m = Manager {
            sessions: Mutex::new(sessions),
            dir,
            registry,
        };
        m.refresh();
        m
    }

    fn save(&self, sessions: &HashMap<String, Session>) {
        let tmp = self.registry.with_extension("tmp");
        if fs::write(
            &tmp,
            serde_json::to_vec_pretty(sessions).unwrap_or_default(),
        )
        .is_ok()
        {
            let _ = fs::rename(&tmp, &self.registry);
        }
    }

    /// Mark sessions whose Studio died as crashed and stop their orphan Rojo.
    pub fn refresh(&self) {
        let mut s = self.sessions.lock().unwrap();
        for sess in s.values_mut() {
            if sess.state != "ready" {
                continue;
            }
            if !win::pid_alive(sess.studio_pid) {
                sess.state = "crashed".into();
                win::kill_tree(sess.rojo_pid);
            }
        }
        self.save(&s);
    }

    pub fn list(&self) -> Vec<Session> {
        self.refresh();
        self.sessions.lock().unwrap().values().cloned().collect()
    }

    pub fn get(&self, token: &str) -> Option<Session> {
        self.sessions.lock().unwrap().get(token).cloned()
    }

    pub fn open(&self, worktree: &str) -> Result<Session, String> {
        let wt = fs::canonicalize(worktree).map_err(|e| format!("worktree {worktree}: {e}"))?;
        let wt = PathBuf::from(wt.to_string_lossy().trim_start_matches(r"\\?\"));
        if !wt.join("default.project.json").exists() {
            return Err(format!("{} has no default.project.json", wt.display()));
        }
        let token = token_for(&wt);
        let place_name = format!("roxlit-{token}.rbxl");
        let place = self.dir.join(&place_name);

        // Reserve under the lock so two opens of one worktree do not launch two Studios.
        let port = {
            let mut s = self.sessions.lock().unwrap();
            if let Some(old) = s.get(&token) {
                let alive = win::pid_alive(old.studio_pid) && win::pid_alive(old.rojo_pid);
                if old.state == "starting" || (old.state == "ready" && alive) {
                    return Ok(old.clone());
                }
                win::kill_tree(old.studio_pid);
                win::kill_tree(old.rojo_pid);
            }
            let used: Vec<u16> = s
                .values()
                .filter(|x| x.token != token)
                .map(|x| x.rojo_port)
                .collect();
            let port = (FIRST_PORT..FIRST_PORT + 100)
                .find(|p| !used.contains(p) && port_free(*p))
                .ok_or("no free Rojo port in 34900-34999")?;
            s.insert(
                token.clone(),
                Session {
                    token: token.clone(),
                    worktree: wt.to_string_lossy().into(),
                    place: place_name.clone(),
                    rojo_port: port,
                    studio_pid: 0,
                    rojo_pid: 0,
                    opened: now(),
                    state: "starting".into(),
                    error: None,
                },
            );
            self.save(&s);
            port
        };

        let result = self.start(&wt, &token, &place, &place_name, port);
        let mut s = self.sessions.lock().unwrap();
        let sess = s.get_mut(&token).ok_or("session vanished")?;
        match result {
            Ok((studio_pid, rojo_pid)) => {
                sess.studio_pid = studio_pid;
                sess.rojo_pid = rojo_pid;
                sess.state = "ready".into();
            }
            Err(e) => {
                sess.state = "failed".into();
                sess.error = Some(e);
            }
        }
        let out = sess.clone();
        self.save(&s);
        Ok(out)
    }

    fn start(
        &self,
        wt: &Path,
        token: &str,
        place: &Path,
        name: &str,
        port: u16,
    ) -> Result<(u32, u32), String> {
        // stdout and stderr share one log per step, so a failed build says why.
        let log = |suffix: &str| -> (Stdio, Stdio) {
            match fs::File::create(self.dir.join(format!("{token}.{suffix}.log"))) {
                Ok(f) => match f.try_clone() {
                    Ok(g) => (Stdio::from(f), Stdio::from(g)),
                    Err(_) => (Stdio::from(f), Stdio::null()),
                },
                Err(_) => (Stdio::null(), Stdio::null()),
            }
        };
        let project = self.wrapper(wt, token, name)?;
        let (out, err) = log("build");
        let build = Command::new(rojo_exe())
            .current_dir(wt)
            .creation_flags_no_window()
            .arg("build")
            .arg(&project)
            .arg("-o")
            .arg(place)
            .stdout(out)
            .stderr(err)
            .spawn()
            .map_err(|e| format!("rojo build: {e}"))?;
        wait_capped(build, BUILD_CAP).map_err(|e| {
            let tail = fs::read_to_string(self.dir.join(format!("{token}.build.log")))
                .map(|t| t.lines().rev().take(3).collect::<Vec<_>>().join(" | "))
                .unwrap_or_default();
            format!("rojo build {e}: {tail}")
        })?;

        let (out, err) = log("serve");

        let serve = Command::new(rojo_exe())
            .current_dir(wt)
            .creation_flags_no_window()
            .arg("serve")
            .arg(&project)
            .args(["--port", &port.to_string()])
            .stdout(out)
            .stderr(err)
            .spawn()
            .map_err(|e| format!("rojo serve: {e}"))?;
        let rojo_pid = serve.id();
        if !wait_listening(port, SERVE_CAP) {
            win::kill_tree(rojo_pid);
            return Err(format!(
                "rojo serve not listening on {port} after {} s",
                SERVE_CAP.as_secs()
            ));
        }

        let exe = studio_exe().ok_or("RobloxStudioBeta.exe not found")?;
        cap_framerate();
        let studio_pid =
            win::launch_hidden(&exe, place).map_err(|e| format!("launch Studio: {e}"))?;
        if !win::wait_loaded(studio_pid, name, LOAD_CAP) {
            win::kill_tree(studio_pid);
            win::kill_tree(rojo_pid);
            return Err(format!(
                "Studio did not load {name} within {} s",
                LOAD_CAP.as_secs()
            ));
        }
        Ok((studio_pid, rojo_pid))
    }

    /// Project file that nests the worktree's `default.project.json` under a root named
    /// after the place. The Roblox Studio MCP lists a local place by `game.Name`, and Rojo
    /// renames the DataModel to the project name on sync, so without this two worktrees
    /// of one game would both list as e.g. "Driply" and the session plugin, which reads
    /// the token from `game.Name`, would lose it after the first sync.
    fn wrapper(&self, wt: &Path, token: &str, name: &str) -> Result<PathBuf, String> {
        let inner = wt.join("default.project.json");
        let body = serde_json::json!({
            "name": name,
            "tree": { "$path": inner.to_string_lossy().replace('\\', "/") },
        });
        let path = self.dir.join(format!("{token}.project.json"));
        fs::write(&path, body.to_string()).map_err(|e| format!("write {}: {e}", path.display()))?;
        Ok(path)
    }

    /// Close = kill. The worktree holds the scripts and managed tree; the place file
    /// stays for the cache.
    pub fn close(&self, token: &str) -> Result<(), String> {
        let mut s = self.sessions.lock().unwrap();
        let sess = s
            .remove(token)
            .ok_or_else(|| format!("no session {token}"))?;
        win::kill_tree(sess.studio_pid);
        win::kill_tree(sess.rojo_pid);
        self.save(&s);
        Ok(())
    }
}
