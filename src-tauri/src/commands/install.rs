use crate::commands::context;
use crate::commands::project;
use crate::error::{InstallerError, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::ipc::Channel;
use tokio::io::AsyncWriteExt;

/// Progress events streamed from Rust to the React frontend via Channel.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum SetupEvent {
    #[serde(rename_all = "camelCase")]
    StepStarted {
        step: String,
        description: String,
        step_index: usize,
        total_steps: usize,
    },
    #[serde(rename_all = "camelCase")]
    StepProgress {
        step: String,
        progress: f64,
        detail: String,
    },
    #[serde(rename_all = "camelCase")]
    StepCompleted { step: String, detail: String },
    #[serde(rename_all = "camelCase")]
    StepWarning { step: String, message: String },
    #[serde(rename_all = "camelCase")]
    Error { step: String, message: String },
    Finished,
}

/// Configuration received from the frontend to drive the installation.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallConfig {
    pub ai_tool: String,
    pub project_path: String,
    pub project_name: String,
    pub skip_aftman: bool,
    pub skip_rojo: bool,
    pub skip_rbxsync: bool,
    pub plugins_path: Option<String>,
}

use crate::util::expand_tilde;

/// Orchestrates the full installation process, reporting progress through a Channel.
#[tauri::command]
pub async fn run_installation(
    config: InstallConfig,
    on_event: Channel<SetupEvent>,
) -> Result<()> {
    // Resolve ~ in the project path before doing anything
    let config = InstallConfig {
        project_path: expand_tilde(&config.project_path),
        ..config
    };

    let total_steps = calculate_total_steps(&config);
    let mut step_index: usize = 0;

    // Step 1: Install Aftman (if needed)
    if !config.skip_aftman {
        step_index += 1;
        on_event
            .send(SetupEvent::StepStarted {
                step: "aftman".into(),
                description: "Installing Aftman toolchain manager".into(),
                step_index,
                total_steps,
            })
            .map_err(|e| InstallerError::Custom(e.to_string()))?;

        match install_aftman(&on_event).await {
            Ok(()) => {
                on_event
                    .send(SetupEvent::StepCompleted {
                        step: "aftman".into(),
                        detail: "Aftman installed successfully".into(),
                    })
                    .map_err(|e| InstallerError::Custom(e.to_string()))?;
            }
            Err(e) => {
                on_event
                    .send(SetupEvent::Error {
                        step: "aftman".into(),
                        message: e.to_string(),
                    })
                    .map_err(|e| InstallerError::Custom(e.to_string()))?;
                return Err(e);
            }
        }
    }

    // Step 2: Install Rojo via Aftman (if needed)
    if !config.skip_rojo {
        step_index += 1;
        on_event
            .send(SetupEvent::StepStarted {
                step: "rojo".into(),
                description: "Installing Rojo file sync".into(),
                step_index,
                total_steps,
            })
            .map_err(|e| InstallerError::Custom(e.to_string()))?;

        match install_rojo(&config, &on_event).await {
            Ok(()) => {
                on_event
                    .send(SetupEvent::StepCompleted {
                        step: "rojo".into(),
                        detail: "Rojo installed successfully".into(),
                    })
                    .map_err(|e| InstallerError::Custom(e.to_string()))?;
            }
            Err(e) => {
                on_event
                    .send(SetupEvent::Error {
                        step: "rojo".into(),
                        message: e.to_string(),
                    })
                    .map_err(|e| InstallerError::Custom(e.to_string()))?;
                return Err(e);
            }
        }
    }

    // Step 3: Install Rojo Studio plugin
    step_index += 1;
    on_event
        .send(SetupEvent::StepStarted {
            step: "plugin".into(),
            description: "Installing Rojo plugin for Roblox Studio".into(),
            step_index,
            total_steps,
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    match install_studio_plugin(&config).await {
        Ok(()) => {
            on_event
                .send(SetupEvent::StepCompleted {
                    step: "plugin".into(),
                    detail: "Studio plugin installed".into(),
                })
                .map_err(|e| InstallerError::Custom(e.to_string()))?;
        }
        Err(e) => {
            // Plugin installation is non-critical — warn but continue
            on_event
                .send(SetupEvent::StepWarning {
                    step: "plugin".into(),
                    message: format!("Could not install plugin automatically: {e}. You can install it manually from the Rojo GitHub releases."),
                })
                .map_err(|e| InstallerError::Custom(e.to_string()))?;
        }
    }

    // Step 4: Install RbxSync (if needed) — non-critical, warn on failure
    if !config.skip_rbxsync {
        step_index += 1;
        on_event
            .send(SetupEvent::StepStarted {
                step: "rbxsync".into(),
                description: "Installing RbxSync (instance sync)".into(),
                step_index,
                total_steps,
            })
            .map_err(|e| InstallerError::Custom(e.to_string()))?;

        match install_rbxsync(&config, &on_event).await {
            Ok(()) => {
                on_event
                    .send(SetupEvent::StepCompleted {
                        step: "rbxsync".into(),
                        detail: "RbxSync installed successfully".into(),
                    })
                    .map_err(|e| InstallerError::Custom(e.to_string()))?;
            }
            Err(e) => {
                // RbxSync is non-critical — warn but continue
                on_event
                    .send(SetupEvent::StepWarning {
                        step: "rbxsync".into(),
                        message: format!("Could not install RbxSync: {e}. You can install it manually later."),
                    })
                    .map_err(|e| InstallerError::Custom(e.to_string()))?;
            }
        }
    }

    // Step 4b: Install RoxlitDebug plugin (non-critical, no extra step count)
    install_debug_plugin(&config, &on_event);

    // Step 5: Create project structure
    step_index += 1;
    on_event
        .send(SetupEvent::StepStarted {
            step: "project".into(),
            description: "Creating project structure".into(),
            step_index,
            total_steps,
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    project::create_project(&config.project_path, &config.project_name)?;
    on_event
        .send(SetupEvent::StepCompleted {
            step: "project".into(),
            detail: "Project structure created".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Step 6: Generate AI context files + MCP config
    step_index += 1;
    on_event
        .send(SetupEvent::StepStarted {
            step: "context".into(),
            description: "Generating AI context files".into(),
            step_index,
            total_steps,
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    context::generate_context(&config.project_path, &config.ai_tool, &config.project_name)?;
    on_event
        .send(SetupEvent::StepCompleted {
            step: "context".into(),
            detail: format!(
                "AI context files generated for {}",
                context::tool_display_name(&config.ai_tool)
            ),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // All done
    on_event
        .send(SetupEvent::Finished)
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    Ok(())
}

fn calculate_total_steps(config: &InstallConfig) -> usize {
    let mut steps = 3; // plugin + project + context are always run
    if !config.skip_aftman {
        steps += 1;
    }
    if !config.skip_rojo {
        steps += 1;
    }
    if !config.skip_rbxsync {
        steps += 1;
    }
    steps
}

/// Downloads and installs Aftman from its GitHub releases.
async fn install_aftman(on_event: &Channel<SetupEvent>) -> Result<()> {
    // Asset names follow the pattern: aftman-{version}-{platform}-{arch}.zip
    // We use a known stable version to avoid breaking changes in future releases.
    let version = "0.3.0";
    let target = if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "macos-aarch64"
        } else {
            "macos-x86_64"
        }
    } else {
        "linux-x86_64"
    };

    let url = format!(
        "https://github.com/LPGhatguy/aftman/releases/download/v{version}/aftman-{version}-{target}.zip"
    );

    on_event
        .send(SetupEvent::StepProgress {
            step: "aftman".into(),
            progress: 0.1,
            detail: "Downloading Aftman...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Download the zip to a temp file
    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        return Err(InstallerError::Custom(format!(
            "Failed to download Aftman: HTTP {} from {url}",
            response.status()
        )));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut stream = response.bytes_stream();

    let temp_dir = std::env::temp_dir().join("roxlit-installer");
    tokio::fs::create_dir_all(&temp_dir).await?;
    let zip_path = temp_dir.join("aftman.zip");

    let mut file = tokio::fs::File::create(&zip_path).await?;
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let progress = 0.1 + (downloaded as f64 / total_size as f64) * 0.6;
            on_event
                .send(SetupEvent::StepProgress {
                    step: "aftman".into(),
                    progress,
                    detail: format!(
                        "Downloading... {:.1} MB / {:.1} MB",
                        downloaded as f64 / 1_000_000.0,
                        total_size as f64 / 1_000_000.0,
                    ),
                })
                .map_err(|e| InstallerError::Custom(e.to_string()))?;
        }
    }
    file.flush().await?;
    drop(file);

    on_event
        .send(SetupEvent::StepProgress {
            step: "aftman".into(),
            progress: 0.75,
            detail: "Extracting Aftman...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Extract the zip — this is sync but fast, so we spawn_blocking
    let zip_path_clone = zip_path.clone();
    let aftman_bin_dir = dirs::home_dir()
        .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?
        .join(".aftman")
        .join("bin");

    let bin_dir = aftman_bin_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        std::fs::create_dir_all(&bin_dir)?;
        let file = std::fs::File::open(&zip_path_clone)?;
        let mut archive = zip::ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();

            // Only extract the aftman binary
            if name.contains("aftman") && !name.ends_with('/') {
                let file_name = std::path::Path::new(&name)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let out_path = bin_dir.join(&file_name);
                let mut out_file = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out_file)?;

                // Make executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        &out_path,
                        std::fs::Permissions::from_mode(0o755),
                    )?;
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| InstallerError::Custom(e.to_string()))??;

    on_event
        .send(SetupEvent::StepProgress {
            step: "aftman".into(),
            progress: 0.9,
            detail: "Running aftman self-install...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Run aftman self-install to configure PATH integration
    let aftman_bin = aftman_bin_dir.join(if cfg!(target_os = "windows") {
        "aftman.exe"
    } else {
        "aftman"
    });

    let mut cmd = tokio::process::Command::new(&aftman_bin);
    cmd.arg("self-install");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // self-install can fail if already installed — that's fine
        if !stderr.contains("already") {
            return Err(InstallerError::Custom(format!(
                "aftman self-install failed: {stderr}"
            )));
        }
    }

    // Clean up temp files
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    Ok(())
}

/// Installs Rojo by writing an aftman.toml and running aftman install.
async fn install_rojo(config: &InstallConfig, on_event: &Channel<SetupEvent>) -> Result<()> {
    let project_path = PathBuf::from(&config.project_path);
    std::fs::create_dir_all(&project_path)?;

    on_event
        .send(SetupEvent::StepProgress {
            step: "rojo".into(),
            progress: 0.2,
            detail: "Adding Rojo to project toolchain...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Write aftman.toml pinning the Rojo version
    let aftman_toml = project_path.join("aftman.toml");
    std::fs::write(
        &aftman_toml,
        "[tools]\nrojo = \"rojo-rbx/rojo@7.4.4\"\n",
    )?;

    // Use the full path to aftman since it may not be in PATH yet
    let aftman_bin = dirs::home_dir()
        .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?
        .join(".aftman")
        .join("bin")
        .join(if cfg!(target_os = "windows") {
            "aftman.exe"
        } else {
            "aftman"
        });

    on_event
        .send(SetupEvent::StepProgress {
            step: "rojo".into(),
            progress: 0.3,
            detail: "Stopping existing Rojo processes...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Kill any running rojo process so aftman can overwrite rojo.exe
    kill_process_by_name("rojo").await;

    on_event
        .send(SetupEvent::StepProgress {
            step: "rojo".into(),
            progress: 0.4,
            detail: "Downloading Rojo (this may take a moment)...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Try aftman install with retry — file locks on Windows can linger briefly
    let max_attempts = 3;
    let mut last_err = String::new();
    for attempt in 1..=max_attempts {
        let mut cmd = tokio::process::Command::new(&aftman_bin);
        cmd.arg("install")
            .arg("--no-trust-check")
            .current_dir(&project_path);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let output = cmd.output().await?;

        if output.status.success() {
            last_err.clear();
            break;
        }

        last_err = String::from_utf8_lossy(&output.stderr).to_string();

        // Only retry on file lock errors (os error 32 on Windows)
        if !last_err.contains("os error 32") || attempt == max_attempts {
            break;
        }

        on_event
            .send(SetupEvent::StepProgress {
                step: "rojo".into(),
                progress: 0.4,
                detail: format!("File locked, retrying ({attempt}/{max_attempts})..."),
            })
            .map_err(|e| InstallerError::Custom(e.to_string()))?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        kill_process_by_name("rojo").await;
    }

    if !last_err.is_empty() {
        return Err(InstallerError::Custom(format!(
            "Failed to install Rojo: {last_err}"
        )));
    }

    on_event
        .send(SetupEvent::StepProgress {
            step: "rojo".into(),
            progress: 1.0,
            detail: "Rojo installed".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    Ok(())
}

const RBXSYNC_REPO: &str = "Smokestack-Games/rbxsync";
const RBXSYNC_VERSION: &str = "1.3.0";

/// Downloads a binary from a URL to the target path with progress reporting.
async fn download_binary(url: &str, target_path: &PathBuf) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
        return Err(InstallerError::Custom(format!(
            "Failed to download: HTTP {} from {url}",
            response.status()
        )));
    }

    let bytes = response.bytes().await?;
    tokio::fs::write(target_path, &bytes).await?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(target_path, std::fs::Permissions::from_mode(0o755)).await?;
    }

    Ok(())
}

/// Returns the RbxSync CLI asset name for the current platform.
fn rbxsync_asset_name() -> Option<&'static str> {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some("rbxsync-windows-x86_64.exe")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Some("rbxsync-macos-aarch64")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        Some("rbxsync-macos-x86_64")
    } else {
        None // Linux or unsupported arch
    }
}

/// Returns the RbxSync MCP server download URL for the current platform.
/// macOS ARM: from upstream rbxsync releases. Windows: from our Roxlit releases.
fn rbxsync_mcp_download_url() -> Option<String> {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Some(format!(
            "https://github.com/{RBXSYNC_REPO}/releases/download/v{RBXSYNC_VERSION}/rbxsync-mcp-macos-arm64"
        ))
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some(format!(
            "https://github.com/Roxlit/installer/releases/latest/download/rbxsync-mcp.exe"
        ))
    } else {
        None
    }
}

/// Downloads and installs RbxSync CLI, Studio plugin, and MCP server (if available).
async fn install_rbxsync(config: &InstallConfig, on_event: &Channel<SetupEvent>) -> Result<()> {
    let asset_name = rbxsync_asset_name().ok_or_else(|| {
        InstallerError::Custom("RbxSync is not available for this platform".into())
    })?;

    let home = dirs::home_dir()
        .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?;
    let bin_dir = home.join(".roxlit").join("bin");

    // 1. Download RbxSync CLI
    on_event
        .send(SetupEvent::StepProgress {
            step: "rbxsync".into(),
            progress: 0.1,
            detail: "Downloading RbxSync CLI...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    let cli_url = format!(
        "https://github.com/{RBXSYNC_REPO}/releases/download/v{RBXSYNC_VERSION}/{asset_name}"
    );
    let cli_bin_name = if cfg!(target_os = "windows") {
        "rbxsync.exe"
    } else {
        "rbxsync"
    };
    let cli_path = bin_dir.join(cli_bin_name);
    download_binary(&cli_url, &cli_path).await?;

    // 2. Download RbxSync Studio plugin
    on_event
        .send(SetupEvent::StepProgress {
            step: "rbxsync".into(),
            progress: 0.5,
            detail: "Installing RbxSync Studio plugin...".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    let plugin_url = format!(
        "https://github.com/{RBXSYNC_REPO}/releases/download/v{RBXSYNC_VERSION}/RbxSync.rbxm"
    );

    let plugins_path = match &config.plugins_path {
        Some(path) => PathBuf::from(path),
        None => {
            if cfg!(target_os = "windows") {
                dirs::data_local_dir()
                    .ok_or_else(|| InstallerError::Custom("Cannot find AppData".into()))?
                    .join("Roblox")
                    .join("Plugins")
            } else if cfg!(target_os = "macos") {
                home.join("Library").join("Roblox").join("Plugins")
            } else {
                return Ok(()); // Linux — no plugins
            }
        }
    };
    std::fs::create_dir_all(&plugins_path)?;
    let plugin_path = plugins_path.join("RbxSync.rbxm");
    download_binary(&plugin_url, &plugin_path).await?;

    // 3. Download MCP server (macOS ARM + Windows x64)
    if let Some(mcp_url) = rbxsync_mcp_download_url() {
        on_event
            .send(SetupEvent::StepProgress {
                step: "rbxsync".into(),
                progress: 0.8,
                detail: "Installing RbxSync MCP server...".into(),
            })
            .map_err(|e| InstallerError::Custom(e.to_string()))?;

        let mcp_bin_name = if cfg!(target_os = "windows") {
            "rbxsync-mcp.exe"
        } else {
            "rbxsync-mcp"
        };
        let mcp_path = bin_dir.join(mcp_bin_name);
        download_binary(&mcp_url, &mcp_path).await?;
    }

    on_event
        .send(SetupEvent::StepProgress {
            step: "rbxsync".into(),
            progress: 1.0,
            detail: "RbxSync installed".into(),
        })
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    Ok(())
}

/// Downloads and copies the Rojo Studio plugin to the local plugins folder.
async fn install_studio_plugin(config: &InstallConfig) -> Result<()> {
    let plugins_path = match &config.plugins_path {
        Some(path) => PathBuf::from(path),
        None => {
            // Use the default plugins path for the current OS
            let base = if cfg!(target_os = "windows") {
                dirs::data_local_dir()
                    .ok_or_else(|| InstallerError::Custom("Cannot find AppData".into()))?
                    .join("Roblox")
                    .join("Plugins")
            } else if cfg!(target_os = "macos") {
                dirs::home_dir()
                    .ok_or_else(|| InstallerError::Custom("Cannot find home directory".into()))?
                    .join("Library")
                    .join("Roblox")
                    .join("Plugins")
            } else {
                return Err(InstallerError::Custom(
                    "Roblox Studio plugins are not supported on this OS".into(),
                ));
            };
            base
        }
    };

    std::fs::create_dir_all(&plugins_path)?;

    // Download the Rojo plugin from the latest release
    let url = "https://github.com/rojo-rbx/rojo/releases/latest/download/Rojo.rbxm";
    let response = reqwest::get(url).await?;

    if !response.status().is_success() {
        return Err(InstallerError::Custom(format!(
            "Failed to download Rojo plugin: HTTP {}",
            response.status()
        )));
    }

    let bytes = response.bytes().await?;

    let plugin_file = plugins_path.join("Rojo.rbxm");
    std::fs::write(&plugin_file, &bytes)?;

    Ok(())
}

/// Attempts to kill all processes matching the given name.
/// Silently ignores errors — this is best-effort to release file locks.
async fn kill_process_by_name(name: &str) {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", &format!("{name}.exe")])
            .creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = tokio::process::Command::new("pkill");
        cmd.args(["-f", name]);
        let _ = cmd.output().await;
    }

    // Give the OS a moment to release file handles
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}

/// Install the RoxlitDebug Studio plugin for capturing Studio output.
/// Non-critical — emits a warning and continues if it fails.
fn install_debug_plugin(config: &InstallConfig, on_event: &Channel<SetupEvent>) {
    let plugins_dir = match &config.plugins_path {
        Some(path) => PathBuf::from(path),
        None => {
            if cfg!(target_os = "windows") {
                match dirs::data_local_dir() {
                    Some(d) => d.join("Roblox").join("Plugins"),
                    None => return,
                }
            } else if cfg!(target_os = "macos") {
                match dirs::home_dir() {
                    Some(d) => d.join("Library").join("Roblox").join("Plugins"),
                    None => return,
                }
            } else {
                return;
            }
        }
    };

    if std::fs::create_dir_all(&plugins_dir).is_err() {
        let _ = on_event.send(SetupEvent::StepWarning {
            step: "plugin".into(),
            message: "Could not create plugins directory for RoxlitDebug".into(),
        });
        return;
    }

    let plugin_path = plugins_dir.join("RoxlitDebug.rbxm");
    match std::fs::write(&plugin_path, crate::templates::debug_plugin_rbxm()) {
        Ok(_) => {}
        Err(e) => {
            let _ = on_event.send(SetupEvent::StepWarning {
                step: "plugin".into(),
                message: format!("Could not install RoxlitDebug plugin: {e}"),
            });
        }
    }
}
