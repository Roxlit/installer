use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{Event, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::commands::rbxsync::rbxsync_bin_path;
use crate::error::{InstallerError, Result};
use crate::util::expand_tilde;

// ── Events ──────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum SyncEvent {
    #[serde(rename_all = "camelCase")]
    FileChanged { path: String },
    SyncStarted,
    #[serde(rename_all = "camelCase")]
    SyncCompleted { files_synced: u32 },
    ExtractStarted,
    #[serde(rename_all = "camelCase")]
    ExtractCompleted { backup_path: String },
    #[serde(rename_all = "camelCase")]
    Conflict { path: String, backup_path: String },
    Error { message: String },
}

// ── State ───────────────────────────────────────────────────────────────────

pub struct AutoSyncState {
    watcher_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    poller_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    active: Arc<AtomicBool>,
    sync_lock: Arc<tokio::sync::Mutex<()>>,
    last_sync_time: Arc<Mutex<Option<SystemTime>>>,
}

impl Default for AutoSyncState {
    fn default() -> Self {
        Self {
            watcher_handle: Arc::new(Mutex::new(None)),
            poller_handle: Arc::new(Mutex::new(None)),
            active: Arc::new(AtomicBool::new(false)),
            sync_lock: Arc::new(tokio::sync::Mutex::new(())),
            last_sync_time: Arc::new(Mutex::new(None)),
        }
    }
}

