use tauri::Manager;

mod commands;
mod error;
mod templates;

/// Open a folder in the user's code editor (cursor, code, etc.)
#[tauri::command]
async fn open_in_editor(editor: String, path: String) -> Result<(), String> {
    let cmd = match editor.as_str() {
        "cursor" => "cursor",
        "claude" => "claude",
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
        .invoke_handler(tauri::generate_handler![
            commands::detect::detect_environment,
            commands::install::run_installation,
            commands::config::load_config,
            commands::config::save_project,
            commands::rojo::start_rojo,
            commands::rojo::stop_rojo,
            commands::rojo::get_rojo_status,
            open_url_fallback,
            open_in_editor,
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Kill rojo serve when the window is closed
                if let Some(state) = _window.try_state::<commands::rojo::RojoProcess>() {
                    state.inner().kill_sync();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Roxlit");
}
