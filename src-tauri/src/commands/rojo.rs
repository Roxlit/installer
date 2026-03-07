use serde::Serialize;
use std::sync::Arc;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

use crate::commands::logs::{send_log, LauncherStatus, LogServerState, LoggerState, SessionLogger};
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
    pub backup_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl Default for RojoProcess {
    fn default() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            abort_handle: Arc::new(Mutex::new(None)),
            backup_handle: Arc::new(Mutex::new(None)),
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
        // Abort the auto-backup timer
        if let Ok(mut guard) = self.backup_handle.try_lock() {
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
    logger_state: tauri::State<'_, LoggerState>,
    log_server_state: tauri::State<'_, LogServerState>,
    launcher_status: tauri::State<'_, LauncherStatus>,
    mcp_state: tauri::State<'_, crate::commands::logs::McpState>,
    telemetry_state: tauri::State<'_, crate::commands::logs::TelemetryState>,
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

    // Clean up legacy rbxsync files (no longer needed)
    let _ = std::fs::remove_file(project_dir.join("rbxsync.json"));
    let _ = std::fs::remove_file(project_dir.join(".rbxsyncignore"));

    // Ensure MCP binary exists (download if missing)
    ensure_mcp_binary().await;

    // Ensure unified Roxlit plugin is installed in Studio
    ensure_roxlit_plugin();

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

    // Extract project name for logger and launcher status
    let project_name = std::path::Path::new(&project_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    // Initialize session logger (creates .roxlit/logs/, rotates previous log)
    let (system_sender, output_sender) = {
        let mut guard = logger_state.logger.lock().await;
        if guard.is_none() {
            *guard = SessionLogger::new(&project_path, project_name).await;
        }
        match guard.as_ref() {
            Some(l) => (Some(l.system_sender()), Some(l.output_sender())),
            None => (None, None),
        }
    };

    // Mark launcher as active so the Studio plugin can auto-connect
    launcher_status.set_active(&project_path, project_name).await;

    // Start the HTTP log server for Studio output capture + /status + MCP relay
    if let (Some(ref sys_tx), Some(ref out_tx)) = (&system_sender, &output_sender) {
        let shared_status = launcher_status.shared();
        let shared_mcp = mcp_state.shared();
        let shared_telemetry = telemetry_state.shared();
        if let Some(handle) = crate::commands::logs::start_log_server(sys_tx.clone(), out_tx.clone(), shared_status, shared_mcp, shared_telemetry).await {
            log_server_state.set_handle(handle).await;
            send_log(sys_tx, "roxlit", "Studio log server started on 127.0.0.1:19556");
        }
    }

    // Kill any orphaned roxlit-mcp/rbxsync process from a previous version that used external binary
    kill_orphaned_roxlit_mcp().await;

    // Auto-open Studio if a placeId is linked to this project
    auto_open_studio(&project_path, system_sender.as_ref()).await;

    // Start rojo serve
    let mut cmd = tokio::process::Command::new(&rojo);
    cmd.arg("serve")
        .current_dir(&project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

    let mut child = cmd.spawn().map_err(|e| {
        InstallerError::Custom(format!("Failed to start rojo: {e}"))
    })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Store the child process
    {
        let mut guard = state.child.lock().await;
        *guard = Some(child);
    }

    let child_arc = state.child.clone();
    let event_clone = on_event.clone();
    let launcher_status_shared = launcher_status.shared();

    // Read stdout and stream events
    let stdout_log_tx = system_sender.clone();
    let reader_handle = tokio::spawn(async move {
        let mut port_detected = false;

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
                        if !port_detected {
                            if let Some(port) = parse_rojo_port(&line) {
                                port_detected = true;
                                // Store the port in launcher status so /status exposes it
                                let mut guard = launcher_status_shared.lock().await;
                                guard.rojo_port = Some(port);
                                drop(guard);
                                let _ = event_clone.send(RojoEvent::Started { port });
                            }
                        }
                        let _ = event_clone.send(RojoEvent::Output {
                            line,
                            stream: "stdout".into(),
                        });
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }

        let code = {
            let mut guard = child_arc.lock().await;
            if let Some(ref mut child) = *guard {
                child.wait().await.ok().and_then(|s| s.code())
            } else {
                None
            }
        };

        {
            let mut guard = child_arc.lock().await;
            *guard = None;
        }

        let _ = event_clone.send(RojoEvent::Stopped { code });
    });

    // Stderr reader
    let event_stderr = on_event;
    let stderr_log_tx = system_sender;
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

    // Store abort handle
    {
        let mut guard = state.abort_handle.lock().await;
        *guard = Some(reader_handle);
    }

    // Start auto-backup timer (every 10 minutes)
    let backup_project_path = project_path.clone();
    let backup_handle = tokio::spawn(async move {
        use crate::commands::backup;

        // Wait 2 minutes before first backup (let user start working)
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;

        let interval = std::time::Duration::from_secs(600); // 10 minutes
        let max_backup_bytes: u64 = 100 * 1024 * 1024; // 100 MB default limit

        loop {
            // Create auto-backup (blocking git ops in spawn_blocking)
            let path = backup_project_path.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let name = format!("auto-{}", backup::now_timestamp());
                match backup::create_backup(&path, &name) {
                    Ok(_) => {
                        // Cleanup old auto-backups if over size limit
                        backup::cleanup_by_size(&path, max_backup_bytes);
                    }
                    Err(_) => {} // No changes or git not available — skip silently
                }
            })
            .await;

            tokio::time::sleep(interval).await;
        }
    });
    {
        let mut guard = state.backup_handle.lock().await;
        *guard = Some(backup_handle);
    }

    Ok(())
}

