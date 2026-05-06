use tauri::Manager;

mod commands;
mod error;
mod templates;
pub mod util;

/// Open a folder in the user's code editor (cursor, code, etc.)
/// For GUI editors (cursor, code, windsurf): passes the path as argument to open the folder.
/// For Claude Code: opens a terminal in the project directory and runs `claude`.
#[tauri::command]
async fn open_in_editor(editor: String, path: String) -> Result<(), String> {
    let path = util::expand_tilde(&path);

    if editor == "claude" {
        #[cfg(target_os = "windows")]
        {
            let result = tokio::process::Command::new("wt.exe")
                .args(["-d", &path, "cmd", "/k", "claude"])
                .creation_flags(0x08000000)
                .spawn();
            if result.is_ok() {
                return Ok(());
            }
            tokio::process::Command::new("cmd.exe")
                .args(["/c", "start", "cmd.exe", "/k", &format!("cd /d \"{}\" && claude", path)])
                .creation_flags(0x08000000)
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("Failed to open terminal: {e}"))?;
            return Ok(());
        }
        #[cfg(not(target_os = "windows"))]
        {
            return tokio::process::Command::new("claude")
                .current_dir(&path)
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("Failed to open claude: {e}"));
        }
    }

    // Find the editor executable, then open the project folder
    let exe = find_editor_exe(&editor)
        .ok_or_else(|| format!("Could not find '{editor}'. Make sure it is installed."))?;

    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg(&path);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch {}: {e}", exe.display()))
}

/// Finds the executable for a GUI editor.
/// Strategy: registry (Windows) → common install paths → PATH.
fn find_editor_exe(editor: &str) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // 1. Try Windows registry first — covers any install location
        if let Some(p) = find_editor_in_registry(editor) {
            return Some(p);
        }

        // 2. Common per-user and system install paths
        let local = dirs::data_local_dir()?;
        let program_files = std::env::var("ProgramFiles").unwrap_or_default();
        let program_files_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();

        let subfolder = match editor {
            "vscode"    => "Microsoft VS Code",
            "cursor"    => "cursor",
            "windsurf"  => "Windsurf",
            _           => "Microsoft VS Code",
        };
        let bin = match editor {
            "vscode" => "bin/code.cmd",
            _        => &format!("{}.exe", editor),
        };

        let roots = [
            local.to_string_lossy().to_string(),
            format!("{local}/Programs", local = local.display()),
            program_files.clone(),
            program_files_x86.clone(),
        ];

        for root in &roots {
            let candidate = std::path::Path::new(root).join(subfolder).join(bin);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 3. Fall back to PATH (works on macOS/Linux and Windows if the CLI is installed)
    let cli = match editor {
        "vscode"   => "code",
        "cursor"   => "cursor",
        "windsurf" => "windsurf",
        _          => "code",
    };
    // `which` just checks if the name resolves — use it as a sanity check
    which_exe(cli)
}

/// Looks up an editor executable path from the Windows registry.
/// VS Code, Cursor, and Windsurf all register under App Paths when installed.
#[cfg(target_os = "windows")]
fn find_editor_in_registry(editor: &str) -> Option<std::path::PathBuf> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    let exe_name = match editor {
        "vscode"   => "Code.exe",
        "cursor"   => "cursor.exe",
        "windsurf" => "windsurf.exe",
        _          => return None,
    };

    let reg_path = format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe_name}");

    // Check HKCU first (per-user install), then HKLM (system install)
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(&reg_path, KEY_READ) {
            if let Ok(path) = key.get_value::<String, _>("") {
                let p = std::path::PathBuf::from(&path);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Returns the full path of an executable if it exists in PATH, else None.
fn which_exe(name: &str) -> Option<std::path::PathBuf> {
    // Try spawning with --version to check existence without side effects
    let cmd = if cfg!(target_os = "windows") {
        std::process::Command::new("where").arg(name).output().ok()
    } else {
        std::process::Command::new("which").arg(name).output().ok()
    };
    cmd.and_then(|o| {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().lines().next()?.to_string();
            Some(std::path::PathBuf::from(s))
        } else {
            None
        }
    })
}

/// Fallback URL opener for WSL development where xdg-open doesn't work.
#[tauri::command]
async fn open_url_fallback(url: String) -> Result<(), String> {
    let result = tokio::process::Command::new("wslview")
        .arg(&url)
        .output()
        .await;

    if result.is_ok() && result.unwrap().status.success() {
        return Ok(());
    }

    let result = tokio::process::Command::new("cmd.exe")
        .args(["/c", "start", &url])
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err("Could not open URL".into()),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .manage(commands::rojo::RojoProcess::default())
        .manage(commands::logs::LoggerState::default())
        .manage(commands::logs::LogServerState::default())
        .manage(commands::logs::LauncherStatus::default())
        .manage(commands::logs::McpState::default())
        .manage(commands::logs::TelemetryState::default())
        .invoke_handler(tauri::generate_handler![
            commands::detect::detect_environment,
            commands::install::run_installation,
            commands::install::check_studio_mcp_status,
            commands::install::setup_studio_mcp,
            commands::config::load_config,
            commands::config::save_project,
            commands::config::save_update_state,
            commands::config::save_settings,
            commands::config::scan_for_projects,
            commands::config::check_project_exists,
            commands::config::set_active_project,
            commands::update::check_for_update,
            commands::rojo::start_rojo,
            commands::rojo::stop_rojo,
            commands::rojo::get_rojo_status,
            open_url_fallback,
            open_in_editor,
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Persist linked placeId before shutdown (so next Start Development opens Studio)
                if let Some(state) = _window.try_state::<commands::logs::LauncherStatus>() {
                    let shared = state.inner().shared();
                    let save_info = shared.try_lock().ok().and_then(|guard| {
                        let place_id = guard.linked_place_id?;
                        let path = if guard.project_path.is_empty() { return None } else { guard.project_path.clone() };
                        Some((path, place_id, guard.linked_universe_id))
                    });
                    if let Some((path, place_id, universe_id)) = save_info {
                        commands::config::save_place_id(&path, place_id, universe_id);
                    }
                }
                // Kill rojo serve when the window is closed
                if let Some(state) = _window.try_state::<commands::rojo::RojoProcess>() {
                    state.inner().kill_sync();
                }
                // Stop the Studio log HTTP server when the window is closed
                if let Some(state) = _window.try_state::<commands::logs::LogServerState>() {
                    state.inner().kill_sync();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Roxlit");
}
