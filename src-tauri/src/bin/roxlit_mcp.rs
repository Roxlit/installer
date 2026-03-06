//! roxlit-mcp: MCP server for Roxlit (Roblox AI development)
//!
//! Implements the Model Context Protocol over stdio (JSON-RPC 2.0).
//! Forwards `run_code` tool calls to the Roxlit launcher HTTP server
//! at 127.0.0.1:19556, which relays them to the Studio plugin.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const LAUNCHER_URL: &str = "http://127.0.0.1:19556";
const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "roxlit";
const SERVER_VERSION: &str = "0.1.0";

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue, // Skip malformed JSON
        };

        // Notifications have no "id" — don't respond
        if request.get("id").is_none() {
            continue;
        }

        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => handle_initialize(id),
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, &params),
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                }
            }),
        };

        let serialized = serde_json::to_string(&response).unwrap();
        let _ = writeln!(stdout, "{}", serialized);
        let _ = stdout.flush();
    }
}

fn handle_initialize(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            }
        }
    })
}

fn handle_tools_list(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "run_code",
                    "description": "Execute Luau code in Roblox Studio. The code runs with PluginSecurity via loadstring(). Use this to inspect game state, verify instances, modify properties, or run quick scripts. Output from print() and warn() is captured and returned. Maximum ~5000 characters of code.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "code": {
                                "type": "string",
                                "description": "Luau code to execute in Studio"
                            }
                        },
                        "required": ["code"]
                    }
                },
                {
                    "name": "get_logs",
                    "description": "Read logs from a Roxlit session. Two sources: 'output' (default) for Studio game output (prints, warns, errors from user scripts — use this to debug the user's game), or 'system' for Roxlit infrastructure logs (rojo, mcp events). Logs contain playtest markers (═══════ PLAYTEST #N START/END ═══════). Use the 'playtest' parameter to filter: 'latest' (default for output) returns only the most recent playtest, 'all' returns everything, or a number for a specific playtest.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project_path": {
                                "type": "string",
                                "description": "Absolute path to the Rojo project directory"
                            },
                            "source": {
                                "type": "string",
                                "enum": ["output", "system"],
                                "description": "Which log to read: 'output' (default) for Studio game output, 'system' for Roxlit infrastructure"
                            },
                            "session": {
                                "type": "string",
                                "description": "Session to read: 'latest' (default) or a session_id from list_sessions"
                            },
                            "playtest": {
                                "type": "string",
                                "description": "Filter by playtest: 'latest' (default for output) returns only the most recent playtest, 'all' returns everything, or a number (e.g. '1', '2') for a specific playtest"
                            },
                            "tail": {
                                "type": "integer",
                                "description": "Only return the last N lines (0 or omitted = all lines). Applied after playtest filtering."
                            }
                        },
                        "required": ["project_path"]
                    }
                },
                {
                    "name": "list_sessions",
                    "description": "List available Roxlit log sessions for a project. Returns session metadata (ID, start time, project name). Use the session_id with get_logs to read a specific session's logs.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project_path": {
                                "type": "string",
                                "description": "Absolute path to the Rojo project directory"
                            }
                        },
                        "required": ["project_path"]
                    }
                },
                {
                    "name": "backup_create",
                    "description": "Create a backup snapshot of the current project state. Uses git stash internally — does NOT modify the working tree (you keep working normally). Use before risky changes, switching debug approaches, or when the user asks. Returns the backup ID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project_path": {
                                "type": "string",
                                "description": "Absolute path to the project directory"
                            },
                            "name": {
                                "type": "string",
                                "description": "Optional descriptive name for the backup (e.g. 'before-ragdoll-fix')"
                            }
                        },
                        "required": ["project_path"]
                    }
                },
                {
                    "name": "backup_list",
                    "description": "List all Roxlit backups for a project. Returns backup IDs, names, and timestamps.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project_path": {
                                "type": "string",
                                "description": "Absolute path to the project directory"
                            }
                        },
                        "required": ["project_path"]
                    }
                },
                {
                    "name": "backup_restore",
                    "description": "Restore the project to a previous backup state. This OVERWRITES current files with the backup's state. The backup is preserved (not deleted) so you can restore again if needed.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project_path": {
                                "type": "string",
                                "description": "Absolute path to the project directory"
                            },
                            "id": {
                                "type": "string",
                                "description": "Backup ID to restore (e.g. 'bk-001')"
                            }
                        },
                        "required": ["project_path", "id"]
                    }
                },
                {
                    "name": "backup_diff",
                    "description": "Show what changed since a backup was created. Returns a git diff between the backup state and the current working tree.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project_path": {
                                "type": "string",
                                "description": "Absolute path to the project directory"
                            },
                            "id": {
                                "type": "string",
                                "description": "Backup ID to diff against (e.g. 'bk-001')"
                            }
                        },
                        "required": ["project_path", "id"]
                    }
                }
            ]
        }
    })
}

