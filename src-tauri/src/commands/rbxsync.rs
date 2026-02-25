use serde::Serialize;
use std::sync::Arc;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::error::{InstallerError, Result};
use crate::util::expand_tilde;

/// Events streamed from the rbxsync serve process to the frontend.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum RbxSyncEvent {
    #[serde(rename_all = "camelCase")]
    Output { line: String, stream: String },
    Started,
    Stopped { code: Option<i32> },
    Error { message: String },
}

/// Managed state holding the rbxsync child process.
pub struct RbxSyncProcess {
    pub child: Arc<Mutex<Option<tokio::process::Child>>>,
    pub abort_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl Default for RbxSyncProcess {
    fn default() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            abort_handle: Arc::new(Mutex::new(None)),
        }
    }
}

impl RbxSyncProcess {
    /// Kill the rbxsync process synchronously (for window close handler).
    pub fn kill_sync(&self) {
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(ref mut child) = *guard {
                let _ = child.start_kill();
            }
            *guard = None;
        }
        if let Ok(mut guard) = self.abort_handle.try_lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }
}

/// Resolve the rbxsync binary path (~/.roxlit/bin/rbxsync or PATH).
fn rbxsync_bin_path() -> String {
    if let Some(home) = dirs::home_dir() {
        let bin = if cfg!(target_os = "windows") {
            home.join(".roxlit").join("bin").join("rbxsync.exe")
        } else {
            home.join(".roxlit").join("bin").join("rbxsync")
        };
        if bin.exists() {
            return bin.to_string_lossy().to_string();
        }
    }
    // Fallback to PATH
    "rbxsync".to_string()
}

/// Start `rbxsync serve` in the given project directory and stream output.
#[tauri::command]
pub async fn start_rbxsync(
    project_path: String,
    on_event: Channel<RbxSyncEvent>,
    state: tauri::State<'_, RbxSyncProcess>,
) -> Result<()> {
    // Check if already running
    {
        let guard = state.child.lock().await;
        if guard.is_some() {
            return Err(InstallerError::Custom(
                "RbxSync is already running".into(),
            ));
        }
    }

    let rbxsync = rbxsync_bin_path();
    let project_path = expand_tilde(&project_path);

    // Kill any orphaned rbxsync process holding the port from a previous session
    kill_orphaned_rbxsync().await;

    let mut cmd = tokio::process::Command::new(&rbxsync);
    cmd.arg("serve")
        .current_dir(&project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let mut child = cmd
        .spawn()
        .map_err(|e| InstallerError::Custom(format!("Failed to start rbxsync: {e}")))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Store the child process
    {
        let mut guard = state.child.lock().await;
        *guard = Some(child);
    }

    let child_arc = state.child.clone();
    let event_clone = on_event.clone();

    // Spawn a task to read stdout and stream events
    let reader_handle = tokio::spawn(async move {
        let mut started_sent = false;

        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(raw_line)) => {
                        let line = strip_ansi(&raw_line);
                        // Detect when rbxsync is ready
                        if !started_sent {
                            let lower = line.to_lowercase();
                            if lower.contains("listening") || lower.contains("started") || lower.contains("ready") {
                                started_sent = true;
                                let _ = event_clone.send(RbxSyncEvent::Started);
                            }
                        }
                        let _ = event_clone.send(RbxSyncEvent::Output {
                            line,
                            stream: "stdout".into(),
                        });
                    }
                    Ok(None) => break,
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

        let _ = event_clone.send(RbxSyncEvent::Stopped { code });
    });

    // Also spawn a stderr reader
    let event_stderr = on_event.clone();
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(raw_line)) = lines.next_line().await {
                let line = strip_ansi(&raw_line);
                let _ = event_stderr.send(RbxSyncEvent::Output {
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

/// Stop the running rbxsync serve process.
#[tauri::command]
pub async fn stop_rbxsync(state: tauri::State<'_, RbxSyncProcess>) -> Result<()> {
    {
        let mut guard = state.child.lock().await;
        if let Some(ref mut child) = *guard {
            child.kill().await.map_err(|e| {
                InstallerError::Custom(format!("Failed to kill rbxsync: {e}"))
            })?;
        }
        *guard = None;
    }

    {
        let mut guard = state.abort_handle.lock().await;
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }

    Ok(())
}

/// Check if rbxsync is currently running.
#[tauri::command]
pub async fn get_rbxsync_status(state: tauri::State<'_, RbxSyncProcess>) -> Result<bool> {
    let mut guard = state.child.lock().await;
    if let Some(ref mut child) = *guard {
        match child.try_wait() {
            Ok(None) => Ok(true),
            _ => {
                *guard = None;
                Ok(false)
            }
        }
    } else {
        Ok(false)
    }
}

/// Run `rbxsync extract` to do a full DataModel extraction.
/// Should be called after Studio connects to get the initial state of all instances.
#[tauri::command]
pub async fn extract_rbxsync(project_path: String) -> Result<String> {
    let rbxsync = rbxsync_bin_path();
    let project_path = expand_tilde(&project_path);
    let mut cmd = tokio::process::Command::new(&rbxsync);
    cmd.arg("extract")
        .current_dir(&project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = cmd
        .output()
        .await
        .map_err(|e| InstallerError::Custom(format!("Failed to run rbxsync extract: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(format!("{}{}", stdout, stderr))
    } else {
        Err(InstallerError::Custom(format!(
            "rbxsync extract failed: {}{}",
            stdout, stderr
        )))
    }
}

/// Kill orphaned rbxsync processes from a previous session that may still hold the port.
async fn kill_orphaned_rbxsync() {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", "rbxsync.exe"])
            .creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = tokio::process::Command::new("pkill");
        cmd.args(["-f", "rbxsync serve"]);
        let _ = cmd.output().await;
    }

    // Give the OS time to release the port
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
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
