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
    let cmd = match editor.as_str() {
        "cursor" => "cursor",
        "vscode" | "windsurf" => "code",
        _ => "code",
    };

    let result = tokio::process::Command::new(cmd)
        .arg(&path)
        .spawn();

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to open {cmd}: {e}")),
    }
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
        .manage(commands::rbxsync::RbxSyncProcess::default())
        .manage(commands::autosync::AutoSyncState::default())
        .invoke_handler(tauri::generate_handler![
            commands::detect::detect_environment,
            commands::install::run_installation,
            commands::config::load_config,
            commands::config::save_project,
            commands::config::save_update_state,
            commands::config::save_settings,
            commands::update::check_for_update,
            commands::rojo::start_rojo,
            commands::rojo::stop_rojo,
            commands::rojo::get_rojo_status,
            commands::rbxsync::start_rbxsync,
            commands::rbxsync::stop_rbxsync,
            commands::rbxsync::get_rbxsync_status,
            commands::rbxsync::extract_rbxsync,
            commands::autosync::start_auto_sync,
            commands::autosync::stop_auto_sync,
            commands::autosync::get_auto_sync_status,
            commands::autosync::trigger_extract_now,
            open_url_fallback,
            open_in_editor,
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Kill rojo serve when the window is closed
                if let Some(state) = _window.try_state::<commands::rojo::RojoProcess>() {
                    state.inner().kill_sync();
                }
                // Kill rbxsync serve when the window is closed
                if let Some(state) = _window.try_state::<commands::rbxsync::RbxSyncProcess>() {
                    state.inner().kill_sync();
                }
                // Stop auto-sync when the window is closed
                if let Some(state) = _window.try_state::<commands::autosync::AutoSyncState>() {
                    state.inner().kill_sync();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Roxlit");
}
