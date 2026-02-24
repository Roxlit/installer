use crate::error::Result;
use crate::templates;
use std::fs;
use std::path::Path;

/// Creates the standard Rojo project structure at the given path.
pub fn create_project(project_path: &str, project_name: &str) -> Result<()> {
    let root = Path::new(project_path);

    // Create directory tree
    fs::create_dir_all(root.join("src").join("server"))?;
    fs::create_dir_all(root.join("src").join("client"))?;
    fs::create_dir_all(root.join("src").join("shared"))?;

    // Rojo project config
    fs::write(
        root.join("default.project.json"),
        templates::project_json(project_name),
    )?;

    // Luau strict-mode config
    fs::write(root.join(".luaurc"), templates::luaurc())?;

    // Starter scripts so the project isn't empty
    fs::write(
        root.join("src").join("server").join("main.server.luau"),
        templates::server_script(),
    )?;

    fs::write(
        root.join("src").join("client").join("main.client.luau"),
        templates::client_script(),
    )?;

    fs::write(
        root.join("src").join("shared").join("Shared.luau"),
        templates::shared_module(),
    )?;

    // RbxSync config — exclude services Rojo handles, sync only instances
    fs::write(
        root.join("rbxsync.json"),
        templates::rbxsync_json(project_name),
    )?;

    // RbxSync ignore file
    fs::write(
        root.join(".rbxsyncignore"),
        ".git/\n.roxlit/\n.claude/\n.cursor/\n.vscode/\n.windsurf/\n.github/\nnode_modules/\n",
    )?;

    Ok(())
}
