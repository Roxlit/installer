use serde::Serialize;
use std::sync::Arc;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::error::{InstallerError, Result};
use crate::util::expand_tilde;

/// Events streamed from the rojo serve process to the frontend.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum RojoEvent {
    #[serde(rename_all = "camelCase")]
    Output { line: String, stream: String },
    #[serde(rename_all = "camelCase")]
    Started { port: u16 },
    Stopped { code: Option<i32> },
    Error { message: String },
}

/// Managed state holding the rojo child process.
pub struct RojoProcess {
    pub child: Arc<Mutex<Option<tokio::process::Child>>>,
    pub abort_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl Default for RojoProcess {
    fn default() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            abort_handle: Arc::new(Mutex::new(None)),
        }
    }
}

impl RojoProcess {
    /// Kill the rojo process synchronously (for window close handler).
    pub fn kill_sync(&self) {
        // Try to kill the child process
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(ref mut child) = *guard {
                let _ = child.start_kill();
            }
            *guard = None;
        }
        // Abort the reader task
        if let Ok(mut guard) = self.abort_handle.try_lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }
}

/// Resolve the rojo binary path (aftman installs to ~/.aftman/bin/).
fn rojo_bin_path() -> String {
    if let Some(home) = dirs::home_dir() {
        let aftman_rojo = if cfg!(target_os = "windows") {
            home.join(".aftman").join("bin").join("rojo.exe")
        } else {
            home.join(".aftman").join("bin").join("rojo")
        };
        if aftman_rojo.exists() {
            return aftman_rojo.to_string_lossy().to_string();
        }
    }
    // Fallback to PATH
    "rojo".to_string()
}