/// Stop the running rojo serve process.
#[tauri::command]
pub async fn stop_rojo(
    state: tauri::State<'_, RojoProcess>,
    log_server_state: tauri::State<'_, LogServerState>,
    launcher_status: tauri::State<'_, LauncherStatus>,
) -> Result<()> {
    // Persist linked placeId + universeId to config before shutting down
    {
        let shared = launcher_status.shared();
        let guard = shared.lock().await;
        if let Some(place_id) = guard.linked_place_id {
            if !guard.project_path.is_empty() {
                crate::commands::config::save_place_id(
                    &guard.project_path,
                    place_id,
                    guard.linked_universe_id,
                );
            }
        }
    }

    // Mark launcher as inactive so the Studio plugin stops auto-connecting
    launcher_status.set_inactive().await;

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

    // Stop auto-backup timer
    {
        let mut guard = state.backup_handle.lock().await;
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }

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
/// Handles migration from the legacy layout where Rojo used scripts/ to avoid conflicting with the old sync tool's src/.
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

/// Download or update roxlit-mcp binary.
/// Re-downloads when the launcher version changes (version tracked in .roxlit/bin/mcp.version).
async fn ensure_mcp_binary() {
    let mcp_bin_name = if cfg!(target_os = "windows") {
        "roxlit-mcp.exe"
    } else {
        "roxlit-mcp"
    };

    let bin_dir = match dirs::home_dir() {
        Some(h) => h.join(".roxlit").join("bin"),
        None => return,
    };

    let mcp_path = bin_dir.join(mcp_bin_name);
    let version_file = bin_dir.join("mcp.version");
    let current_version = env!("CARGO_PKG_VERSION");

    // Check if binary exists AND version matches
    if mcp_path.exists() {
        if let Ok(stored) = tokio::fs::read_to_string(&version_file).await {
            if stored.trim() == current_version {
                return; // Up to date
            }
        }
        // Version mismatch or no version file — re-download
    }

    // Determine download URL
    let url = if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        "https://github.com/Roxlit/installer/releases/latest/download/roxlit-mcp.exe".to_string()
    } else {
        return; // No MCP for this platform yet
    };

    // Best-effort download — don't block launcher startup if it fails
    let _ = tokio::fs::create_dir_all(&bin_dir).await;
    if let Ok(response) = reqwest::get(&url).await {
        if response.status().is_success() {
            if let Ok(bytes) = response.bytes().await {
                let _ = tokio::fs::write(&mcp_path, &bytes).await;
                // Track which version this binary belongs to
                let _ = tokio::fs::write(&version_file, current_version).await;
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

    // Clean up old rbxsync-mcp binary if it exists
    let old_mcp = bin_dir.join(if cfg!(target_os = "windows") { "rbxsync-mcp.exe" } else { "rbxsync-mcp" });
    let _ = tokio::fs::remove_file(&old_mcp).await;
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
    let mcp_bin_name = if cfg!(target_os = "windows") { "roxlit-mcp.exe" } else { "roxlit-mcp" };
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
            let mcp_missing_from_context = mcp_available && !content.contains("Roxlit MCP server");
            // Also regenerate if still referencing old rbxsync names
            let has_old_rbxsync = content.contains("RbxSync MCP server") || content.contains("rbxsync");
            version_stale || mcp_missing_from_context || has_old_rbxsync
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
        "roxlit-mcp.exe"
    } else {
        "roxlit-mcp"
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
            let content = std::fs::read_to_string(path).unwrap_or_default();
            // Regenerate if: Windows backslash bug OR still references old rbxsync names
            let needs_regen = content.contains(".roxlit\\") || content.contains("rbxsync");
            if !needs_regen {
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

/// Ensure the unified Roxlit Studio plugin is installed.
///
/// Checks if `Roxlit.rbxm` exists in the Studio plugins folder. If not, it was
/// either never installed or was deleted — the installer downloads it during setup,
/// and this function just verifies it's present.
/// Also cleans up old plugins (RoxlitDebug, RbxSync) that the unified plugin replaces.
/// Non-critical — silently ignores errors.
fn ensure_roxlit_plugin() {
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

    // Clean up old plugins that the unified Roxlit plugin replaces
    for old_name in &["RoxlitDebug.rbxm", "RoxlitDebug.rbxmx", "RbxSync.rbxm", "rbxsync.rbxm"] {
        let old_path = plugins_dir.join(old_name);
        if old_path.exists() {
            let _ = std::fs::remove_file(&old_path);
        }
    }
}

/// Kill orphaned roxlit-mcp/rbxsync processes from a previous session that may still hold port 44755.
/// Users upgrading from versions that used the external binary may have a leftover process.
async fn kill_orphaned_roxlit_mcp() {
    #[cfg(target_os = "windows")]
    {
        // Kill old rbxsync processes (legacy)
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", "rbxsync.exe"])
            .creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output().await;

        // Kill roxlit-mcp processes
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", "roxlit-mcp.exe"])
            .creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Kill old rbxsync processes (legacy)
        let mut cmd = tokio::process::Command::new("pkill");
        cmd.args(["-f", "rbxsync serve"]);
        let _ = cmd.output().await;

        // Kill roxlit-mcp processes
        let mut cmd = tokio::process::Command::new("pkill");
        cmd.args(["-f", "roxlit-mcp serve"]);
        let _ = cmd.output().await;
    }

    // Give the OS time to release the port
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
}

/// Auto-open Roblox Studio if the project has a linked placeId
/// and Studio is not already running.
async fn auto_open_studio(project_path: &str, log_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>) {
    // Skip if Studio is already running — the plugin will auto-connect
    if is_studio_running(log_tx).await {
        return;
    }

    // Read the config to find the placeId/universeId for this project
    let config = match crate::commands::config::load_config().await {
        Some(c) => c,
        None => return,
    };

    let project = config.projects.iter().find(|p| p.path == project_path);

    let place_id = project.and_then(|p| p.place_id);
    let universe_id = project.and_then(|p| p.universe_id);

    let place_id = match place_id {
        Some(id) if id > 0 => id,
        _ => {
            if let Some(tx) = log_tx {
                send_log(tx, "roxlit", "No linked placeId — open Studio manually. It will link automatically on first connect.");
            }
            return;
        }
    };

    if let Some(tx) = log_tx {
        send_log(tx, "roxlit", &format!("Opening Studio for placeId {place_id}..."));
    }

    open_studio_url(place_id, universe_id.unwrap_or(0)).await;
}

/// Check if Roblox Studio is already running.
async fn is_studio_running(log_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>) -> bool {
    #[cfg(target_os = "windows")]
    {
        // Check both possible process names
        for process_name in ["RobloxStudioBeta.exe", "RobloxStudioDesktop.exe"] {
            let output = tokio::process::Command::new("tasklist")
                .args(["/FI", &format!("IMAGENAME eq {process_name}"), "/NH"])
                .creation_flags(0x08000000)
                .output()
                .await;
            if let Ok(out) = output {
                let text = String::from_utf8_lossy(&out.stdout);
                if text.contains(process_name) {
                    if let Some(tx) = log_tx {
                        send_log(tx, "roxlit", &format!("Studio already running ({process_name}), skipping auto-open"));
                    }
                    return true;
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("pgrep")
            .arg("-x")
            .arg("RobloxStudio")
            .output()
            .await;
        if let Ok(out) = output {
            if out.status.success() {
                if let Some(tx) = log_tx {
                    send_log(tx, "roxlit", "Studio already running, skipping auto-open");
                }
                return true;
            }
        }
    }
    false
}

/// Open Roblox Studio for a specific place via the roblox-studio: protocol.
/// Uses PowerShell on Windows because cmd.exe and rundll32 split URLs at `+` delimiters.
#[allow(unused_variables)]
async fn open_studio_url(place_id: u64, universe_id: u64) {
    let url = format!(
        "roblox-studio:1+launchmode:edit+task:EditPlace+placeId:{place_id}+universeId:{universe_id}"
    );

    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-Command", &format!("Start-Process '{url}'")]);
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output().await;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = tokio::process::Command::new("open")
            .arg(&url)
            .output()
            .await;
    }
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
