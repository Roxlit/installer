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
        // Claude Code is a CLI tool — open a terminal at the project directory
        #[cfg(target_os = "windows")]
        {
            // Try Windows Terminal first, fall back to cmd.exe
            let result = tokio::process::Command::new("wt.exe")
                .args(["-d", &path, "cmd", "/k", "claude"])
                .spawn();
            if result.is_ok() {
                return Ok(());
            }
            // Fallback: cmd.exe
            let result = tokio::process::Command::new("cmd.exe")
                .args(["/c", "start", "cmd.exe", "/k", &format!("cd /d \"{}\" && claude", path)])
                .spawn();
            match result {
                Ok(_) => return Ok(()),
                Err(e) => return Err(format!("Failed to open terminal: {e}")),
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            // On macOS/Linux, just run claude in the project directory
            let result = tokio::process::Command::new("claude")
                .current_dir(&path)
                .spawn();
            match result {
                Ok(_) => return Ok(()),
                Err(e) => return Err(format!("Failed to open claude: {e}")),
            }
        }
    }

    // GUI editors: pass path as argument to open the folder
    let candidates: &[&str] = match editor.as_str() {
        "cursor" => &["cursor"],
        "windsurf" => &["windsurf"],
        "vscode" => &[
            // CLI alias (only works if user ran "Install 'code' command in PATH")
            "code",
            // Default install paths on Windows
            r"C:\Users\Default\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd",
        ],
        _ => &["code"],
    };

    // On Windows, also check the per-user AppData path for VS Code
    #[cfg(target_os = "windows")]
    if editor == "vscode" {
        if let Some(local) = dirs::data_local_dir() {
            let exe = local.join("Programs").join("Microsoft VS Code").join("bin").join("code.cmd");
            if exe.exists() {
                let result = tokio::process::Command::new(&exe)
                    .arg(&path)
                    .creation_flags(0x08000000)
                    .spawn();
                if result.is_ok() {
                    return Ok(());
                }
            }
        }
    }

    for cmd in candidates {
        #[allow(unused_mut)]
        let mut command = tokio::process::Command::new(cmd);
        command.arg(&path);
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        if command.spawn().is_ok() {
            return Ok(());
        }
    }

    Err(format!("Could not open editor for '{editor}'. Make sure it is installed."))
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
