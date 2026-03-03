//! roxlit-mcp: MCP server for Roxlit (Roblox AI development)
//!
//! Implements the Model Context Protocol over stdio (JSON-RPC 2.0).
//! Forwards `run_code` tool calls to the Roxlit launcher HTTP server
//! at 127.0.0.1:19556, which relays them to the Studio plugin.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
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
    let tail = arguments["tail"].as_u64().unwrap_or(0) as usize;

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
