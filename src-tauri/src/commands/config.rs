use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{InstallerError, Result};
use crate::util::expand_tilde;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub ai_tool: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub universe_id: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoxlitConfig {
    pub version: u32,
    pub projects: Vec<ProjectEntry>,
    pub last_active_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_update_check: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_delay_days: Option<u32>,
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".roxlit").join("config.json"))
}

#[tauri::command]
pub async fn load_config() -> Option<RoxlitConfig> {
    let path = config_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

#[tauri::command]
pub async fn save_project(project: ProjectEntry) -> Result<RoxlitConfig> {
    let path = config_path()
        .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?;

    let mut config = load_config().await.unwrap_or(RoxlitConfig {
        version: 1,
        projects: vec![],
        last_active_project: None,
        last_update_check: None,
        dismissed_version: None,
        update_delay_days: None,
    });

    // Expand tilde so paths are always absolute
    let mut project = project;
    project.path = expand_tilde(&project.path);

    // Upsert by path — preserve place_id/universe_id from existing entry
    if let Some(existing) = config.projects.iter_mut().find(|p| p.path == project.path) {
        let preserved_place_id = existing.place_id;
        let preserved_universe_id = existing.universe_id;
        *existing = project.clone();
        if existing.place_id.is_none() {
            existing.place_id = preserved_place_id;
        }
        if existing.universe_id.is_none() {
            existing.universe_id = preserved_universe_id;
        }
    } else {
        config.projects.push(project.clone());
    }
    config.last_active_project = Some(project.path);

    // Write
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| InstallerError::Custom(e.to_string()))?;
    std::fs::write(&path, json)?;

    Ok(config)
}

#[tauri::command]
pub async fn save_update_state(
    last_update_check: Option<String>,
    dismissed_version: Option<String>,
) -> Result<()> {
    let path = config_path()
        .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?;

    let mut config = load_config().await.unwrap_or(RoxlitConfig {
        version: 1,
        projects: vec![],
        last_active_project: None,
        last_update_check: None,
        dismissed_version: None,
        update_delay_days: None,
    });

    if last_update_check.is_some() {
        config.last_update_check = last_update_check;
    }
    if dismissed_version.is_some() {
        config.dismissed_version = dismissed_version;
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| InstallerError::Custom(e.to_string()))?;
    std::fs::write(&path, json)?;

    Ok(())
}

#[tauri::command]
pub async fn save_settings(update_delay_days: u32) -> Result<()> {
    let path = config_path()
        .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?;

    let mut config = load_config().await.unwrap_or(RoxlitConfig {
        version: 1,
        projects: vec![],
        last_active_project: None,
        last_update_check: None,
        dismissed_version: None,
        update_delay_days: None,
    });

    config.update_delay_days = Some(update_delay_days);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| InstallerError::Custom(e.to_string()))?;
    std::fs::write(&path, json)?;

    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredProject {
    pub name: String,
    pub path: String,
    pub ai_tool: String,
}

/// Scans a parent directory for existing Rojo projects (subdirs with default.project.json).
/// Skips dotfiles/dotdirs. Detects AI tool from context files.
#[tauri::command]
pub async fn scan_for_projects(parent_dir: String) -> Vec<DiscoveredProject> {
    let expanded = expand_tilde(&parent_dir);
    let parent = Path::new(&expanded);
    let mut projects = Vec::new();

    let entries = match std::fs::read_dir(parent) {
        Ok(e) => e,
        Err(_) => return projects,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Skip dotfiles/dotdirs
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }

        // Skip non-directories
        if !path.is_dir() {
            continue;
        }

        // Must have default.project.json (Rojo project indicator)
        if !path.join("default.project.json").exists() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let path_str = path.to_string_lossy().to_string();
        let ai_tool = detect_ai_tool(&path);

        projects.push(DiscoveredProject {
            name,
            path: path_str,
            ai_tool,
        });
    }

    projects
}

/// Detects which AI tool a project uses by checking for context files.
fn detect_ai_tool(project_path: &Path) -> String {
    if project_path.join("CLAUDE.md").exists() {
        return "claude".to_string();
    }
    if project_path.join(".cursorrules").exists() {
        return "cursor".to_string();
    }
    if project_path.join(".windsurfrules").exists() {
        return "windsurf".to_string();
    }
    if project_path
        .join(".github")
        .join("copilot-instructions.md")
        .exists()
    {
        return "vscode".to_string();
    }
    // Default for unknown
    "claude".to_string()
}

/// Checks if a project path still exists on disk (directory + default.project.json).
#[tauri::command]
pub async fn check_project_exists(path: String) -> bool {
    let expanded = expand_tilde(&path);
    let path = Path::new(&expanded);
    path.exists() && path.join("default.project.json").exists()
}

/// Persists the active project path in config so it's remembered on next launch.
#[tauri::command]
pub async fn set_active_project(path: String) -> Result<()> {
    let config_path = config_path()
        .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?;

    let mut config = load_config().await.unwrap_or(RoxlitConfig {
        version: 1,
        projects: vec![],
        last_active_project: None,
        last_update_check: None,
        dismissed_version: None,
        update_delay_days: None,
    });

    config.last_active_project = Some(expand_tilde(&path));

    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| InstallerError::Custom(e.to_string()))?;
    std::fs::write(&config_path, json)?;

    Ok(())
}

/// Persist a placeId and universeId for the given project path in the config file.
/// Called when stop_rojo flushes the linked IDs from LauncherStatus.
pub fn save_place_id(project_path: &str, place_id: u64, universe_id: Option<u64>) {
    let path = match config_path() {
        Some(p) => p,
        None => return,
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut config: RoxlitConfig = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(_) => return,
    };

    if let Some(project) = config.projects.iter_mut().find(|p| p.path == project_path) {
        project.place_id = Some(place_id);
        if let Some(uid) = universe_id {
            project.universe_id = Some(uid);
        }
        if let Ok(json) = serde_json::to_string_pretty(&config) {
            let _ = std::fs::write(&path, json);
        }
    }
}
