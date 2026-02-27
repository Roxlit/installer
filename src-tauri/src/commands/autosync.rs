use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::commands::logs::{send_log, LoggerState};
use crate::commands::rbxsync::rbxsync_bin_path;
use crate::error::{InstallerError, Result};
use crate::util::expand_tilde;

// ── Events ──────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum SyncEvent {
    ExtractStarted,
    #[serde(rename_all = "camelCase")]
    ExtractCompleted { backup_path: String },
    Error { message: String },
}

// ── State ───────────────────────────────────────────────────────────────────

pub struct AutoSyncState {
    poller_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    active: Arc<AtomicBool>,
}

impl Default for AutoSyncState {
    fn default() -> Self {
        Self {
            poller_handle: Arc::new(Mutex::new(None)),
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl AutoSyncState {
    pub fn kill_sync(&self) {
        self.active.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.poller_handle.try_lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn collect_rbxjson_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_rbxjson_files(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("rbxjson") {
                files.push(path);
            }
        }
    }
    files
}

fn is_luau(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("luau")
}

fn collect_luau_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_luau_files(&path));
            } else if is_luau(&path) {
                files.push(path);
            }
        }
    }
    files
}

/// Snapshot all .luau file contents before extract so we can restore them after.
pub(crate) fn snapshot_luau_files(project_path: &str) -> HashMap<PathBuf, Vec<u8>> {
    let scripts_dir = Path::new(project_path).join("src");
    let files = collect_luau_files(&scripts_dir);
    let mut snapshot = HashMap::new();
    for file in files {
        if let Ok(content) = std::fs::read(&file) {
            snapshot.insert(file, content);
        }
    }
    snapshot
}

/// Restore .luau files that were overwritten by rbxsync extract.
/// Returns the number of files restored.
pub(crate) fn restore_luau_files(snapshot: &HashMap<PathBuf, Vec<u8>>) -> u32 {
    let mut restored = 0;
    for (path, original_content) in snapshot {
        if let Ok(current_content) = std::fs::read(path) {
            if current_content != *original_content {
                if std::fs::write(path, original_content).is_ok() {
                    restored += 1;
                }
            }
        }
    }
    restored
}

