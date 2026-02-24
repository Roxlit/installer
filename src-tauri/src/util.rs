/// Expands a leading `~` in a path to the user's home directory.
/// Also normalizes path separators for the current OS.
pub fn expand_tilde(path: &str) -> String {
    let result = if path.starts_with("~/") || path == "~" {
        if let Some(home) = dirs::home_dir() {
            let rest = &path[1..]; // "/RobloxProjects/..."
            home.join(&rest[1..]).to_string_lossy().to_string()
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };
    // Normalize separators for the current OS
    if cfg!(windows) {
        result.replace('/', "\\")
    } else {
        result
    }
}
