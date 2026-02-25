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

    // Ensure project directories exist (user may have deleted src/)
    let project_dir = std::path::Path::new(&project_path);
    for subdir in &["src/ServerScriptService", "src/StarterPlayer/StarterPlayerScripts", "src/ReplicatedStorage"] {
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