/// Run `rbxsync extract` while protecting .luau files from being overwritten.
/// Rojo owns .luau scripts — rbxsync should only update .rbxjson files.
async fn run_extract_command(project_path: &str) -> std::result::Result<String, String> {
    // Snapshot .luau files before extract
    let luau_snapshot = snapshot_luau_files(project_path);

    let rbxsync = rbxsync_bin_path();
    let mut cmd = tokio::process::Command::new(&rbxsync);
    cmd.arg("extract")
        .current_dir(project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run rbxsync extract: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Restore any .luau files that rbxsync overwrote
    let restored = restore_luau_files(&luau_snapshot);

    if output.status.success() {
        let mut result = format!("{}{}", stdout, stderr);
        if restored > 0 {
            result.push_str(&format!(
                "\n(Roxlit: restored {} .luau file(s) — Rojo owns scripts)",
                restored
            ));
        }
        Ok(result)
    } else {
        // Still restore .luau even if extract failed partially
        Err(format!("rbxsync extract failed: {}{}", stdout, stderr))
    }
}

fn create_backup(project_path: &str) -> std::result::Result<String, String> {
    let src_dir = Path::new(project_path).join("src");
    if !src_dir.exists() {
        return Ok(String::new());
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let backup_dir = Path::new(project_path)
        .join(".roxlit")
        .join("backups")
        .join(timestamp.to_string());

    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("Failed to create backup dir: {e}"))?;

    let files = collect_rbxjson_files(&src_dir);
    let mut manifest_files = Vec::new();

    for file in &files {
        if let Ok(rel) = file.strip_prefix(&src_dir) {
            let dest = backup_dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            if std::fs::copy(file, &dest).is_ok() {
                manifest_files.push(rel.to_string_lossy().to_string());
            }
        }
    }

    let manifest = serde_json::json!({
        "timestamp": timestamp,
        "files": manifest_files,
    });
    let manifest_path = backup_dir.join("manifest.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )
    .map_err(|e| format!("Failed to write manifest: {e}"))?;

    Ok(backup_dir.to_string_lossy().to_string())
}

fn cleanup_old_backups(project_path: &str, max_backups: usize) {
    let backups_dir = Path::new(project_path).join(".roxlit").join("backups");
    if !backups_dir.exists() {
        return;
    }

    let mut entries: Vec<_> = std::fs::read_dir(&backups_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();

    entries.sort_by_key(|e| e.file_name());

    while entries.len() > max_backups {
        if let Some(oldest) = entries.first() {
            let _ = std::fs::remove_dir_all(oldest.path());
            entries.remove(0);
        }
    }
}

// ── Commands ────────────────────────────────────────────────────────────────

/// Starts the Studio extract poller. Periodically pulls Studio state to local
/// .rbxjson files for backup/exploration. Local→Studio sync is intentionally
/// disabled — instance editing goes through MCP or Studio directly.
#[tauri::command]
pub async fn start_auto_sync(
    project_path: String,
    extract_interval_secs: Option<u64>,
    on_event: Channel<SyncEvent>,
    state: tauri::State<'_, AutoSyncState>,
    logger_state: tauri::State<'_, LoggerState>,
) -> Result<()> {
    if state.active.load(Ordering::SeqCst) {
        return Err(InstallerError::Custom(
            "Auto-sync is already running".into(),
        ));
    }

    state.active.store(true, Ordering::SeqCst);
    let interval = extract_interval_secs.unwrap_or(30);
    let project_path = expand_tilde(&project_path);

    // Extract log sender (logger was initialized by rojo)
    let log_sender = {
        let guard = logger_state.logger.lock().await;
        guard.as_ref().map(|l| l.sender())
    };

    // ── Studio poller task (Studio → FS, backup only) ──
    let poller_event = on_event.clone();
    let poller_active = state.active.clone();
    let poller_project = project_path.clone();

    let poller_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;

            if !poller_active.load(Ordering::SeqCst) {
                break;
            }

            // Create backup before extract
            let backup_path = match create_backup(&poller_project) {
                Ok(p) if p.is_empty() => continue,
                Ok(p) => p,
                Err(e) => {
                    if let Some(ref tx) = log_sender {
                        send_log(tx, "sync", &format!("Backup failed: {e}"));
                    }
                    let _ = poller_event.send(SyncEvent::Error {
                        message: format!("Backup failed: {e}"),
                    });
                    continue;
                }
            };

            if let Some(ref tx) = log_sender {
                send_log(tx, "sync", "Extracting instances from Studio...");
            }
            let _ = poller_event.send(SyncEvent::ExtractStarted);

            match run_extract_command(&poller_project).await {
                Ok(_) => {
                    if let Some(ref tx) = log_sender {
                        send_log(tx, "sync", "Extract complete");
                    }
                    let _ = poller_event.send(SyncEvent::ExtractCompleted {
                        backup_path: backup_path.clone(),
                    });
                }
                Err(e) => {
                    if let Some(ref tx) = log_sender {
                        send_log(tx, "sync", &format!("Extract error: {e}"));
                    }
                    let _ = poller_event.send(SyncEvent::Error { message: e });
                }
            }

            cleanup_old_backups(&poller_project, 20);
        }
    });

    *state.poller_handle.lock().await = Some(poller_handle);

    Ok(())
}

#[tauri::command]
pub async fn stop_auto_sync(state: tauri::State<'_, AutoSyncState>) -> Result<()> {
    state.active.store(false, Ordering::SeqCst);

    if let Some(handle) = state.poller_handle.lock().await.take() {
        handle.abort();
    }

    Ok(())
}

#[tauri::command]
pub async fn get_auto_sync_status(state: tauri::State<'_, AutoSyncState>) -> Result<bool> {
    Ok(state.active.load(Ordering::SeqCst))
}

#[tauri::command]
pub async fn trigger_extract_now(
    project_path: String,
    _state: tauri::State<'_, AutoSyncState>,
) -> Result<String> {
    let project_path = expand_tilde(&project_path);

    create_backup(&project_path).map_err(InstallerError::Custom)?;
    cleanup_old_backups(&project_path, 20);

    run_extract_command(&project_path)
        .await
        .map_err(InstallerError::Custom)
}
