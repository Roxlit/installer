use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{InstallerError, Result};

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
    });

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
