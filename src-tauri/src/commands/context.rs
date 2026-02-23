use crate::error::Result;
use crate::templates;
use std::fs;
use std::path::Path;

/// Generates AI context files tailored to the selected tool.
pub fn generate_context(project_path: &str, ai_tool: &str, project_name: &str) -> Result<()> {
    let root = Path::new(project_path);
    let context_content = templates::ai_context(project_name);

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
