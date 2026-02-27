use serde::Serialize;
use std::sync::Arc;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

use crate::commands::logs::{send_log, LogServerState, LoggerState, SessionLogger};
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
    #[allow(dead_code)]
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

/// Managed state holding the background rbxsync serve process (needed for MCP).
///
/// This runs silently — no UI, no events. It just needs to be alive on port 44755
/// so that rbxsync-mcp can send commands to Studio via the rbxsync plugin.
pub struct RbxSyncProcess {
    child: Arc<Mutex<Option<tokio::process::Child>>>,
}

impl Default for RbxSyncProcess {
    fn default() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
        }
    }
}

impl RbxSyncProcess {
    /// Kill the process synchronously (for window close handler).
    pub fn kill_sync(&self) {
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(ref mut child) = *guard {
                let _ = child.start_kill();
            }
            *guard = None;
        }
    }

    /// Start rbxsync serve silently in the background.
    async fn start(&self, project_path: &str, log_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>) {
        // Already running?
        {
            let mut guard = self.child.lock().await;
            if let Some(ref mut child) = *guard {
                if child.try_wait().ok().flatten().is_none() {
                    return; // Still running
                }
            }
            *guard = None;
        }

        let bin = rbxsync_bin_path();
        if bin.is_none() {
            if let Some(tx) = log_tx {
                send_log(tx, "roxlit", "rbxsync binary not found — MCP tools unavailable");
            }
            return;
        }
        let bin = bin.unwrap();

        let mut cmd = tokio::process::Command::new(&bin);
        cmd.arg("serve")
            .current_dir(project_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        match cmd.spawn() {
            Ok(mut child) => {
                // Pipe stdout/stderr to session log (silent, no frontend events)
                if let Some(tx) = log_tx {
                    if let Some(stdout) = child.stdout.take() {
                        let tx_out = tx.clone();
                        tokio::spawn(async move {
                            let reader = BufReader::new(stdout);
                            let mut lines = reader.lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                send_log(&tx_out, "rbxsync", &strip_ansi(&line));
                            }
                        });
                    }
                    if let Some(stderr) = child.stderr.take() {
                        let tx_err = tx.clone();
                        tokio::spawn(async move {
                            let reader = BufReader::new(stderr);
                            let mut lines = reader.lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                send_log(&tx_err, "rbxsync-err", &strip_ansi(&line));
                            }
                        });
                    }
                    send_log(tx, "roxlit", "rbxsync serve started (MCP bridge)");
                }

                let mut guard = self.child.lock().await;
                *guard = Some(child);
            }
            Err(e) => {
                if let Some(tx) = log_tx {
                    send_log(tx, "roxlit", &format!("Failed to start rbxsync serve: {e}"));
                }
            }
        }
    }

    /// Stop the rbxsync serve process.
    async fn stop(&self) {
        let mut guard = self.child.lock().await;
        if let Some(ref mut child) = *guard {
            let _ = child.kill().await;
        }
        *guard = None;
    }
}

