pub mod context_packs;

/// Returns the default.project.json content for Rojo.
pub fn project_json(project_name: &str) -> String {
    format!(
        r#"{{
  "name": "{project_name}",
  "globIgnorePaths": ["**/*.rbxjson"],
  "tree": {{
    "$className": "DataModel",
    "ServerScriptService": {{
      "$className": "ServerScriptService",
      "$ignoreUnknownInstances": true,
      "$path": "scripts/ServerScriptService"
    }},
    "StarterPlayer": {{
      "$className": "StarterPlayer",
      "$ignoreUnknownInstances": true,
      "StarterPlayerScripts": {{
        "$className": "StarterPlayerScripts",
        "$ignoreUnknownInstances": true,
        "$path": "scripts/StarterPlayer/StarterPlayerScripts"
      }},
      "StarterCharacterScripts": {{
        "$className": "StarterCharacterScripts",
        "$ignoreUnknownInstances": true,
        "$path": "scripts/StarterPlayer/StarterCharacterScripts"
      }}
    }},
    "ReplicatedStorage": {{
      "$className": "ReplicatedStorage",
      "$ignoreUnknownInstances": true,
      "$path": "scripts/ReplicatedStorage"
    }},
    "ReplicatedFirst": {{
      "$className": "ReplicatedFirst",
      "$ignoreUnknownInstances": true,
      "$path": "scripts/ReplicatedFirst"
    }},
    "ServerStorage": {{
      "$className": "ServerStorage",
      "$ignoreUnknownInstances": true,
      "$path": "scripts/ServerStorage"
    }},
    "Workspace": {{
      "$className": "Workspace",
      "$ignoreUnknownInstances": true,
      "$path": "scripts/Workspace"
    }},
    "StarterGui": {{
      "$className": "StarterGui",
      "$ignoreUnknownInstances": true,
      "$path": "scripts/StarterGui"
    }},
    "StarterPack": {{
      "$className": "StarterPack",
      "$ignoreUnknownInstances": true,
      "$path": "scripts/StarterPack"
    }}
  }}
}}
"#
    )
}

/// Returns the .luaurc configuration for strict type checking.
pub fn luaurc() -> &'static str {
    r#"{
  "languageMode": "strict"
}
"#
}

/// Returns a minimal server-side starter script.
pub fn server_script() -> &'static str {
    r#"--!strict
-- Server entry point. Code here runs on the Roblox server.

local Players = game:GetService("Players")

Players.PlayerAdded:Connect(function(player: Player)
	print(`{player.Name} joined the game`)
end)

Players.PlayerRemoving:Connect(function(player: Player)
	print(`{player.Name} left the game`)
end)
"#
}

/// Returns a minimal client-side starter script.
pub fn client_script() -> &'static str {
    r#"--!strict
-- Client entry point. Code here runs on each player's device.

local Players = game:GetService("Players")

local player = Players.LocalPlayer
print(`Client started for {player.Name}`)
"#
}

/// Returns a minimal shared module.
pub fn shared_module() -> &'static str {
    r#"--!strict
-- Shared module. Accessible from both server and client via ReplicatedStorage.

local Shared = {}

return Shared
"#
}

/// Returns the rbxsync.json configuration.
/// Only excludes Roblox internals (CoreGui, CorePackages). RbxSync handles instances
/// (.rbxjson) in all other services. Scripts (.luau) are managed by Rojo.
pub fn rbxsync_json(project_name: &str) -> String {
    format!(
        r#"{{
  "name": "{project_name}",
  "tree": "./src",
  "config": {{
    "excludeServices": [
      "CoreGui",
      "CorePackages"
    ],
    "scriptSourceMode": "external"
  }},
  "sync": {{
    "mode": "bidirectional",
    "conflictResolution": "keepLocal",
    "autoSync": true
  }}
}}
"#
    )
}

/// Context version — bump this whenever ai_context() content changes significantly.
/// ensure_ai_context() compares this against the marker in the existing file to decide
/// whether to regenerate. Format: same as Cargo.toml version.
pub const CONTEXT_VERSION: &str = "0.5.0";

/// Marker prefix used to embed the version in the generated context file.
/// Must be a comment that AI tools will ignore but we can parse.
const VERSION_MARKER: &str = "<!-- roxlit-context-version:";

/// Marker that delimits the start of the user's custom notes section.
/// Everything from this marker to the end of the file is preserved on regeneration.
pub const USER_NOTES_MARKER: &str = "## Your Notes";

