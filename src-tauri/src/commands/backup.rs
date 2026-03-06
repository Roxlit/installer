//! Backup system — creates git stash snapshots of project state.
//! Used by both the MCP server (roxlit_mcp.rs) and auto-backup timer.

use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Run a git command in the given directory.
pub fn run_git(path: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if stderr.trim().is_empty() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(stderr)
        }
    }
}

/// Ensure the project directory is a git repository.
pub fn ensure_git_repo(path: &str) -> Result<(), String> {
    let git_dir = Path::new(path).join(".git");
    if git_dir.exists() {
        return Ok(());
    }

    if Command::new("git").arg("--version").output().is_err() {
        return Err(
            "Git is not installed. Backups require git. Download it from: https://git-scm.com"
                .to_string(),
        );
    }

    run_git(path, &["init"]).map_err(|e| format!("Failed to init git repo: {e}"))?;
    run_git(path, &["add", "-A"]).map_err(|e| format!("Failed to stage files: {e}"))?;
    run_git(path, &["commit", "-m", "roxlit: initial commit for backups"])
        .map_err(|e| format!("Failed to create initial commit: {e}"))?;

    Ok(())
}

/// Get the next backup ID (bk-001, bk-002, etc.) from the manifest.
pub fn next_backup_id(path: &str) -> String {
    let manifest_path = Path::new(path).join(".roxlit").join("backups.jsonl");

    let mut max_num: u32 = 0;
    if let Ok(content) = std::fs::read_to_string(&manifest_path) {
        for line in content.lines() {
            if let Ok(entry) = serde_json::from_str::<Value>(line) {
                if let Some(id) = entry["id"].as_str() {
                    if let Some(num_str) = id.strip_prefix("bk-") {
                        if let Ok(n) = num_str.parse::<u32>() {
                            max_num = max_num.max(n);
                        }
                    }
                }
            }
        }
    }

    format!("bk-{:03}", max_num + 1)
}

/// Get current timestamp as ISO 8601 string (cross-platform).
pub fn now_timestamp() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    for md in &month_days {
        if remaining < *md {
            break;
        }
        remaining -= md;
        m += 1;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m + 1,
        remaining + 1,
        hours,
        minutes,
        seconds
    )
}

/// Find the stash index for a given backup ID.
pub fn find_stash_index(path: &str, backup_id: &str) -> Option<usize> {
    let stash_list = run_git(path, &["stash", "list"]).ok()?;
    let pattern = format!("roxlit:{backup_id}:");

    for line in stash_list.lines() {
        if line.contains(&pattern) {
            let start = line.find('{')? + 1;
            let end = line.find('}')?;
            return line[start..end].parse().ok();
        }
    }
    None
}

/// Check if a backup ID is a pre-restore backup.
pub fn is_pre_restore_backup(path: &str, backup_id: &str) -> bool {
    let manifest_path = Path::new(path).join(".roxlit").join("backups.jsonl");
    if let Ok(content) = std::fs::read_to_string(&manifest_path) {
        for line in content.lines() {
            if let Ok(entry) = serde_json::from_str::<Value>(line) {
                if entry["id"].as_str() == Some(backup_id) {
                    if let Some(name) = entry["name"].as_str() {
                        return name.starts_with("pre-restore-");
                    }
                }
            }
        }
    }
    false
}