fn handle_tools_call(id: Value, params: &Value) -> Value {
    let tool_name = params["name"].as_str().unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    match tool_name {
        "run_code" => tool_run_code(id, &arguments),
        "get_logs" => tool_get_logs(id, &arguments),
        "list_sessions" => tool_list_sessions(id, &arguments),
        "backup_create" => tool_backup_create(id, &arguments),
        "backup_list" => tool_backup_list(id, &arguments),
        "backup_restore" => tool_backup_restore(id, &arguments),
        "backup_diff" => tool_backup_diff(id, &arguments),
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32602,
                "message": format!("Unknown tool: {}", tool_name)
            }
        }),
    }
}

fn tool_run_code(id: Value, arguments: &Value) -> Value {
    let code = match arguments["code"].as_str() {
        Some(c) => c,
        None => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": "Error: 'code' parameter is required" }],
                    "isError": true
                }
            });
        }
    };

    // Build HTTP client with timeout matching the launcher's 30s limit + buffer
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(35))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": format!("Failed to create HTTP client: {}", e) }],
                    "isError": true
                }
            });
        }
    };

    // POST to the launcher's MCP relay endpoint
    let url = format!("{}/mcp/run-code", LAUNCHER_URL);
    let body = json!({ "code": code });

    let response = client.post(&url).json(&body).send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().unwrap_or_default();

            if status.is_success() {
                // Parse the launcher's response: { "success": bool, "result": "..." }
                if let Ok(parsed) = serde_json::from_str::<Value>(&body_text) {
                    let success = parsed["success"].as_bool().unwrap_or(false);
                    let result = parsed["result"]
                        .as_str()
                        .unwrap_or("(no output)")
                        .to_string();

                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": result }],
                            "isError": !success
                        }
                    })
                } else {
                    // Raw text response
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": body_text }],
                            "isError": false
                        }
                    })
                }
            } else {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": format!("Launcher error (HTTP {}): {}", status.as_u16(), body_text) }],
                        "isError": true
                    }
                })
            }
        }
        Err(e) => {
            let msg = if e.is_connect() {
                "Roxlit launcher is not running. Start it with 'Start Development' in the Roxlit app.".to_string()
            } else if e.is_timeout() {
                "Studio plugin did not respond within 30 seconds. Make sure Roblox Studio is open and the Roxlit plugin is installed.".to_string()
            } else {
                format!("Failed to reach Roxlit launcher: {}", e)
            };

            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": msg }],
                    "isError": true
                }
            })
        }
    }
}

fn tool_get_logs(id: Value, arguments: &Value) -> Value {
    let project_path = match arguments["project_path"].as_str() {
        Some(p) => p,
        None => {
            return mcp_error_result(id, "'project_path' parameter is required");
        }
    };

    let source = arguments["source"].as_str().unwrap_or("output");
    let session = arguments["session"].as_str().unwrap_or("latest");
    let playtest_default = if source == "output" { "latest" } else { "all" };
    let playtest = arguments["playtest"].as_str().unwrap_or(playtest_default);
    let tail = arguments["tail"]
        .as_u64()
        .or_else(|| arguments["tail"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0) as usize;

    let logs_dir = std::path::Path::new(project_path)
        .join(".roxlit")
        .join("logs");

    let filename = match source {
        "system" => "system.log",
        _ => "output.log",
    };

    let log_file = if session == "latest" {
        logs_dir.join(filename)
    } else {
        logs_dir.join(format!("{session}-{filename}"))
    };

    if !log_file.exists() {
        return mcp_error_result(id, &format!("No log file found at {}", log_file.display()));
    }

    let content = match std::fs::read_to_string(&log_file) {
        Ok(c) => c,
        Err(e) => {
            return mcp_error_result(id, &format!("Error reading log file: {e}"));
        }
    };

    // Filter by playtest if requested
    let filtered = filter_by_playtest(&content, playtest);

    let output = if tail > 0 {
        let lines: Vec<&str> = filtered.lines().collect();
        let start = lines.len().saturating_sub(tail);
        lines[start..].join("\n")
    } else {
        filtered
    };

    // Truncate if extremely large to avoid flooding the AI context
    let output = if output.len() > 100_000 {
        let truncated = &output[output.len() - 100_000..];
        format!("[...truncated, showing last ~100KB...]\n{truncated}")
    } else {
        output
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": output }],
            "isError": false
        }
    })
}