/// Start `rojo serve` in the given project directory and stream output.
#[tauri::command]
pub async fn start_rojo(
    project_path: String,
    on_event: Channel<RojoEvent>,
    state: tauri::State<'_, RojoProcess>,
) -> Result<()> {
    // Check if already running
    {
        let guard = state.child.lock().await;
        if guard.is_some() {
            return Err(InstallerError::Custom(
                "Rojo is already running".into(),
            ));
        }
    }

    let rojo = rojo_bin_path();
    let project_path = expand_tilde(&project_path);

    // Kill any orphaned rojo process holding the port from a previous session
    kill_orphaned_rojo().await;

    // Ensure project directory and essential config files exist
    let project_dir = std::path::Path::new(&project_path);
    if !project_dir.exists() {
        std::fs::create_dir_all(project_dir).map_err(|e| {
            InstallerError::Custom(format!("Failed to create project directory: {e}"))
        })?;
    }

    let aftman_toml = project_dir.join("aftman.toml");
    if !aftman_toml.exists() {
        std::fs::write(&aftman_toml, "[tools]\nrojo = \"rojo-rbx/rojo@7.4.4\"\n")
            .map_err(|e| InstallerError::Custom(format!(
                "Failed to write aftman.toml at {}: {e}", aftman_toml.display()
            )))?;
    }

    // Migrate legacy projects: move .luau files from src/ to scripts/
    let scripts_dir = project_dir.join("scripts");
    let legacy_src = project_dir.join("src");
    if !scripts_dir.exists() && legacy_src.exists() {
        // Check if src/ has any .luau files (legacy layout)
        let has_luau = has_luau_files(&legacy_src);
        if has_luau {
            let _ = std::fs::create_dir_all(&scripts_dir);
            move_luau_tree(&legacy_src, &scripts_dir);
        }
    }

    let project_json = project_dir.join("default.project.json");
    // Rewrite project.json if it still references src/ for scripts
    if project_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&project_json) {
            if content.contains("\"src/ServerScriptService\"")
                || content.contains("\"src/StarterPlayer")
                || content.contains("\"src/ReplicatedStorage\"")
            {
                let name = project_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("my-game");
                let _ = std::fs::write(&project_json, crate::templates::project_json(name));
            }
        }
    } else {
        let name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-game");
        std::fs::write(&project_json, crate::templates::project_json(name))
            .map_err(|e| InstallerError::Custom(format!(
                "Failed to write default.project.json at {}: {e}", project_json.display()
            )))?;
    }

    // Update .rbxsyncignore to include scripts/ if missing
    let rbxsyncignore = project_dir.join(".rbxsyncignore");
    if rbxsyncignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&rbxsyncignore) {
            if !content.contains("scripts/") {
                let _ = std::fs::write(
                    &rbxsyncignore,
                    format!("{}scripts/\n", content),
                );
            }
        }
    }

    // Regenerate AI context if it still references the old src/ layout
    regenerate_ai_context_if_stale(project_dir, &project_path);

    // Ensure project directories exist (user may have deleted scripts/)
    for subdir in &[
        "scripts/ServerScriptService",
        "scripts/StarterPlayer/StarterPlayerScripts",
        "scripts/StarterPlayer/StarterCharacterScripts",
        "scripts/ReplicatedStorage",
        "scripts/ReplicatedFirst",
        "scripts/ServerStorage",
        "scripts/Workspace",
        "scripts/StarterGui",
        "scripts/StarterPack",
    ] {
        let dir = project_dir.join(subdir);
        if !dir.exists() {
            let _ = std::fs::create_dir_all(&dir);
        }
    }

    let mut cmd = tokio::process::Command::new(&rojo);
    cmd.arg("serve")
        .current_dir(&project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn()
        .map_err(|e| InstallerError::Custom(format!("Failed to start rojo: {e}")))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Store the child process
    {
        let mut guard = state.child.lock().await;
        *guard = Some(child);
    }

    let child_arc = state.child.clone();
    let event_clone = on_event.clone();

    // Spawn a task to read stdout + stderr and stream events
    let reader_handle = tokio::spawn(async move {
        let mut port_detected = false;

        // Read stdout
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(raw_line)) => {
                        let line = strip_ansi(&raw_line);
                        // Try to detect the port from rojo output
                        if !port_detected {
                            if let Some(port) = parse_rojo_port(&line) {
                                port_detected = true;
                                let _ = event_clone.send(RojoEvent::Started { port });
                            }
                        }
                        let _ = event_clone.send(RojoEvent::Output {
                            line,
                            stream: "stdout".into(),
                        });
                    }
                    Ok(None) => break, // EOF
                    Err(_) => break,
                }
            }
        }

        // Process has exited, get the exit code
        let code = {
            let mut guard = child_arc.lock().await;
            if let Some(ref mut child) = *guard {
                child.wait().await.ok().and_then(|s| s.code())
            } else {
                None
            }
        };

        // Clean up
        {
            let mut guard = child_arc.lock().await;
            *guard = None;
        }

        let _ = event_clone.send(RojoEvent::Stopped { code });
    });

    // Also spawn a stderr reader
    let event_stderr = on_event.clone();
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(raw_line)) = lines.next_line().await {
                let line = strip_ansi(&raw_line);
                let _ = event_stderr.send(RojoEvent::Output {
                    line,
                    stream: "stderr".into(),
                });
            }
        });
    }

    // Store the abort handle
    {
        let mut guard = state.abort_handle.lock().await;
        *guard = Some(reader_handle);
    }

    Ok(())
}

/// Stop the running rojo serve process.
#[tauri::command]
pub async fn stop_rojo(state: tauri::State<'_, RojoProcess>) -> Result<()> {
    // Kill the child process
    {
        let mut guard = state.child.lock().await;
        if let Some(ref mut child) = *guard {
            child.kill().await.map_err(|e| {
                InstallerError::Custom(format!("Failed to kill rojo: {e}"))
            })?;
        }
        *guard = None;
    }

    // Abort the reader task
    {
        let mut guard = state.abort_handle.lock().await;
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }

    Ok(())
}

