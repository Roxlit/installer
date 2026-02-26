use crate::error::Result;
use crate::templates;
use std::fs;
use std::path::Path;

/// Generates AI context files tailored to the selected tool.
pub fn generate_context(project_path: &str, ai_tool: &str, project_name: &str) -> Result<()> {
    let root = Path::new(project_path);

    // Check if MCP binary exists to include RbxSync MCP info
    let mcp_bin_name = if cfg!(target_os = "windows") { "rbxsync-mcp.exe" } else { "rbxsync-mcp" };
    let mcp_available = dirs::home_dir()
        .map(|h| h.join(".roxlit").join("bin").join(mcp_bin_name).exists())
        .unwrap_or(false);

    let context_content = templates::ai_context(project_name, mcp_available);

    match ai_tool {
        "claude" => {
            fs::write(root.join("CLAUDE.md"), &context_content)?;
        }
        "cursor" => {
            fs::write(root.join(".cursorrules"), &context_content)?;
        }
        "windsurf" => {
            fs::write(root.join(".windsurfrules"), &context_content)?;
        }
        "vscode" => {
            // Copilot reads instructions from .github/copilot-instructions.md
            fs::create_dir_all(root.join(".github"))?;
            fs::write(
                root.join(".github").join("copilot-instructions.md"),
                &context_content,
            )?;
        }
        _ => {
            // Generic fallback for unknown tools
            fs::write(root.join("AI-CONTEXT.md"), &context_content)?;
        }
    }

    // Write context packs to .roxlit/context/
    write_context_packs(root)?;

    // Configure MCP if the binary is available
    if mcp_available {
        configure_mcp(root, ai_tool)?;
    }

    Ok(())
}

/// Writes curated Roblox documentation packs to `.roxlit/context/`.
fn write_context_packs(project_root: &Path) -> Result<()> {
    let context_dir = project_root.join(".roxlit").join("context");
    fs::create_dir_all(&context_dir)?;

    fs::write(context_dir.join("index.md"), templates::context_packs::index())?;
    fs::write(context_dir.join("datastore.md"), templates::context_packs::datastore())?;
    fs::write(context_dir.join("remote-events.md"), templates::context_packs::remote_events())?;
    fs::write(context_dir.join("player-lifecycle.md"), templates::context_packs::player_lifecycle())?;
    fs::write(context_dir.join("workspace-physics.md"), templates::context_packs::workspace_physics())?;
    fs::write(context_dir.join("replication.md"), templates::context_packs::replication())?;
    fs::write(context_dir.join("services-reference.md"), templates::context_packs::services_reference())?;
    fs::write(context_dir.join("studio-ui.md"), templates::context_packs::studio_ui())?;

    Ok(())
}

/// Writes MCP server configuration for the selected AI tool.
pub fn configure_mcp(project_root: &Path, ai_tool: &str) -> Result<()> {
    let mcp_bin_name = if cfg!(target_os = "windows") { "rbxsync-mcp.exe" } else { "rbxsync-mcp" };
    let mcp_binary = dirs::home_dir()
        .map(|h| h.join(".roxlit").join("bin").join(mcp_bin_name))
        .ok_or_else(|| crate::error::InstallerError::Custom("Cannot find home directory".into()))?;

    let mcp_path_str = mcp_binary.to_string_lossy().to_string();

    // Claude Code uses .mcp.json at project root for MCP config.
    // Cursor, VS Code, and Windsurf use tool-specific directories.
    match ai_tool {
        "claude" => {
            let config_path = project_root.join(".mcp.json");
            let config = format!(
                r#"{{
  "mcpServers": {{
    "rbxsync": {{
      "type": "stdio",
      "command": "{mcp_path_str}"
    }}
  }}
}}
"#
            );
            fs::write(config_path, config)?;
        }
        "cursor" => {
            let dir = project_root.join(".cursor");
            fs::create_dir_all(&dir)?;
            let config_path = dir.join("mcp.json");
            let config = format!(
                r#"{{
  "mcpServers": {{
    "rbxsync": {{
      "command": "{mcp_path_str}"
    }}
  }}
}}
"#
            );
            fs::write(config_path, config)?;
        }
        "vscode" => {
            let dir = project_root.join(".vscode");
            fs::create_dir_all(&dir)?;
            let config_path = dir.join("mcp.json");
            let config = format!(
                r#"{{
  "servers": {{
    "rbxsync": {{
      "type": "stdio",
      "command": "{mcp_path_str}"
    }}
  }}
}}
"#
            );
            fs::write(config_path, config)?;
        }
        "windsurf" => {
            // Windsurf uses a global config at ~/.codeium/windsurf/mcp_config.json
            if let Some(home) = dirs::home_dir() {
                let dir = home.join(".codeium").join("windsurf");
                fs::create_dir_all(&dir)?;
                let config_path = dir.join("mcp_config.json");
                // Don't overwrite if it already exists (user may have other servers)
                if !config_path.exists() {
                    let config = format!(
                        r#"{{
  "mcpServers": {{
    "rbxsync": {{
      "command": "{mcp_path_str}"
    }}
  }}
}}
"#
                    );
                    fs::write(config_path, config)?;
                }
            }
        }
        _ => {
            // Generic fallback — use .mcp.json (same as Claude Code)
            let config_path = project_root.join(".mcp.json");
            let config = format!(
                r#"{{
  "mcpServers": {{
    "rbxsync": {{
      "type": "stdio",
      "command": "{mcp_path_str}"
    }}
  }}
}}
"#
            );
            fs::write(config_path, config)?;
        }
    }

    Ok(())
}

/// Returns a human-readable name for the AI tool ID.
pub fn tool_display_name(ai_tool: &str) -> &str {
    match ai_tool {
        "claude" => "Claude Code",
        "cursor" => "Cursor",
        "vscode" => "VS Code + Copilot",
        "windsurf" => "Windsurf",
        _ => "your AI tool",
    }
}