fn tool_list_sessions(id: Value, arguments: &Value) -> Value {
    let project_path = match arguments["project_path"].as_str() {
        Some(p) => p,
        None => {
            return mcp_error_result(id, "'project_path' parameter is required");
        }
    };

    let logs_dir = std::path::Path::new(project_path)
        .join(".roxlit")
        .join("logs");
    let manifest = logs_dir.join("sessions.jsonl");

    if !manifest.exists() {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": "No sessions found. Start development in the Roxlit launcher first." }],
                "isError": false
            }
        });
    }

    let content = match std::fs::read_to_string(&manifest) {
        Ok(c) => c,
        Err(e) => {
            return mcp_error_result(id, &format!("Error reading sessions manifest: {e}"));
        }
    };

    let mut sessions = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(mut entry) = serde_json::from_str::<Value>(line) {
            if let Some(session_id) = entry["session_id"].as_u64() {
                let rotated_output = logs_dir.join(format!("{session_id}-output.log"));
                let is_current = !rotated_output.exists() && logs_dir.join("output.log").exists();
                entry["is_current"] = json!(is_current);
                entry["has_output"] = json!(is_current || rotated_output.exists());
                entry["has_system"] = json!(
                    (is_current && logs_dir.join("system.log").exists())
                    || logs_dir.join(format!("{session_id}-system.log")).exists()
                );
            }
            sessions.push(entry);
        }
    }

    let output = serde_json::to_string_pretty(&sessions).unwrap_or_default();

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": output }],
            "isError": false
        }
    })
}

/// Filter log content by playtest markers.
/// - "all": return everything as-is
/// - "latest": return from the last PLAYTEST START marker to end of file
/// - "N" (number): return lines between PLAYTEST #N START and PLAYTEST #N END
fn filter_by_playtest(content: &str, playtest: &str) -> String {
    if playtest == "all" {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let marker_pattern = "═══════ PLAYTEST #";

    if playtest == "latest" {
        // Find the last START marker
        let mut last_start = None;
        for (i, line) in lines.iter().enumerate() {
            if line.contains(marker_pattern) && line.contains("START") {
                last_start = Some(i);
            }
        }
        match last_start {
            Some(start) => lines[start..].join("\n"),
            None => content.to_string(), // No markers found, return all
        }
    } else if let Ok(n) = playtest.parse::<u32>() {
        // Find PLAYTEST #N START and END
        let start_marker = format!("PLAYTEST #{n} START");
        let end_marker = format!("PLAYTEST #{n} END");
        let mut start_idx = None;
        let mut end_idx = None;
        for (i, line) in lines.iter().enumerate() {
            if line.contains(&start_marker) {
                start_idx = Some(i);
            }
            if line.contains(&end_marker) {
                end_idx = Some(i);
            }
        }
        match (start_idx, end_idx) {
            (Some(s), Some(e)) => lines[s..=e].join("\n"),
            (Some(s), None) => lines[s..].join("\n"), // Still running
            _ => format!("No playtest #{n} found in logs"),
        }
    } else {
        content.to_string() // Unknown value, return all
    }
}

// ─── Backup tools ───────────────────────────────────────────────────────────

fn tool_backup_create(id: Value, arguments: &Value) -> Value {
    let project_path = match arguments["project_path"].as_str() {
        Some(p) => p,
        None => return mcp_error_result(id, "'project_path' parameter is required"),
    };
    let name = arguments["name"].as_str().unwrap_or("");

    // Ensure git repo exists
    if let Err(e) = ensure_git_repo(project_path) {
        return mcp_error_result(id, &e);
    }

    // Stage everything (including untracked) so stash create captures it
    if let Err(e) = run_git(project_path, &["add", "-A"]) {
        return mcp_error_result(id, &format!("Failed to stage files: {e}"));
    }

    // Create stash without modifying working tree
    let sha = match run_git(project_path, &["stash", "create"]) {
        Ok(s) => s.trim().to_string(),
        Err(e) => return mcp_error_result(id, &format!("Failed to create stash: {e}")),
    };

    if sha.is_empty() {
        // git stash create returns empty when there are no changes vs HEAD
        // Reset index so we don't leave staged files
        let _ = run_git(project_path, &["reset"]);
        return mcp_result(id, "Nothing to backup — no changes detected since last commit.");
    }

    // Reset index (we staged for stash create, undo that)
    let _ = run_git(project_path, &["reset"]);

    // Get next backup ID
    let backup_id = next_backup_id(project_path);
    let label = if name.is_empty() {
        backup_id.clone()
    } else {
        name.to_string()
    };
    let stash_msg = format!("roxlit:{backup_id}:{label}");

    // Store the stash
    if let Err(e) = run_git(project_path, &["stash", "store", "-m", &stash_msg, &sha]) {
        return mcp_error_result(id, &format!("Failed to store stash: {e}"));
    }

    // Write to manifest
    let timestamp = chrono_now();
    let manifest_dir = Path::new(project_path).join(".roxlit");
    let _ = std::fs::create_dir_all(&manifest_dir);
    let manifest_path = manifest_dir.join("backups.jsonl");
    let entry = json!({
        "id": backup_id,
        "name": if name.is_empty() { None } else { Some(name) },
        "timestamp": timestamp,
        "stash_sha": sha,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&manifest_path)
        .ok();
    if let Some(ref mut f) = file {
        let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }

    let display_name = if name.is_empty() {
        backup_id.clone()
    } else {
        format!("{backup_id} ({name})")
    };
    mcp_result(id, &format!("Backup created: {display_name}\nTimestamp: {timestamp}"))
}

fn tool_backup_list(id: Value, arguments: &Value) -> Value {
    let project_path = match arguments["project_path"].as_str() {
        Some(p) => p,
        None => return mcp_error_result(id, "'project_path' parameter is required"),
    };

    let manifest_path = Path::new(project_path)
        .join(".roxlit")
        .join("backups.jsonl");

    if !manifest_path.exists() {
        return mcp_result(id, "No backups found.");
    }

    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(e) => return mcp_error_result(id, &format!("Error reading backups manifest: {e}")),
    };

    let mut backups = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<Value>(line) {
            backups.push(entry);
        }
    }

    if backups.is_empty() {
        return mcp_result(id, "No backups found.");
    }

    let output = serde_json::to_string_pretty(&backups).unwrap_or_default();
    mcp_result(id, &output)
}