/// Check if rojo is currently running.
#[tauri::command]
pub async fn get_rojo_status(state: tauri::State<'_, RojoProcess>) -> Result<bool> {
    let mut guard = state.child.lock().await;
    if let Some(ref mut child) = *guard {
        // try_wait returns Ok(Some(status)) if exited, Ok(None) if still running
        match child.try_wait() {
            Ok(None) => Ok(true),  // Still running
            _ => {
                *guard = None;
                Ok(false)
            }
        }
    } else {
        Ok(false)
    }
}

/// Strip ANSI escape sequences (e.g. `\x1b[32m`) from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC + '[' + params + final letter
            if let Some(next) = chars.next() {
                if next == '[' {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse the port number from rojo serve output.
/// Rojo prints something like: "Rojo server listening on port 34872"
fn parse_rojo_port(line: &str) -> Option<u16> {
    let lower = line.to_lowercase();
    if lower.contains("listening") || lower.contains("port") {
        // Find a port-like number (4-5 digits)
        for word in line.split_whitespace().rev() {
            // Also handle "localhost:34872" format
            let num_str = if let Some(pos) = word.rfind(':') {
                &word[pos + 1..]
            } else {
                word
            };
            if let Ok(port) = num_str.parse::<u16>() {
                if port >= 1024 {
                    return Some(port);
                }
            }
        }
    }
    None
}

/// Check recursively if a directory contains any .luau files.
fn has_luau_files(dir: &std::path::Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if has_luau_files(&path) {
                    return true;
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("luau") {
                return true;
            }
        }
    }
    false
}

/// Move .luau files from src/ to scripts/, preserving directory structure.
/// Only moves .luau files — .rbxjson files stay in src/ for rbxsync.
fn move_luau_tree(src: &std::path::Path, dest: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                let sub_dest = dest.join(&name);
                let _ = std::fs::create_dir_all(&sub_dest);
                move_luau_tree(&path, &sub_dest);
            } else if path.extension().and_then(|e| e.to_str()) == Some("luau") {
                let dest_file = dest.join(&name);
                // Move = copy + delete (works across filesystems)
                if std::fs::copy(&path, &dest_file).is_ok() {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

/// Regenerate AI context files if they still reference the old `src/` layout for scripts.
/// Reads ~/.roxlit/config.json to find the AI tool for this project.
fn regenerate_ai_context_if_stale(project_dir: &std::path::Path, project_path: &str) {
    // Detect which AI context file exists and check if it's stale
    let context_files = [
        "CLAUDE.md",
        ".cursorrules",
        ".windsurfrules",
        ".github/copilot-instructions.md",
        "AI-CONTEXT.md",
    ];

    let needs_update = context_files.iter().any(|f| {
        let path = project_dir.join(f);
        if let Ok(content) = std::fs::read_to_string(&path) {
            // Old layout had "Write Luau code in `src/`" — new says `scripts/`
            content.contains("Write Luau code in `src/`")
                || content.contains("Edit local files in `src/`. Rojo syncs")
        } else {
            false
        }
    });

    if !needs_update {
        return;
    }

    // Read config to find ai_tool for this project
    let ai_tool = dirs::home_dir()
        .and_then(|h| std::fs::read_to_string(h.join(".roxlit").join("config.json")).ok())
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .and_then(|config| {
            config["projects"]
                .as_array()?
                .iter()
                .find(|p| p["path"].as_str() == Some(project_path))
                .and_then(|p| p["aiTool"].as_str().map(String::from))
        })
        .unwrap_or_else(|| "claude".to_string());

    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-game");

    let _ = crate::commands::context::generate_context(project_path, &ai_tool, project_name);
}

/// Kill orphaned rojo processes from a previous session that may still hold the port.
async fn kill_orphaned_rojo() {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", "rojo.exe"])
            .creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = tokio::process::Command::new("pkill");
        cmd.args(["-f", "rojo serve"]);
        let _ = cmd.output().await;
    }

    // Give the OS time to release the port
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}
