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