fn tool_backup_restore(id: Value, arguments: &Value) -> Value {
    let project_path = match arguments["project_path"].as_str() {
        Some(p) => p,
        None => return mcp_error_result(id, "'project_path' parameter is required"),
    };
    let backup_id = match arguments["id"].as_str() {
        Some(i) => i,
        None => return mcp_error_result(id, "'id' parameter is required"),
    };

    let stash_index = match find_stash_index(project_path, backup_id) {
        Some(i) => i,
        None => {
            return mcp_error_result(
                id,
                &format!("Backup '{backup_id}' not found in git stash list"),
            )
        }
    };

    let stash_ref = format!("stash@{{{stash_index}}}");

    // Auto-backup current state before restoring (so user can undo the restore)
    let mut auto_backup_msg = String::new();
    let _ = run_git(project_path, &["add", "-A"]);
    if let Ok(sha) = run_git(project_path, &["stash", "create"]) {
        let sha = sha.trim().to_string();
        if !sha.is_empty() {
            let auto_id = next_backup_id(project_path);
            let auto_label = format!("pre-restore-{backup_id}");
            let stash_msg = format!("roxlit:{auto_id}:{auto_label}");
            if run_git(project_path, &["stash", "store", "-m", &stash_msg, &sha]).is_ok() {
                // Write to manifest
                let timestamp = chrono_now();
                let manifest_dir = std::path::Path::new(project_path).join(".roxlit");
                let manifest_path = manifest_dir.join("backups.jsonl");
                let entry = json!({
                    "id": auto_id,
                    "name": auto_label,
                    "timestamp": timestamp,
                    "stash_sha": sha,
                    "auto": true,
                });
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&manifest_path)
                {
                    let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
                }
                auto_backup_msg = format!(" Current state saved as '{auto_id}' ({auto_label}) in case you need to undo.");
            }
        }
    }
    let _ = run_git(project_path, &["reset"]);

    // Restore files from stash without dropping it
    if let Err(e) = run_git(project_path, &["checkout", &stash_ref, "--", "."]) {
        return mcp_error_result(id, &format!("Failed to restore backup: {e}"));
    }

    // Unstage everything so working tree is clean but not committed
    let _ = run_git(project_path, &["reset"]);

    mcp_result(
        id,
        &format!("Restored backup '{backup_id}'. Files have been reverted to the backup state.{auto_backup_msg}"),
    )
}

