use serde::Serialize;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use tokio::process::Command;

/// Results from scanning the user's system for required tools.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionResult {
    pub os: String,
    pub studio_installed: bool,
    pub studio_plugins_path: Option<String>,
    pub rojo_installed: bool,
    pub rojo_version: Option<String>,
    pub aftman_installed: bool,
    pub aftman_version: Option<String>,
}

/// Scans the system for Roblox Studio, Rojo, and Aftman.
#[tauri::command]
pub async fn detect_environment() -> crate::error::Result<DetectionResult> {
    let os = std::env::consts::OS.to_string();

    let (studio_installed, studio_plugins_path) = detect_studio(&os);
    let (rojo_installed, rojo_version) = detect_cli_tool("rojo").await;
    let (aftman_installed, aftman_version) = detect_cli_tool("aftman").await;

    Ok(DetectionResult {
        os,
        studio_installed,
        studio_plugins_path: studio_plugins_path.map(|p| p.to_string_lossy().to_string()),
        rojo_installed,
        rojo_version,
        aftman_installed,
        aftman_version,
    })
}

/// Checks known filesystem paths for a Roblox Studio installation.
fn detect_studio(os: &str) -> (bool, Option<PathBuf>) {
    match os {
        "windows" => {
            // Studio stores versions in %LOCALAPPDATA%\Roblox\Versions\
            if let Some(local_app_data) = dirs::data_local_dir() {
                let versions_dir = local_app_data.join("Roblox").join("Versions");
                if versions_dir.exists() {
                    // Each version is a subdirectory containing RobloxStudioBeta.exe
                    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
                        for entry in entries.flatten() {
                            if entry.path().join("RobloxStudioBeta.exe").exists() {
                                let plugins_path =
                                    local_app_data.join("Roblox").join("Plugins");
                                return (true, Some(plugins_path));
                            }
                        }
                    }
                }
            }
            (false, None)
        }
        "macos" => {
            let studio_app = PathBuf::from("/Applications/RobloxStudio.app");
            if studio_app.exists() {
                let plugins_path = dirs::home_dir()
                    .map(|h| h.join("Library").join("Roblox").join("Plugins"));
                (true, plugins_path)
            } else {
                (false, None)
            }
        }
        // Linux doesn't have native Roblox Studio support
        _ => (false, None),
    }
}

/// Runs `<tool> --version` and parses the output to check availability.
async fn detect_cli_tool(name: &str) -> (bool, Option<String>) {
    // Also check the aftman bin directory directly
    let bin_path = dirs::home_dir()
        .map(|h| h.join(".aftman").join("bin").join(name));

    let mut cmd = Command::new(name);
    cmd.arg("--version");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let result = cmd.output().await;

    // If the tool isn't in PATH, try the aftman bin directory
    let result = match result {
        Ok(output) if output.status.success() => Ok(output),
        _ => {
            if let Some(ref path) = bin_path {
                let mut cmd = Command::new(path);
                cmd.arg("--version");
                #[cfg(target_os = "windows")]
                cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
                cmd.output().await
            } else {
                return (false, None);
            }
        }
    };

    match result {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_string();
            (true, Some(version))
        }
        _ => (false, None),
    }
}
