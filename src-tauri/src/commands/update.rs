use serde::Serialize;

use crate::error::{InstallerError, Result};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub published_at: String,
    pub html_url: String,
    pub body: String,
}

/// Parse an ISO 8601 UTC timestamp (e.g. "2025-06-15T10:30:00Z") into a Unix timestamp (seconds).
/// Only handles the exact format GitHub returns: YYYY-MM-DDTHH:MM:SSZ.
fn parse_iso8601_to_unix(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 20 || !s.ends_with('Z') {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: i64 = s[5..7].parse().ok()?;
    let day: i64 = s[8..10].parse().ok()?;
    let hour: i64 = s[11..13].parse().ok()?;
    let min: i64 = s[14..16].parse().ok()?;
    let sec: i64 = s[17..19].parse().ok()?;

    // Days from year 0 to the start of the given year (simplified, handles leap years)
    fn days_from_year(y: i64) -> i64 {
        365 * y + y / 4 - y / 100 + y / 400
    }

    let month_days: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;

    let mut day_of_year: i64 = day - 1;
    for i in 0..(month - 1) as usize {
        day_of_year += month_days[i];
    }
    if is_leap && month > 2 {
        day_of_year += 1;
    }

    let days = days_from_year(year) - days_from_year(1970) + day_of_year;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

/// Get current time as Unix timestamp (seconds).
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Compare two semver strings (e.g. "0.2.0" > "0.1.0").
/// Returns true if `remote` is newer than `local`.
fn is_newer_version(local: &str, remote: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64) {
        let parts: Vec<u64> = s
            .trim_start_matches('v')
            .splitn(3, '.')
            .filter_map(|p| p.parse().ok())
            .collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };
    let l = parse(local);
    let r = parse(remote);
    r > l
}

const RATE_LIMIT_SECS: i64 = 24 * 3600; // 24 hours

#[tauri::command]
pub async fn check_for_update(
    last_check: Option<String>,
    dismissed_version: Option<String>,
    cooling_days: Option<u32>,
) -> Result<Option<UpdateInfo>> {
    let cooling_secs = (cooling_days.unwrap_or(7) as i64) * 24 * 3600;
    // Rate limit: skip if last check was less than 24h ago
    if let Some(ref ts) = last_check {
        if let Some(last_unix) = parse_iso8601_to_unix(ts) {
            if now_unix() - last_unix < RATE_LIMIT_SECS {
                return Ok(None);
            }
        }
    }

    // Fetch latest release from GitHub
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/Roxlit/installer/releases/latest")
        .header("User-Agent", "Roxlit-Launcher")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    let response = match response {
        Ok(r) => r,
        Err(_) => return Ok(None), // Network error — silent failure
    };

    // 404 means no releases exist yet
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if !response.status().is_success() {
        return Ok(None);
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| InstallerError::Custom(e.to_string()))?;

    // Filter out drafts and pre-releases
    if body["draft"].as_bool().unwrap_or(true) || body["prerelease"].as_bool().unwrap_or(true) {
        return Ok(None);
    }

    let tag = body["tag_name"].as_str().unwrap_or_default();
    let remote_version = tag.trim_start_matches('v');
    let local_version = env!("CARGO_PKG_VERSION");

    // Already dismissed this version
    if let Some(ref dismissed) = dismissed_version {
        if dismissed.trim_start_matches('v') == remote_version {
            return Ok(None);
        }
    }

    // Check if remote is actually newer
    if !is_newer_version(local_version, remote_version) {
        return Ok(None);
    }

    // Cooling period: release must be old enough (configurable, default 7 days)
    let published_at = body["published_at"].as_str().unwrap_or_default();
    if let Some(pub_unix) = parse_iso8601_to_unix(published_at) {
        if now_unix() - pub_unix < cooling_secs {
            return Ok(None);
        }
    } else {
        // Can't parse date — don't show update
        return Ok(None);
    }

    let html_url = body["html_url"].as_str().unwrap_or_default().to_string();
    let release_body = body["body"].as_str().unwrap_or_default().to_string();

    // Re-verify: if the release URL would 404, don't show
    // (handles case where release was deleted after we fetched it)
    let verify = client
        .head(&html_url)
        .header("User-Agent", "Roxlit-Launcher")
        .send()
        .await;
    if let Ok(resp) = verify {
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
    }

    Ok(Some(UpdateInfo {
        version: remote_version.to_string(),
        published_at: published_at.to_string(),
        html_url,
        body: release_body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iso8601() {
        let ts = parse_iso8601_to_unix("2025-01-01T00:00:00Z").unwrap();
        // 2025-01-01 00:00:00 UTC
        assert_eq!(ts, 1735689600);
    }

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(is_newer_version("1.0.0", "1.0.1"));
        assert!(!is_newer_version("0.2.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(is_newer_version("v0.1.0", "v0.2.0"));
    }
}