/// Create a backup. Returns (backup_id, message) on success.
pub fn create_backup(path: &str, name: &str) -> Result<(String, String), String> {
    ensure_git_repo(path)?;

    // Stage everything so stash captures untracked files
    run_git(path, &["add", "-A"]).map_err(|e| format!("Failed to stage files: {e}"))?;

    let sha = run_git(path, &["stash", "create"])
        .map_err(|e| format!("Failed to create stash: {e}"))?
        .trim()
        .to_string();

    // Reset index
    let _ = run_git(path, &["reset"]);

    if sha.is_empty() {
        return Err("Nothing to backup — no changes detected since last commit.".to_string());
    }

    let backup_id = next_backup_id(path);
    let label = if name.is_empty() {
        backup_id.clone()
    } else {
        name.to_string()
    };
    let stash_msg = format!("roxlit:{backup_id}:{label}");

    run_git(path, &["stash", "store", "-m", &stash_msg, &sha])
        .map_err(|e| format!("Failed to store stash: {e}"))?;

    // Write to manifest
    let timestamp = now_timestamp();
    let manifest_dir = Path::new(path).join(".roxlit");
    let _ = std::fs::create_dir_all(&manifest_dir);
    let manifest_path = manifest_dir.join("backups.jsonl");

    // Get list of changed files for metadata
    let changed_files = run_git(path, &["stash", "show", "--name-only", &sha])
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .collect::<Vec<_>>();

    let entry = json!({
        "id": backup_id,
        "name": if name.is_empty() { None::<&str> } else { Some(name) },
        "timestamp": timestamp,
        "stash_sha": sha,
        "auto": name.starts_with("auto-"),
        "files_changed": changed_files,
    });

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&manifest_path)
    {
        let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }

    let display_name = if name.is_empty() {
        backup_id.clone()
    } else {
        format!("{backup_id} ({name})")
    };

    Ok((
        backup_id,
        format!("Backup created: {display_name}\nTimestamp: {timestamp}"),
    ))
}

/// Get total size of all roxlit stashes in bytes.
pub fn total_stash_size(path: &str) -> u64 {
    let manifest_path = Path::new(path).join(".roxlit").join("backups.jsonl");
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut total: u64 = 0;
    for line in content.lines() {
        if let Ok(entry) = serde_json::from_str::<Value>(line) {
            if let Some(sha) = entry["stash_sha"].as_str() {
                // Get object size from git
                if let Ok(size_str) = run_git(path, &["cat-file", "-s", sha]) {
                    if let Ok(size) = size_str.trim().parse::<u64>() {
                        total += size;
                    }
                }
            }
        }
    }
    total
}

/// Clean up old auto-backups if total size exceeds the limit.
/// Removes oldest auto-backups first, keeps manual backups.
pub fn cleanup_by_size(path: &str, max_bytes: u64) {
    let manifest_path = Path::new(path).join(".roxlit").join("backups.jsonl");
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let entries: Vec<Value> = content
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    if entries.is_empty() {
        return;
    }

    // Calculate current total size
    let current_size = total_stash_size(path);
    if current_size <= max_bytes {
        return;
    }

    // Find auto-backups sorted by timestamp (oldest first)
    let mut auto_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e["auto"].as_bool().unwrap_or(false))
        .map(|(i, _)| i)
        .collect();

    // Sort by timestamp ascending (oldest first)
    auto_indices.sort_by(|a, b| {
        let ts_a = entries[*a]["timestamp"].as_str().unwrap_or("");
        let ts_b = entries[*b]["timestamp"].as_str().unwrap_or("");
        ts_a.cmp(ts_b)
    });

    // Remove oldest auto-backups until under limit
    let mut removed = Vec::new();
    let mut estimated_size = current_size;
    for idx in &auto_indices {
        if estimated_size <= max_bytes {
            break;
        }
        let entry = &entries[*idx];
        if let Some(sha) = entry["stash_sha"].as_str() {
            if let Ok(size_str) = run_git(path, &["cat-file", "-s", sha]) {
                if let Ok(size) = size_str.trim().parse::<u64>() {
                    estimated_size = estimated_size.saturating_sub(size);
                }
            }
            // Drop the stash from git
            if let Some(backup_id) = entry["id"].as_str() {
                if let Some(stash_idx) = find_stash_index(path, backup_id) {
                    let stash_ref = format!("stash@{{{stash_idx}}}");
                    let _ = run_git(path, &["stash", "drop", &stash_ref]);
                }
            }
        }
        removed.push(*idx);
    }

    if removed.is_empty() {
        return;
    }

    // Rewrite manifest without removed entries
    let removed_set: std::collections::HashSet<usize> = removed.into_iter().collect();
    let remaining: Vec<&Value> = entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !removed_set.contains(i))
        .map(|(_, e)| e)
        .collect();

    if let Ok(mut f) = std::fs::File::create(&manifest_path) {
        for entry in remaining {
            let _ = writeln!(f, "{}", serde_json::to_string(entry).unwrap_or_default());
        }
    }
}
