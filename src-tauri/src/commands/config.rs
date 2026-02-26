use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{InstallerError, Result};
use crate::util::expand_tilde;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub ai_tool: String,
    pub created_at: String,
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

    // Upsert by path
    if let Some(existing) = config.projects.iter_mut().find(|p| p.path == project.path) {
        *existing = project.clone();
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
