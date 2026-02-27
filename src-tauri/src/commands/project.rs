use crate::error::Result;
use crate::templates;
use std::fs;
use std::path::Path;

/// Creates the standard Rojo project structure at the given path.
pub fn create_project(project_path: &str, project_name: &str) -> Result<()> {
    let root = Path::new(project_path);

    // Create directory tree (all services where Roblox allows scripts and instances)
    fs::create_dir_all(root.join("src").join("ServerScriptService"))?;
    fs::create_dir_all(root.join("src").join("StarterPlayer").join("StarterPlayerScripts"))?;
    fs::create_dir_all(root.join("src").join("StarterPlayer").join("StarterCharacterScripts"))?;
    fs::create_dir_all(root.join("src").join("ReplicatedStorage"))?;
    fs::create_dir_all(root.join("src").join("ReplicatedFirst"))?;
    fs::create_dir_all(root.join("src").join("ServerStorage"))?;
    fs::create_dir_all(root.join("src").join("Workspace"))?;
    fs::create_dir_all(root.join("src").join("StarterGui"))?;
    fs::create_dir_all(root.join("src").join("StarterPack"))?;

    // Aftman tool manifest (tells aftman which rojo version to use)
    fs::write(
        root.join("aftman.toml"),
        "[tools]\nrojo = \"rojo-rbx/rojo@7.4.4\"\n",
    )?;

    // Rojo project config
    fs::write(
        root.join("default.project.json"),
        templates::project_json(project_name),
    )?;

    // Luau strict-mode config
    fs::write(root.join(".luaurc"), templates::luaurc())?;

    // Starter scripts so the project isn't empty
    fs::write(
        root.join("src").join("ServerScriptService").join("main.server.luau"),
        templates::server_script(),
    )?;

    fs::write(
        root.join("src").join("StarterPlayer").join("StarterPlayerScripts").join("main.client.luau"),
        templates::client_script(),
    )?;

    fs::write(
        root.join("src").join("ReplicatedStorage").join("Shared.luau"),
        templates::shared_module(),
    )?;

    // Debug module — studio-only logging (silent in production)
    fs::write(
        root.join("src").join("ReplicatedStorage").join("Debug.luau"),
        templates::debug_module(),
    )?;

    // RbxSync config — exclude services Rojo handles, sync only instances
    fs::write(
        root.join("rbxsync.json"),
        templates::rbxsync_json(project_name),
    )?;

    // RbxSync ignore file
    fs::write(
        root.join(".rbxsyncignore"),
        ".git/\n.roxlit/\n.claude/\n.cursor/\n.vscode/\n.windsurf/\n.github/\nnode_modules/\nsrc/\n",
    )?;

    Ok(())
}
