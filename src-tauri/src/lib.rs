mod commands;
mod error;
mod templates;

/// Fallback URL opener for WSL development where xdg-open doesn't work.
#[tauri::command]
async fn open_url_fallback(url: String) -> Result<(), String> {
    // Try wslview first (from wslu package), then cmd.exe /c start
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
        .invoke_handler(tauri::generate_handler![
            commands::detect::detect_environment,
            commands::install::run_installation,
            open_url_fallback,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Roxlit Installer");
}