/// Resolve the rbxsync binary path (~/.roxlit/bin/rbxsync).
fn rbxsync_bin_path() -> Option<String> {
    let bin_name = if cfg!(target_os = "windows") { "rbxsync.exe" } else { "rbxsync" };
    let path = dirs::home_dir()?.join(".roxlit").join("bin").join(bin_name);
    if path.exists() {
        Some(path.to_string_lossy().to_string())
    } else {
        None
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
    rbxsync_state: tauri::State<'_, RbxSyncProcess>,
    logger_state: tauri::State<'_, LoggerState>,
    log_server_state: tauri::State<'_, LogServerState>,
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

    // Migrate legacy projects: move files from scripts/ to src/
    let src_dir = project_dir.join("src");
    let legacy_scripts = project_dir.join("scripts");
    if legacy_scripts.exists() && has_luau_files(&legacy_scripts) {
        let _ = std::fs::create_dir_all(&src_dir);
        move_luau_tree(&legacy_scripts, &src_dir);
    }

    let project_json = project_dir.join("default.project.json");
    // Rewrite project.json if it still references scripts/ (old layout)
    if project_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&project_json) {
            if content.contains("\"scripts/ServerScriptService\"")
                || content.contains("\"scripts/StarterPlayer")
                || content.contains("\"scripts/ReplicatedStorage\"")
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

    // Ensure .luaurc exists
    let luaurc = project_dir.join(".luaurc");
    if !luaurc.exists() {
        let _ = std::fs::write(&luaurc, crate::templates::luaurc());
    }

    // Ensure rbxsync.json exists
    let rbxsync_json = project_dir.join("rbxsync.json");
    if !rbxsync_json.exists() {
        let name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-game");
        let _ = std::fs::write(&rbxsync_json, crate::templates::rbxsync_json(name));
    }

    // Ensure .rbxsyncignore exists and includes src/
    let rbxsyncignore = project_dir.join(".rbxsyncignore");
    if rbxsyncignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&rbxsyncignore) {
            if !content.contains("src/") {
                let _ = std::fs::write(
                    &rbxsyncignore,
                    format!("{}src/\n", content),
                );
            }
        }
    } else {
        let _ = std::fs::write(
            &rbxsyncignore,
            ".git/\n.roxlit/\n.claude/\n.cursor/\n.vscode/\n.windsurf/\n.github/\nnode_modules/\nsrc/\n",
        );
    }

    // Ensure MCP binary exists (download if missing)
    ensure_mcp_binary().await;

    // Ensure debug plugin is installed in Studio
    ensure_debug_plugin();

    // Ensure AI context file exists (or regenerate if stale)
    ensure_ai_context(project_dir, &project_path);

    // Ensure Debug.luau exists (added in v0.7.0, older projects don't have it)
    ensure_debug_module(project_dir);

    // Ensure project directories exist (user may have deleted src/)
    for subdir in &[
        "src/ServerScriptService",
        "src/StarterPlayer/StarterPlayerScripts",
        "src/StarterPlayer/StarterCharacterScripts",
        "src/ReplicatedStorage",
        "src/ReplicatedFirst",
        "src/ServerStorage",
        "src/Workspace",
        "src/StarterGui",
        "src/StarterPack",
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

    // Initialize session logger (creates .roxlit/logs/, rotates previous log)
    let log_sender = {
        let mut guard = logger_state.logger.lock().await;
        if guard.is_none() {
            *guard = SessionLogger::new(&project_path).await;
        }
        guard.as_ref().map(|l| l.sender())
    };

    // Start the HTTP log server for Studio output capture
    if let Some(ref tx) = log_sender {
        if let Some(handle) = crate::commands::logs::start_log_server(tx.clone()).await {
            log_server_state.set_handle(handle).await;
            send_log(tx, "roxlit", "Studio log server started on 127.0.0.1:19556");
        }
    }

    // Start rbxsync serve silently (needed for MCP tools: run_code, run_test, etc.)
    rbxsync_state.start(&project_path, log_sender.as_ref()).await;

    let child_arc = state.child.clone();
    let event_clone = on_event.clone();

    // Spawn a task to read stdout + stderr and stream events
    let stdout_log_tx = log_sender.clone();
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
                        if let Some(ref tx) = stdout_log_tx {
                            send_log(tx, "rojo", &line);
                        }
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
    let stderr_log_tx = log_sender;
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(raw_line)) = lines.next_line().await {
                let line = strip_ansi(&raw_line);
                if let Some(ref tx) = stderr_log_tx {
                    send_log(tx, "rojo-err", &line);
                }
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
pub async fn stop_rojo(
    state: tauri::State<'_, RojoProcess>,
    rbxsync_state: tauri::State<'_, RbxSyncProcess>,
    log_server_state: tauri::State<'_, LogServerState>,
) -> Result<()> {
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

    // Stop rbxsync serve
    rbxsync_state.stop().await;

    // Stop the Studio log HTTP server
    log_server_state.stop().await;

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

/// Move .luau and .model.json files from scripts/ to src/, preserving directory structure.
/// Handles migration from the legacy layout where Rojo used scripts/ to avoid conflicting with rbxsync's src/.
fn move_luau_tree(src: &std::path::Path, dest: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                let sub_dest = dest.join(&name);
                let _ = std::fs::create_dir_all(&sub_dest);
                move_luau_tree(&path, &sub_dest);
            } else {
                let ext = path.extension().and_then(|e| e.to_str());
                let is_model_json = path.to_str().map_or(false, |s| s.ends_with(".model.json"));
                if ext == Some("luau") || is_model_json {
                    let dest_file = dest.join(&name);
                    // Move = copy + delete (works across filesystems)
                    if std::fs::copy(&path, &dest_file).is_ok() {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }
}

/// Download rbxsync-mcp binary if it doesn't exist yet.
/// This handles upgrades from versions that didn't include MCP.
async fn ensure_mcp_binary() {
    let mcp_bin_name = if cfg!(target_os = "windows") {
        "rbxsync-mcp.exe"
    } else {
        "rbxsync-mcp"
    };

    let bin_dir = match dirs::home_dir() {
        Some(h) => h.join(".roxlit").join("bin"),
        None => return,
    };

    let mcp_path = bin_dir.join(mcp_bin_name);
    if mcp_path.exists() {
        return; // Already downloaded
    }

    // Determine download URL
    let url = if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "https://github.com/Smokestack-Games/rbxsync/releases/download/v1.3.0/rbxsync-mcp-macos-arm64".to_string()
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        "https://github.com/Roxlit/installer/releases/latest/download/rbxsync-mcp.exe".to_string()
    } else {
        return; // No MCP for this platform
    };

    // Best-effort download — don't block launcher startup if it fails
    let _ = tokio::fs::create_dir_all(&bin_dir).await;
    if let Ok(response) = reqwest::get(&url).await {
        if response.status().is_success() {
            if let Ok(bytes) = response.bytes().await {
                let _ = tokio::fs::write(&mcp_path, &bytes).await;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tokio::fs::set_permissions(
                        &mcp_path,
                        std::fs::Permissions::from_mode(0o755),
                    )
                    .await;
                }
            }
        }
    }
}

/// Ensure AI context files exist and are up to date.
///
/// Checks for a version marker in the existing context file. If the marker is missing
/// (pre-versioning file) or the version is older than the current CONTEXT_VERSION,
/// the file is regenerated. User notes (everything after "## Your Notes") are preserved.
/// Also ensures MCP config exists if the MCP binary is available.
fn ensure_ai_context(project_dir: &std::path::Path, project_path: &str) {
    use crate::templates;

    let context_files = [
        "CLAUDE.md",
        ".cursorrules",
        ".windsurfrules",
        ".github/copilot-instructions.md",
        "AI-CONTEXT.md",
    ];

    // Find the existing context file (if any)
    let existing_file = context_files
        .iter()
        .map(|f| project_dir.join(f))
        .find(|p| p.exists());

    // Check if MCP binary is available (for context variant detection)
    let mcp_bin_name = if cfg!(target_os = "windows") { "rbxsync-mcp.exe" } else { "rbxsync-mcp" };
    let mcp_available = dirs::home_dir()
        .map(|h| h.join(".roxlit").join("bin").join(mcp_bin_name).exists())
        .unwrap_or(false);

    // Check if regeneration is needed
    let needs_regen = match &existing_file {
        None => true, // No context file at all
        Some(path) => {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            // Extract version from marker: <!-- roxlit-context-version: X.Y.Z -->
            let file_version = content
                .lines()
                .find(|line| line.contains("roxlit-context-version:"))
                .and_then(|line| {
                    let start = line.find(':')? + 1;
                    let end = line.find("-->")?;
                    Some(line[start..end].trim())
                });
            let version_stale = match file_version {
                None => true, // No version marker → pre-versioning file, always regenerate
                Some(v) => v != templates::CONTEXT_VERSION,
            };
            // Also regenerate if MCP is now available but context was generated without it
            let mcp_missing_from_context = mcp_available && !content.contains("RbxSync MCP server");
            version_stale || mcp_missing_from_context
        }
    };

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

    // Always ensure MCP config exists if binary is available (even if CLAUDE.md is up to date)
    ensure_mcp_config(project_dir, &ai_tool);

    if !needs_regen {
        return;
    }

    // Extract user notes from existing file before regenerating
    let user_notes = existing_file.as_ref().and_then(|path| {
        let content = std::fs::read_to_string(path).ok()?;
        let marker = templates::USER_NOTES_MARKER;
        let marker_pos = content.find(marker)?;
        Some(content[marker_pos..].to_string())
    });

    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-game");

    // Generate new context (this also writes context packs and MCP config)
    let _ = crate::commands::context::generate_context(project_path, &ai_tool, project_name);

    // If user had custom notes, append them back to the regenerated file
    if let (Some(notes), Some(path)) = (user_notes, &existing_file) {
        if let Ok(new_content) = std::fs::read_to_string(path) {
            // Replace the default "Your Notes" section with the user's saved notes
            if let Some(marker_pos) = new_content.find(templates::USER_NOTES_MARKER) {
                let mut final_content = new_content[..marker_pos].to_string();
                final_content.push_str(&notes);
                let _ = std::fs::write(path, final_content);
            }
        }
    }
}

/// Ensure MCP config file exists if the MCP binary is available.
/// This handles the case where a user upgrades Roxlit and gets MCP for the first time.
fn ensure_mcp_config(project_dir: &std::path::Path, ai_tool: &str) {
    let mcp_bin_name = if cfg!(target_os = "windows") {
        "rbxsync-mcp.exe"
    } else {
        "rbxsync-mcp"
    };

    let mcp_available = dirs::home_dir()
        .map(|h| h.join(".roxlit").join("bin").join(mcp_bin_name).exists())
        .unwrap_or(false);

    if !mcp_available {
        return;
    }

    // Check if MCP config already exists for this AI tool
    let config_path = match ai_tool {
        "claude" => Some(project_dir.join(".mcp.json")),
        "cursor" => Some(project_dir.join(".cursor").join("mcp.json")),
        "vscode" => Some(project_dir.join(".vscode").join("mcp.json")),
        "windsurf" => dirs::home_dir()
            .map(|h| h.join(".codeium").join("windsurf").join("mcp_config.json")),
        _ => Some(project_dir.join(".mcp.json")),
    };

    if let Some(ref path) = config_path {
        if path.exists() {
            // Regenerate if the existing config has Windows backslashes in paths
            // (bug: \b = backspace and \r = carriage return in JSON, corrupts the file).
            let has_backslash_bug = std::fs::read_to_string(path)
                .map(|c| c.contains(".roxlit\\"))
                .unwrap_or(false);
            if !has_backslash_bug {
                return;
            }
        }
    } else {
        return;
    }

    // Create MCP config
    let _ = crate::commands::context::configure_mcp(project_dir, ai_tool);
}

/// Ensure the Debug.luau module exists in the project.
///
/// Added in v0.7.0 — older projects don't have it. The AI context references
/// `require(game.ReplicatedStorage.Debug)`, so the file must exist.
fn ensure_debug_module(project_dir: &std::path::Path) {
    let debug_path = project_dir
        .join("src")
        .join("ReplicatedStorage")
        .join("Debug.luau");
    if !debug_path.exists() {
        let _ = std::fs::create_dir_all(debug_path.parent().unwrap());
        let _ = std::fs::write(&debug_path, crate::templates::debug_module());
    }
}

/// Ensure the RoxlitDebug Studio plugin is installed and up to date.
///
/// Writes `RoxlitDebug.rbxm` (binary format) to the Studio local plugins folder.
/// Always overwrites — the file is small and version checking binary content is complex.
/// Also cleans up the old `.rbxmx` file if it exists.
/// Non-critical — silently ignores errors.
fn ensure_debug_plugin() {
    let plugins_dir = if cfg!(target_os = "windows") {
        dirs::data_local_dir().map(|d| d.join("Roblox").join("Plugins"))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|d| d.join("Library").join("Roblox").join("Plugins"))
    } else {
        None
    };

    let plugins_dir = match plugins_dir {
        Some(d) => d,
        None => return,
    };

    let _ = std::fs::create_dir_all(&plugins_dir);

    // Clean up old .rbxmx version (Studio doesn't load XML plugins)
    let old_xml = plugins_dir.join("RoxlitDebug.rbxmx");
    if old_xml.exists() {
        let _ = std::fs::remove_file(&old_xml);
    }

    // Write the binary .rbxm plugin
    let plugin_path = plugins_dir.join("RoxlitDebug.rbxm");
    let _ = std::fs::write(&plugin_path, crate::templates::debug_plugin_rbxm());
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
