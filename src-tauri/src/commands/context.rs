use crate::error::Result;
use crate::templates;
use std::fs;
use std::path::Path;

/// Generates AI context files tailored to the selected tool.
pub fn generate_context(project_path: &str, ai_tool: &str, project_name: &str) -> Result<()> {
    let root = Path::new(project_path);

    // Check if MCP binary exists to include RbxSync MCP info
    let mcp_available = dirs::home_dir()
        .map(|h| h.join(".roxlit").join("bin").join("rbxsync-mcp").exists())
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

    // Configure MCP if the binary is available
    if mcp_available {
        configure_mcp(root, ai_tool)?;
    }

    Ok(())
}

/// Writes MCP server configuration for the selected AI tool.
fn configure_mcp(project_root: &Path, ai_tool: &str) -> Result<()> {
    let mcp_binary = dirs::home_dir()
        .map(|h| h.join(".roxlit").join("bin").join("rbxsync-mcp"))
        .ok_or_else(|| crate::error::InstallerError::Custom("Cannot find home directory".into()))?;

    let mcp_path_str = mcp_binary.to_string_lossy().to_string();

    match ai_tool {
        "claude" => {
            let dir = project_root.join(".claude");
            fs::create_dir_all(&dir)?;
            let config_path = dir.join("settings.json");
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
            let dir = project_root.join(".windsurf");
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
        _ => {
            // Generic fallback
            let config_path = project_root.join("mcp-config.json");
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
