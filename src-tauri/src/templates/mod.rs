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
      "$path": "src/ServerScriptService"
    }},
    "StarterPlayer": {{
      "$className": "StarterPlayer",
      "$ignoreUnknownInstances": true,
      "StarterPlayerScripts": {{
        "$className": "StarterPlayerScripts",
        "$ignoreUnknownInstances": true,
        "$path": "src/StarterPlayer/StarterPlayerScripts"
      }},
      "StarterCharacterScripts": {{
        "$className": "StarterCharacterScripts",
        "$ignoreUnknownInstances": true,
        "$path": "src/StarterPlayer/StarterCharacterScripts"
      }}
    }},
    "ReplicatedStorage": {{
      "$className": "ReplicatedStorage",
      "$ignoreUnknownInstances": true,
      "$path": "src/ReplicatedStorage"
    }},
    "ReplicatedFirst": {{
      "$className": "ReplicatedFirst",
      "$ignoreUnknownInstances": true,
      "$path": "src/ReplicatedFirst"
    }},
    "ServerStorage": {{
      "$className": "ServerStorage",
      "$ignoreUnknownInstances": true,
      "$path": "src/ServerStorage"
    }},
    "Workspace": {{
      "$className": "Workspace",
      "$ignoreUnknownInstances": true,
      "$path": "src/Workspace"
    }},
    "StarterGui": {{
      "$className": "StarterGui",
      "$ignoreUnknownInstances": true,
      "$path": "src/StarterGui"
    }},
    "StarterPack": {{
      "$className": "StarterPack",
      "$ignoreUnknownInstances": true,
      "$path": "src/StarterPack"
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
  "tree": "./instances",
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

/// Returns the AI context file content with Roblox/Luau development instructions.
/// This is the same content regardless of AI tool — only the filename changes.
pub fn ai_context(project_name: &str, mcp_available: bool) -> String {
    let rbxsync_section = if mcp_available {
        r#"
## RbxSync (Instance Sync + MCP)

This project uses RbxSync alongside Rojo. While Rojo syncs Luau scripts, RbxSync provides sync for all instances (Parts, GUIs, Models, etc.) and an **MCP server** for real-time interaction with Studio.

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
- **Scripts** → always edit local `.luau` files (Rojo syncs to Studio in real-time)
- **Instances** → use MCP tools (`create_instance`, `set_property`, `delete_instance`, etc.) — changes apply instantly in Studio
- **Never edit local `.rbxjson` files directly** — they are auto-generated cache and will be overwritten by the next Studio extract

**Reading:**
- **Specific instance** → use MCP `get_instance` (always fresh, real-time from Studio)
- **Explore project structure** → read local `.rbxjson` files with Glob + Read (good for browsing, but may be up to 30s stale)
- **Before any write** → always use MCP `get_instance` to confirm current state

**Debugging & testing:**
- `run_code` — execute Luau in Studio for debugging (print values, inspect runtime state)
- `run_test` — run tests inside Studio

### Local `.rbxjson` Files — Cache, Not Source of Truth

RbxSync periodically extracts instances from Studio to local `.rbxjson` files. These files are useful for:
- **Exploring** the project structure (Glob + Read)
- **Git history** — track what changed over time
- **Backups** — recover from mistakes

**WARNING**: Local `.rbxjson` files may be up to 30s stale. Studio is the source of truth. If the user edits something in Studio, the local files won't reflect it immediately. Always use MCP `get_instance` when you need current data.

### RbxSync File Structure

Instances are cached under `instances/` mirroring the Roblox DataModel hierarchy:

```
instances/
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
- To find an instance, **search by folder/file name** in `instances/`
- These files are **read-only cache** — use MCP to modify instances

### Sync Workflow

**Editing scripts (.luau)**: Edit local files in `src/`. Rojo syncs to Studio in real-time.

**Editing instances**: Use MCP tools. Changes apply instantly in Studio. Local `.rbxjson` files in `instances/` update on the next auto-extract (~30s).

**Reading instances**: Use MCP `get_instance` for real-time data. Use local `.rbxjson` files only for exploration/browsing.

**File ownership**:
- Rojo owns `.luau` files in `src/` — always edit locally
- MCP owns instance editing — always use MCP tools, never edit `.rbxjson` directly

### Backups

Roxlit automatically backs up `.rbxjson` files before each Studio extract:
- Location: `.roxlit/backups/<timestamp>/`
- Each backup preserves the full `instances/` directory structure
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

Instances are stored under `instances/` mirroring the Roblox DataModel hierarchy:

```
instances/
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
- To find an instance, **search by folder/file name** in `instances/`
- Changes to `.rbxjson` files sync to Studio automatically when RbxSync is running

### Sync Workflow

**Editing scripts (.luau)**: Edit local files in `src/`. Rojo syncs to Studio in real-time.

**Editing instances (.rbxjson)**: Edit local files in `instances/`. RbxSync syncs to Studio (~2s). **But beware**: local files may be up to 30s stale. Before editing, ask the user to confirm the current state in Studio or trigger a manual extract.

**File ownership**:
- Rojo owns `.luau` files in `src/` — always edit locally
- RbxSync owns `.rbxjson` files in `instances/` — edit locally, but verify state before editing

### Backups

Roxlit automatically backs up `.rbxjson` files before each Studio extract:
- Location: `.roxlit/backups/<timestamp>/`
- Each backup preserves the full `instances/` directory structure
- Maximum 20 backups retained (oldest are deleted)
- Each backup has a `manifest.json` listing the files

If the user reports lost changes to instances, check `.roxlit/backups/` for recent backups. You can diff the backup files against current files to find what changed.

"#
    };

    format!(
        r#"# {project_name}

Roblox game project using Rojo for file syncing. Write Luau code in `src/` and Rojo syncs it to Roblox Studio in real time.

## Tech Stack

- **Language**: Luau (Roblox's typed Lua dialect)
- **Sync tool**: Rojo (filesystem <-> Roblox DataModel)
- **Instance sync**: RbxSync (bidirectional sync for Parts, GUIs, etc.)
- **Type checking**: Strict mode (`--!strict`)

## Project Structure

```
src/
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
```

**Creating scripts**: Always create `.luau` files. Rojo syncs them to Studio in real-time regardless of which service they're in. NEVER create a `.rbxjson` file for a script — scripts are always `.luau`.

**Instances** (Parts, GUIs, Models, etc.): Managed by RbxSync. Local `.rbxjson` files in `instances/` are a **cache** of what's in Studio — they are periodically extracted (~30s) and may be stale. See the RbxSync section below for how to read and edit instances correctly.

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

## Common Services

| Service | Purpose |
|---------|---------|
| `Players` | Player join/leave, character access |
| `ReplicatedStorage` | Shared modules, RemoteEvents |
| `ServerScriptService` | Server-only scripts |
| `ServerStorage` | Server-only assets and data |
| `Workspace` | The 3D world (parts, models, terrain) |
| `DataStoreService` | Persistent player data (server only) |
| `RunService` | Game loop events (Heartbeat, RenderStepped) |
| `UserInputService` | Keyboard, mouse, touch input (client only) |
| `TweenService` | Smooth property animations |

## RemoteEvent Pattern (Client ↔ Server Communication)

```luau
-- SERVER: Create and listen
local remote = Instance.new("RemoteEvent")
remote.Name = "DamagePlayer"
remote.Parent = game.ReplicatedStorage

remote.OnServerEvent:Connect(function(player: Player, targetId: number, damage: number)
    -- Always validate on the server — never trust the client
    if damage > 0 and damage <= 100 then
        -- Apply damage
    end
end)

-- CLIENT: Fire to server
local remote = game.ReplicatedStorage:WaitForChild("DamagePlayer")
remote:FireServer(targetId, 25)
```

## DataStore Pattern (Saving Player Data)

```luau
local DataStoreService = game:GetService("DataStoreService")
local store = DataStoreService:GetDataStore("PlayerData")

local function loadData(player: Player): {{ [string]: any }}?
    local ok, data = pcall(function()
        return store:GetAsync("Player_" .. player.UserId)
    end)
    if ok then
        return data
    end
    warn(`Failed to load data for {{player.Name}}`)
    return nil
end

local function saveData(player: Player, data: {{ [string]: any }})
    local ok, err = pcall(function()
        store:SetAsync("Player_" .. player.UserId, data)
    end)
    if not ok then
        warn(`Failed to save data for {{player.Name}}: {{err}}`)
    end
end
```

## Anti-Patterns — Do NOT Do These

- **Never use `wait()`** → use `task.wait()` instead
- **Never put LocalScripts in ServerScriptService** → they won't run
- **Never trust the client** → validate everything on the server
- **Never store secrets in ReplicatedStorage** → clients can read it
- **Never use string paths like `game.Workspace.Part`** → use `:FindFirstChild()` or `:WaitForChild()`
- **Avoid `while true do` without `task.wait()`** → freezes the thread
- **Never call DataStore without `pcall()`** → DataStore calls can fail

## Important Concepts

1. **Client-server model**: The server is authoritative. Properties set on the server replicate to clients, not vice versa.
2. **Filtering Enabled**: Clients cannot directly modify the server's state. Use RemoteEvents/RemoteFunctions for communication.
3. **Player lifecycle**: Use `Players.PlayerAdded` for setup and `Players.PlayerRemoving` for cleanup/saving.
4. **Testing**: Use Studio's "Run" mode (server + client) for networking tests, not "Play Solo".
{rbxsync_section}## Roblox Context Packs

This project includes curated Roblox documentation in `.roxlit/context/`. Before writing code that involves a specific system, **read the relevant file**:

- `.roxlit/context/datastore.md` — DataStoreService: throttling limits, session locking, retry patterns
- `.roxlit/context/remote-events.md` — RemoteEvent/Function: server validation, rate limiting, type checking
- `.roxlit/context/player-lifecycle.md` — PlayerAdded, CharacterAdded, respawn, death handling
- `.roxlit/context/workspace-physics.md` — Parts, CFrame operations, raycasting, collision groups
- `.roxlit/context/replication.md` — What replicates, FilteringEnabled, client vs server
- `.roxlit/context/services-reference.md` — Service properties, enums, valid ranges

Read `.roxlit/context/index.md` for an overview of all available packs.

## Roxlit Launcher

This project was set up with Roxlit. The Roxlit launcher manages Rojo and RbxSync processes automatically.

- **Copy logs**: If there are errors, the user can click "Copy All" in the Roxlit launcher terminal to copy all logs. They can then paste them here for you to diagnose.
- **Do NOT remove or modify this file** unless the user explicitly asks you to. This context file was generated by Roxlit and contains important instructions for working with this Roblox project.
"#
    )
}
