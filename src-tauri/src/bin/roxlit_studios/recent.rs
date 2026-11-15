//! Keep session places out of Studio's "Recent" list. Every session Studio opens a
//! `roxlit-<token>.rbxl`, and Studio records each one in Juan's recent files; by
//! 2026-11-14 they were 97 of the 100 slots and had pushed his real places out.
//!
//! The list lives in `HKCU\Software\Roblox\RobloxStudio\<userId>rbxRecentFiles_v03` as
//! `<i>name` (string) + `<i>lastReadTime` (binary) pairs, newest first, plus a DWORD
//! `rbxRecentFilesCount`. A running Studio may write its own copy back, so the prune runs
//! on a timer rather than once.

use std::time::Duration;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

const ROOT: &str = r"Software\Roblox\RobloxStudio";
const SUFFIX: &str = "rbxRecentFiles_v03";
const COUNT: &str = "rbxRecentFilesCount";
const EVERY: Duration = Duration::from_secs(20);

fn is_session(name: &str) -> bool {
    let n = name.replace('\\', "/").to_lowercase();
    n.contains("/.roxlit/sessions/roxlit-") && n.ends_with(".rbxl")
}

/// Remove session places from one recent-files key, keeping the order of the rest.
/// Returns how many entries it removed.
fn prune_key(key: &RegKey) -> std::io::Result<usize> {
    let mut entries: Vec<(String, Option<RegValue>)> = Vec::new();
    let mut i = 0;
    while let Ok(raw) = key.get_raw_value(format!("{i}name")) {
        let name = String::from_reg_value(&raw).unwrap_or_default();
        let time = key.get_raw_value(format!("{i}lastReadTime")).ok();
        entries.push((name, time));
        i += 1;
    }
    let total = entries.len();
    let keep: Vec<_> = entries
        .into_iter()
        .filter(|(n, _)| !is_session(n))
        .collect();
    let removed = total - keep.len();
    if removed == 0 {
        return Ok(0);
    }
    for (j, (name, time)) in keep.iter().enumerate() {
        key.set_value(format!("{j}name"), name)?;
        match time {
            Some(t) => key.set_raw_value(format!("{j}lastReadTime"), t)?,
            None => {
                let _ = key.delete_value(format!("{j}lastReadTime"));
            }
        }
    }
    for j in keep.len()..total {
        let _ = key.delete_value(format!("{j}name"));
        let _ = key.delete_value(format!("{j}lastReadTime"));
    }
    key.set_value(COUNT, &(keep.len() as u32))?;
    Ok(removed)
}

/// Prune every Studio user's recent list once. Returns entries removed.
pub fn prune() -> usize {
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(ROOT, KEY_READ) else {
        return 0;
    };
    let names: Vec<String> = root
        .enum_keys()
        .flatten()
        .filter(|k| k.ends_with(SUFFIX))
        .collect();
    names
        .iter()
        .filter_map(|n| root.open_subkey_with_flags(n, KEY_READ | KEY_WRITE).ok())
        .map(|k| prune_key(&k).unwrap_or(0))
        .sum()
}

fn log(n: usize) {
    use std::io::Write;
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".roxlit")
        .join("sessions")
        .join("recent.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{} removed {n} session places", crate::session::now());
    }
}

pub fn start() {
    std::thread::spawn(|| loop {
        let n = prune();
        if n > 0 {
            log(n);
        }
        std::thread::sleep(EVERY);
    });
}

#[cfg(test)]
mod tests {
    use super::is_session;

    #[test]
    fn matches_only_session_places() {
        assert!(is_session(
            "C:/Users/Juan/.roxlit/sessions/roxlit-bdd28687.rbxl"
        ));
        assert!(is_session(
            r"C:\Users\Juan\.roxlit\sessions\roxlit-bdd28687.rbxl"
        ));
        assert!(!is_session(
            "C:/Users/Juan/RobloxProjects/Idea/bomb-brawler/build/BombBrawler.rbxlx"
        ));
        assert!(!is_session("C:/Users/Juan/Desktop/roxlit-notes.rbxl"));
    }
}