/// Returns the AI context file content with Roblox/Luau development instructions.
/// This is the same content regardless of AI tool — only the filename changes.
pub fn ai_context(project_name: &str, mcp_available: bool) -> String {
    let rbxsync_section = if mcp_available {
        r#"
## RbxSync (Instance Sync + MCP)

This project uses RbxSync alongside Rojo. While Rojo syncs Luau scripts, RbxSync provides sync for all instances (Parts, GUIs, Models, etc.) and an **MCP server** for real-time interaction with Studio.

**IMPORTANT: You have DIRECT ACCESS to Roblox Studio via MCP tools.** You can read instances, create objects, set properties, execute Luau code, and run tests — all in real-time. Do NOT tell the user "I can't see your screen" or ask them to check things manually when you can use MCP tools to check yourself.

### How it works

- **Rojo**: Syncs `.luau` scripts in ALL services (real-time, filesystem → Studio). One-directional.
- **RbxSync**: Periodically extracts instances from Studio to local `.rbxjson` files (cache/backup). Also syncs local `.rbxjson` edits back to Studio.
- **MCP**: Direct real-time connection to Studio. **This is the source of truth for instances** — always use MCP to read and write instances.

### IMPORTANT: Activating the RbxSync Plugin

The RbxSync plugin must be activated **once per Studio session**:
1. If this is the first time after installation, **restart Roblox Studio** so it loads the new plugin
2. Go to the Plugins tab
3. Find the RbxSync plugin and click "Sync" or "Connect"
4. The plugin connects to `rbxsync serve` (which Roxlit starts automatically in the project directory)

If the user reports that the RbxSync plugin doesn't appear, they need to restart Studio. Plugins are only loaded when Studio starts.
If the user reports that instance sync isn't working, remind them to activate the plugin in Studio. This is required every time Studio is opened.

### MCP Tools — Primary Way to Work with Instances

**Writing:**
- **Scripts** → always edit local `.luau` files in `scripts/` (Rojo syncs to Studio in real-time)
- **Instances** → use MCP tools (`create_instance`, `set_property`, `delete_instance`, etc.) — changes apply instantly in Studio
- **Never edit local `.rbxjson` files directly** — they are auto-generated cache and will be overwritten by the next Studio extract
- **Never create files in `src/`** — rbxsync overwrites it periodically

**Reading:**
- **Specific instance** → use MCP `get_instance` (always fresh, real-time from Studio)
- **Explore project structure** → read local `.rbxjson` files with Glob + Read (good for browsing, but may be up to 30s stale)
- **Before any write** → always use MCP `get_instance` to confirm current state

**Debugging & testing — USE THESE instead of asking the user to check things manually:**
- `run_code` — execute Luau in Studio (print values, inspect runtime state, verify scripts loaded). If the user asks "is it working?", run code to CHECK instead of saying "I can't see your screen"
- `run_test` — run tests inside Studio
- `get_instance` — verify a script or instance exists in Studio's DataModel
- Example: to check if a script is printing, use `run_code` with a test print and read the output

### Local `.rbxjson` Files — Cache, Not Source of Truth

RbxSync periodically extracts instances from Studio to local `.rbxjson` files. These files are useful for:
- **Exploring** the project structure (Glob + Read)
- **Git history** — track what changed over time
- **Backups** — recover from mistakes

**WARNING**: Local `.rbxjson` files may be up to 30s stale. Studio is the source of truth. If the user edits something in Studio, the local files won't reflect it immediately. Always use MCP `get_instance` when you need current data.

### RbxSync File Structure

Instances are cached under `src/` alongside scripts, mirroring the Roblox DataModel hierarchy:

```
src/
  Workspace/
    SpawnLocation/          ← Folder name = instance name
      _meta.rbxjson         ← Class + properties of SpawnLocation
      Decal.rbxjson         ← Child instance (simple, no children of its own)
    MyModel/
      _meta.rbxjson         ← Class + properties of MyModel
      Part1.rbxjson         ← Child Part
      Part2/                ← Child with its own children → becomes a folder
        _meta.rbxjson
        SurfaceGui.rbxjson
  Lighting/
    _meta.rbxjson           ← Lighting service properties
    Atmosphere.rbxjson      ← Post-processing effect
```

Rules:
- **Instances with children** → folder with `_meta.rbxjson` inside (contains class + properties)
- **Leaf instances** (no children) → single `.rbxjson` file
- **`_meta.rbxjson`** contains `""ClassName""` and `""Properties""`
- To find an instance, **search by folder/file name** in `src/`
- These files are **read-only cache** — use MCP to modify instances
- Rojo ignores `.rbxjson` files (`globIgnorePaths`), so both file types coexist in `src/` without conflicts

### Sync Workflow

**Editing scripts (.luau)**: Edit local files in `scripts/`. Rojo syncs to Studio in real-time.

**Editing instances**: Use MCP tools. Changes apply instantly in Studio. Local `.rbxjson` files in `src/` update on the next auto-extract (~30s).

**Reading instances**: Use MCP `get_instance` for real-time data. Use local `.rbxjson` files only for exploration/browsing.

**File ownership**:
- Rojo owns `.luau` files in `scripts/` — always edit locally
- MCP owns instance editing — always use MCP tools, never edit `.rbxjson` directly
- `src/` is rbxsync's domain — never create or edit files there manually

### Backups

Roxlit automatically backs up `.rbxjson` files before each Studio extract:
- Location: `.roxlit/backups/<timestamp>/`
- Each backup preserves the full `src/` directory structure
- Maximum 20 backups retained (oldest are deleted)
- Each backup has a `manifest.json` listing the files

If the user reports lost changes to instances, check `.roxlit/backups/` for recent backups. You can diff the backup files against current files to find what changed.

"#
    } else {
        r#"
## RbxSync (Instance Sync)

This project uses RbxSync alongside Rojo. While Rojo syncs Luau scripts, RbxSync provides sync for all instances (Parts, GUIs, Models, etc.).

### How it works

- **Rojo**: Syncs `.luau` scripts in ALL services (real-time, filesystem → Studio). One-directional.
- **RbxSync**: Periodically extracts instances from Studio to local `.rbxjson` files. Also syncs local `.rbxjson` edits back to Studio.
- Both coexist: Rojo manages `.luau` files, RbxSync manages `.rbxjson` files. No conflicts.

### IMPORTANT: Activating the RbxSync Plugin

The RbxSync plugin must be activated **once per Studio session**:
1. If this is the first time after installation, **restart Roblox Studio** so it loads the new plugin
2. Go to the Plugins tab
3. Find the RbxSync plugin and click "Sync" or "Connect"
4. The plugin connects to `rbxsync serve` (which Roxlit starts automatically in the project directory)

If the user reports that the RbxSync plugin doesn't appear, they need to restart Studio. Plugins are only loaded when Studio starts.
If the user reports that instance sync isn't working, remind them to activate the plugin in Studio. This is required every time Studio is opened.

### Working with Instances

- Instance properties are stored as `.rbxjson` files in the project directory
- You can edit these files to change Part positions, GUI layouts, etc.
- Changes sync automatically to Studio when RbxSync is running (~2s)
- **This project is designed for AI-first development** — instance editing should be done through AI tools, not by manually editing `.rbxjson` files

**WARNING — Studio → local sync is not real-time**: Local `.rbxjson` files are updated by periodic Studio extracts (~30s). If the user edits something in Studio (moves a Part, changes properties, etc.), the local files won't reflect those changes immediately. Before editing a `.rbxjson` file, ask the user to confirm the current state in Studio or trigger a manual extract from the Roxlit launcher.

### RbxSync File Structure

Instances are stored under `src/` alongside scripts, mirroring the Roblox DataModel hierarchy:

```
src/
  Workspace/
    SpawnLocation/          ← Folder name = instance name
      _meta.rbxjson         ← Class + properties of SpawnLocation
      Decal.rbxjson         ← Child instance (simple, no children of its own)
    MyModel/
      _meta.rbxjson         ← Class + properties of MyModel
      Part1.rbxjson         ← Child Part
      Part2/                ← Child with its own children → becomes a folder
        _meta.rbxjson
        SurfaceGui.rbxjson
  Lighting/
    _meta.rbxjson           ← Lighting service properties
    Atmosphere.rbxjson      ← Post-processing effect
```

Rules:
- **Instances with children** → folder with `_meta.rbxjson` inside (contains class + properties)
- **Leaf instances** (no children) → single `.rbxjson` file
- **`_meta.rbxjson`** contains `""ClassName""` and `""Properties""` — edit properties here to change the instance
- To find an instance, **search by folder/file name** in `src/`
- Changes to `.rbxjson` files sync to Studio automatically when RbxSync is running
- Rojo ignores `.rbxjson` files (`globIgnorePaths`), so both file types coexist in `src/` without conflicts

### Sync Workflow

**Editing scripts (.luau)**: Edit local files in `scripts/`. Rojo syncs to Studio in real-time.

**Editing instances (.rbxjson)**: Edit local files in `src/`. RbxSync syncs to Studio (~2s). **But beware**: local files may be up to 30s stale. Before editing, ask the user to confirm the current state in Studio or trigger a manual extract.

**File ownership**:
- Rojo owns `.luau` files in `scripts/` — always edit locally
- RbxSync owns `.rbxjson` files in `src/` — edit locally, but verify state before editing
- `src/` is rbxsync's domain — never create or edit `.luau` files there

### Backups

Roxlit automatically backs up `.rbxjson` files before each Studio extract:
- Location: `.roxlit/backups/<timestamp>/`
- Each backup preserves the full `src/` directory structure
- Maximum 20 backups retained (oldest are deleted)
- Each backup has a `manifest.json` listing the files

If the user reports lost changes to instances, check `.roxlit/backups/` for recent backups. You can diff the backup files against current files to find what changed.

"#
    };

    format!(
        r#"{VERSION_MARKER} {CONTEXT_VERSION} -->
# {project_name}

Roblox game project using Rojo for file syncing. Write Luau code in `scripts/` and Rojo syncs it to Roblox Studio in real time.

## Tech Stack

- **Language**: Luau (Roblox's typed Lua dialect)
- **Sync tool**: Rojo (filesystem <-> Roblox DataModel)
- **Instance sync**: RbxSync (bidirectional sync for Parts, GUIs, etc.)
- **Type checking**: Strict mode (`--!strict`)

## Project Structure

```
scripts/                                ← Luau scripts (synced by Rojo to Studio)
  ServerScriptService/                  → Server scripts
  StarterPlayer/
    StarterPlayerScripts/               → Client scripts (player join)
    StarterCharacterScripts/            → Client scripts (character spawn)
  ReplicatedStorage/                    → Shared modules (server + client)
  ReplicatedFirst/                      → Early client loading scripts
  ServerStorage/                        → Server-only modules and assets
  Workspace/                            → 3D world + scripts in Parts/Models
  StarterGui/                           → GUI templates + LocalScripts
  StarterPack/                          → Starter tools + scripts
src/                                    ← Instance cache (.rbxjson, managed by RbxSync)
  Workspace/
  Lighting/
  ...
```

**Creating scripts**: Create `.luau` files in `scripts/` — Rojo syncs them to Studio in real-time. NEVER create a `.rbxjson` file for a script — scripts are always `.luau`. NEVER create files in `src/` — rbxsync overwrites it periodically.

**Instances** (Parts, GUIs, Models, etc.): Managed by RbxSync. Local `.rbxjson` files in `src/` are a **read-only cache** from RbxSync — they are periodically extracted (~30s) and may be stale. Rojo ignores `.rbxjson` files so both coexist. See the RbxSync section below for how to read and edit instances correctly.

## File Naming Conventions

- `*.server.luau` → Script (server-side)
- `*.client.luau` → LocalScript (client-side)
- `*.luau` → ModuleScript
- `init.luau` / `init.server.luau` / `init.client.luau` in a folder → the folder itself becomes the script

## Luau Coding Standards

- Always use `local` for variable declarations
- Use type annotations: `local health: number = 100`
- Access services with `game:GetService("ServiceName")`
- Require modules relatively: `require(script.Parent.ModuleName)`
- Prefer `task.wait()` over `wait()`, `task.spawn()` over `spawn()`
- Add `--!strict` at the top of every file

## Key Rules

- **Never use `wait()`** → use `task.wait()` instead
- **Never trust the client** → validate everything on the server
- **Never store secrets in ReplicatedStorage** → clients can read it
- **Never call DataStore without `pcall()`** → DataStore calls can fail
- **Testing**: Scripts don't run in edit mode. Press Play (F5) for server + client, Run (F8) for server-only. Read `.roxlit/context/studio-ui.md` before giving Studio UI directions.
{rbxsync_section}## Roblox Context Packs

This project includes curated Roblox documentation in `.roxlit/context/`. Before writing code that involves a specific system, **read the relevant file**:

- `.roxlit/context/datastore.md` — DataStoreService: throttling limits, session locking, retry patterns
- `.roxlit/context/remote-events.md` — RemoteEvent/Function: server validation, rate limiting, type checking
- `.roxlit/context/player-lifecycle.md` — PlayerAdded, CharacterAdded, respawn, death handling
- `.roxlit/context/workspace-physics.md` — Parts, CFrame operations, raycasting, collision groups
- `.roxlit/context/replication.md` — What replicates, FilteringEnabled, client vs server
- `.roxlit/context/services-reference.md` — Service properties, enums, valid ranges
- `.roxlit/context/studio-ui.md` — **READ THIS before giving ANY Studio UI directions**: where panels are (Output, Explorer, etc.), mezzanine/toolbar layout, testing modes (F5/F8), troubleshooting ("my script isn't running")

Read `.roxlit/context/index.md` for an overview of all available packs.

## Roxlit Launcher

This project was set up with Roxlit. The Roxlit launcher manages Rojo and RbxSync processes automatically.

- **Copy logs**: If there are errors, the user can click "Copy All" in the Roxlit launcher terminal to copy all logs. They can then paste them here for you to diagnose.
- **Do NOT remove or modify the Roxlit-generated sections above.** They are auto-updated by Roxlit when new versions are available.

{USER_NOTES_MARKER}

Add your own project-specific notes, rules, or instructions below this line. Roxlit will preserve this section when updating the context above.

"#
    )
}