impl AutoSyncState {
    pub fn kill_sync(&self) {
        self.active.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.watcher_handle.try_lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
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

fn is_ignored(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    path_str.contains(".roxlit")
        || path_str.contains(".git")
        || path_str.contains("node_modules")
}

fn is_rbxjson(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("rbxjson")
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
    let src_dir = Path::new(project_path).join("src");
    let files = collect_luau_files(&src_dir);
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

fn accumulate_paths(event: &Event, pending: &mut Vec<String>) {
    for path in &event.paths {
        if is_rbxjson(path) && !is_ignored(path) {
            let ps = path.to_string_lossy().to_string();
            if !pending.contains(&ps) {
                pending.push(ps);
            }
        }
    }
}

async fn run_sync_command(project_path: &str) -> std::result::Result<String, String> {
    let rbxsync = rbxsync_bin_path();
    let mut cmd = tokio::process::Command::new(&rbxsync);
    cmd.arg("sync")
        .current_dir(project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run rbxsync sync: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(format!("{}{}", stdout, stderr))
    } else {
        Err(format!("rbxsync sync failed: {}{}", stdout, stderr))
    }
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
    let src_dir = Path::new(project_path).join("instances");
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

fn snapshot_mtimes(project_path: &str) -> HashMap<PathBuf, SystemTime> {
    let src_dir = Path::new(project_path).join("instances");
    let files = collect_rbxjson_files(&src_dir);
    let mut mtimes = HashMap::new();
    for file in files {
        if let Ok(meta) = std::fs::metadata(&file) {
            if let Ok(mtime) = meta.modified() {
                mtimes.insert(file, mtime);
            }
        }
    }
    mtimes
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_auto_sync(
    project_path: String,
    extract_interval_secs: Option<u64>,
    on_event: Channel<SyncEvent>,
    state: tauri::State<'_, AutoSyncState>,
) -> Result<()> {
    if state.active.load(Ordering::SeqCst) {
        return Err(InstallerError::Custom(
            "Auto-sync is already running".into(),
        ));
    }

    state.active.store(true, Ordering::SeqCst);
    let interval = extract_interval_secs.unwrap_or(30);
    let project_path = expand_tilde(&project_path);

    // ── File watcher task (FS → Studio) ──
    let watcher_event = on_event.clone();
    let watcher_active = state.active.clone();
    let watcher_lock = state.sync_lock.clone();
    let watcher_last_sync = state.last_sync_time.clone();
    let watcher_project = project_path.clone();

    let watcher_handle = tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let watcher_result = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let _ = tx.send(res);
        });

        let mut watcher = match watcher_result {
            Ok(w) => w,
            Err(e) => {
                let _ = watcher_event.send(SyncEvent::Error {
                    message: format!("Failed to create file watcher: {e}"),
                });
                return;
            }
        };

        let instances_path = PathBuf::from(&watcher_project).join("instances");
        if instances_path.exists() {
            if let Err(e) = watcher.watch(&instances_path, RecursiveMode::Recursive) {
                let _ = watcher_event.send(SyncEvent::Error {
                    message: format!("Failed to watch instances/: {e}"),
                });
                return;
            }
        } else {
            let _ = watcher_event.send(SyncEvent::Error {
                message: "instances/ directory not found — file watcher inactive".into(),
            });
        }

        // Keep watcher alive for the duration of this task
        let _watcher = watcher;

        loop {
            if !watcher_active.load(Ordering::SeqCst) {
                break;
            }

            // Wait for first .rbxjson change
            let first = loop {
                match rx.recv().await {
                    Some(Ok(event)) => {
                        let dominated: Vec<_> = event
                            .paths
                            .iter()
                            .filter(|p| is_rbxjson(p) && !is_ignored(p))
                            .collect();
                        if !dominated.is_empty() {
                            break event;
                        }
                    }
                    Some(Err(_)) => {}
                    None => return, // channel closed
                }
            };

            let mut pending: Vec<String> = Vec::new();
            accumulate_paths(&first, &mut pending);

            // Debounce: collect more events for 1.5s after each event
            loop {
                match tokio::time::timeout(Duration::from_millis(1500), rx.recv()).await {
                    Ok(Some(Ok(event))) => accumulate_paths(&event, &mut pending),
                    Ok(Some(Err(_))) => {}
                    _ => break, // timeout (debounce done) or channel closed
                }
            }

            if pending.is_empty() || !watcher_active.load(Ordering::SeqCst) {
                continue;
            }

            // Emit file changed events
            for p in &pending {
                let _ = watcher_event.send(SyncEvent::FileChanged { path: p.clone() });
            }
            let count = pending.len() as u32;

            // Acquire sync lock and run rbxsync sync
            let _lock = watcher_lock.lock().await;
            let _ = watcher_event.send(SyncEvent::SyncStarted);

            match run_sync_command(&watcher_project).await {
                Ok(_) => {
                    let _ = watcher_event.send(SyncEvent::SyncCompleted {
                        files_synced: count,
                    });
                    *watcher_last_sync.lock().await = Some(SystemTime::now());
                }
                Err(e) => {
                    let _ = watcher_event.send(SyncEvent::Error { message: e });
                }
            }
        }
    });

    // ── Studio poller task (Studio → FS) ──
    let poller_event = on_event.clone();
    let poller_active = state.active.clone();
    let poller_lock = state.sync_lock.clone();
    let poller_last_sync = state.last_sync_time.clone();
    let poller_project = project_path.clone();

    let poller_handle = tokio::spawn(async move {
        loop {
            // Sleep first (measured from completion of previous cycle)
            tokio::time::sleep(Duration::from_secs(interval)).await;

            if !poller_active.load(Ordering::SeqCst) {
                break;
            }

            // Snapshot mtimes before extract (for conflict detection)
            let pre_mtimes = snapshot_mtimes(&poller_project);

            // Create backup
            let backup_path = match create_backup(&poller_project) {
                Ok(p) if p.is_empty() => {
                    // No src/ dir, skip extraction
                    continue;
                }
                Ok(p) => p,
                Err(e) => {
                    let _ = poller_event.send(SyncEvent::Error {
                        message: format!("Backup failed: {e}"),
                    });
                    continue;
                }
            };

            // Acquire sync lock and run extract
            let _lock = poller_lock.lock().await;
            let _ = poller_event.send(SyncEvent::ExtractStarted);

            match run_extract_command(&poller_project).await {
                Ok(_) => {
                    let _ = poller_event.send(SyncEvent::ExtractCompleted {
                        backup_path: backup_path.clone(),
                    });

                    // Conflict detection
                    let last_sync = *poller_last_sync.lock().await;
                    if let Some(last_sync_time) = last_sync {
                        let post_mtimes = snapshot_mtimes(&poller_project);
                        for (path, post_mtime) in &post_mtimes {
                            let was_modified_by_extract = pre_mtimes
                                .get(path)
                                .map(|pre| pre != post_mtime)
                                .unwrap_or(true);

                            let was_modified_locally = pre_mtimes
                                .get(path)
                                .map(|pre| *pre > last_sync_time)
                                .unwrap_or(false);

                            if was_modified_by_extract && was_modified_locally {
                                let _ = poller_event.send(SyncEvent::Conflict {
                                    path: path.to_string_lossy().to_string(),
                                    backup_path: backup_path.clone(),
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = poller_event.send(SyncEvent::Error { message: e });
                }
            }

            // Cleanup old backups
            cleanup_old_backups(&poller_project, 20);
        }
    });

    *state.watcher_handle.lock().await = Some(watcher_handle);
    *state.poller_handle.lock().await = Some(poller_handle);

    Ok(())
}

#[tauri::command]
pub async fn stop_auto_sync(state: tauri::State<'_, AutoSyncState>) -> Result<()> {
    state.active.store(false, Ordering::SeqCst);

    if let Some(handle) = state.watcher_handle.lock().await.take() {
        handle.abort();
    }
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
pub async fn trigger_sync_now(
    project_path: String,
    state: tauri::State<'_, AutoSyncState>,
) -> Result<String> {
    let project_path = expand_tilde(&project_path);
    let _lock = state.sync_lock.lock().await;

    run_sync_command(&project_path)
        .await
        .map_err(InstallerError::Custom)
}

#[tauri::command]
pub async fn trigger_extract_now(
    project_path: String,
    state: tauri::State<'_, AutoSyncState>,
) -> Result<String> {
    let project_path = expand_tilde(&project_path);
    let _lock = state.sync_lock.lock().await;

    create_backup(&project_path).map_err(InstallerError::Custom)?;
    cleanup_old_backups(&project_path, 20);

    run_extract_command(&project_path)
        .await
        .map_err(InstallerError::Custom)
}