fn tool_backup_diff(id: Value, arguments: &Value) -> Value {
    let project_path = match arguments["project_path"].as_str() {
        Some(p) => p,
        None => return mcp_error_result(id, "'project_path' parameter is required"),
    };
    let backup_id = match arguments["id"].as_str() {
        Some(i) => i,
        None => return mcp_error_result(id, "'id' parameter is required"),
    };

    let stash_index = match find_stash_index(project_path, backup_id) {
        Some(i) => i,
        None => {
            return mcp_error_result(
                id,
                &format!("Backup '{backup_id}' not found in git stash list"),
            )
        }
    };

    let stash_ref = format!("stash@{{{stash_index}}}");

    // Diff: stash (backup state) vs working tree
    // stash_ref is the old state, current working tree is new
    let diff = match run_git(project_path, &["diff", &stash_ref]) {
        Ok(d) => d,
        Err(e) => return mcp_error_result(id, &format!("Failed to compute diff: {e}")),
    };

    if diff.trim().is_empty() {
        return mcp_result(id, "No differences — current state matches the backup.");
    }

    // Truncate if too large
    let output = if diff.len() > 50_000 {
        format!(
            "[...diff truncated, showing last ~50KB...]\n{}",
            &diff[diff.len() - 50_000..]
        )
    } else {
        diff
    };

    mcp_result(id, &output)
}

// ─── Git helpers ────────────────────────────────────────────────────────────

/// Run a git command in the given directory. Returns stdout on success, stderr on error.
fn run_git(path: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // Some git commands (like stash create with no changes) return success with empty output
        // but we also want to handle actual errors
        if stderr.trim().is_empty() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(stderr)
        }
    }
}

/// Ensure the project directory is a git repository. If not, initialize one.
fn ensure_git_repo(path: &str) -> Result<(), String> {
    let git_dir = Path::new(path).join(".git");
    if git_dir.exists() {
        return Ok(());
    }

    // Check if git is installed
    if Command::new("git").arg("--version").output().is_err() {
        return Err("Git is not installed. Backups require git. Download it from: https://git-scm.com".to_string());
    }

    run_git(path, &["init"]).map_err(|e| format!("Failed to init git repo: {e}"))?;
    run_git(path, &["add", "-A"]).map_err(|e| format!("Failed to stage files: {e}"))?;
    run_git(path, &["commit", "-m", "roxlit: initial commit for backups"])
        .map_err(|e| format!("Failed to create initial commit: {e}"))?;

    Ok(())
}

/// Find the stash index for a given backup ID by searching git stash list.
fn find_stash_index(path: &str, backup_id: &str) -> Option<usize> {
    let stash_list = run_git(path, &["stash", "list"]).ok()?;
    let pattern = format!("roxlit:{backup_id}:");

    for line in stash_list.lines() {
        if line.contains(&pattern) {
            // Line format: stash@{N}: ...
            let start = line.find('{')? + 1;
            let end = line.find('}')?;
            return line[start..end].parse().ok();
        }
    }
    None
}

/// Get the next backup ID (bk-001, bk-002, etc.) from the manifest.
fn next_backup_id(path: &str) -> String {
    let manifest_path = Path::new(path).join(".roxlit").join("backups.jsonl");

    let mut max_num: u32 = 0;
    if let Ok(content) = std::fs::read_to_string(&manifest_path) {
        for line in content.lines() {
            if let Ok(entry) = serde_json::from_str::<Value>(line) {
                if let Some(id) = entry["id"].as_str() {
                    if let Some(num_str) = id.strip_prefix("bk-") {
                        if let Ok(n) = num_str.parse::<u32>() {
                            max_num = max_num.max(n);
                        }
                    }
                }
            }
        }
    }

    format!("bk-{:03}", max_num + 1)
}

/// Get current timestamp as ISO 8601 string (cross-platform, no external dependencies).
fn chrono_now() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Calculate year/month/day from days since epoch (1970-01-01)
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    for md in &month_days {
        if remaining < *md { break; }
        remaining -= md;
        m += 1;
    }

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, minutes, seconds)
}

/// Helper for MCP tool success responses.
fn mcp_result(id: Value, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }],
            "isError": false
        }
    })
}

/// Helper for MCP tool error responses.
fn mcp_error_result(id: Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": format!("Error: {message}") }],
            "isError": true
        }
    })
}
