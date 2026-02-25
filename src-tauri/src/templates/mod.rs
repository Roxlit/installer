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
      "$path": "src/ServerScriptService"
    }},
    "StarterPlayer": {{
      "$className": "StarterPlayer",
      "StarterPlayerScripts": {{
        "$className": "StarterPlayerScripts",
        "$path": "src/StarterPlayer/StarterPlayerScripts"
      }}
    }},
    "ReplicatedStorage": {{
      "$className": "ReplicatedStorage",
      "$path": "src/ReplicatedStorage"
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

/// Returns the AI context file content with Roblox/Luau development instructions.
/// This is the same content regardless of AI tool — only the filename changes.
pub fn ai_context(project_name: &str, mcp_available: bool) -> String {
    let rbxsync_section = if mcp_available {
        r#"
## RbxSync (Instance Sync + MCP)

This project uses RbxSync alongside Rojo. While Rojo syncs Luau scripts, RbxSync provides **bidirectional sync for all instances** (Parts, GUIs, Models, etc.) and an **MCP server** for debugging.

### How it works

- **Rojo**: Syncs `.luau` scripts to the Roblox DataModel (filesystem → Studio). Configured with `globIgnorePaths: ["**/*.rbxjson"]` so it ignores instance files
- **RbxSync**: Syncs instances bidirectionally (Studio ↔ filesystem as `.rbxjson` files). Handles ALL services (Workspace, Lighting, ReplicatedStorage, ServerScriptService, and any other). Only excludes CoreGui and CorePackages (Roblox internals)
- Both coexist in the same directories without conflict — Rojo manages `.luau` files, RbxSync manages `.rbxjson` files

### IMPORTANT: Activating the RbxSync Plugin

The RbxSync plugin must be activated **once per Studio session**:
1. If this is the first time after installation, **restart Roblox Studio** so it loads the new plugin
2. Go to the Plugins tab
3. Find the RbxSync plugin and click "Sync" or "Connect"
4. The plugin connects to `rbxsync serve` (which Roxlit starts automatically in the project directory)

If the user reports that the RbxSync plugin doesn't appear, they need to restart Studio. Plugins are only loaded when Studio starts.
If the user reports that instance sync isn't working, remind them to activate the plugin in Studio. This is required every time Studio is opened.

### MCP Tools

The RbxSync MCP server provides tools that complement local file editing:

**Writing (always use local files):**
- To modify scripts → edit `.luau` files (Rojo syncs to Studio)
- To modify instances → edit `.rbxjson` files (RbxSync syncs to Studio)
- **Never use MCP to create, modify, or delete instances/scripts** — it bypasses sync and causes conflicts

**IMPORTANT — Reading before writing `.rbxjson` files:**
- Local `.rbxjson` files may be up to 30 seconds stale (they are refreshed by periodic Studio extracts)
- **Before modifying any `.rbxjson` file, ALWAYS read the current state from Studio via `get_instance` first.** Do NOT trust the local file as the source of truth — the user may have moved, renamed, or changed the instance in Studio since the last extract.
- Only after confirming the current state via MCP should you edit the local `.rbxjson` file.

**Reading (choose the most efficient method):**
- To read a **specific instance** → use `get_instance` (always fresh from Studio, saves tokens vs reading `.rbxjson`)
- To **explore a large hierarchy** or find instances → read local `.rbxjson` files with Glob + Read (fewer round-trips than chaining `get_children`), but verify critical properties via `get_instance` before editing
- For **read-only exploration** where precision doesn't matter → local files are fine

**Debugging & testing:**
- `run_code` — execute Luau in Studio for debugging (print values, inspect runtime state)
- `run_test` — run tests inside Studio

### Working with Instances

- Instance properties are stored as `.rbxjson` files in the project directory
- You can read and modify these files to change Part positions, GUI layouts, etc.
- Changes sync automatically to Studio when RbxSync is running
- **This project is designed for AI-first development** — instance editing should be done through AI tools, not by manually editing `.rbxjson` files

### RbxSync File Structure

Instances are stored under `src/` mirroring the Roblox DataModel hierarchy:

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
- **`_meta.rbxjson`** always contains `""ClassName""` and `""Properties""` — edit properties here to change the instance
- To find an instance, **search by folder/file name** in `src/`, not just in script directories
- Changes to `.rbxjson` files sync to Studio automatically when RbxSync is running

### Sync Workflow

Roxlit manages sync automatically. Here's what you need to know:

**Editing scripts (.luau)**: Just edit the files. Rojo syncs them to Studio in real-time.

**Editing instances (.rbxjson)**: **Always read the current state via MCP `get_instance` BEFORE editing.** Local files may be up to 30s stale. After confirming the live state, edit the local `.rbxjson` file — Roxlit auto-syncs it to Studio (~2s).

**Reading current Studio state**: Use MCP `get_instance` for fresh data. Local `.rbxjson` files are updated every ~30s by auto-extract — only use them for exploration, not as source of truth before edits.

**File ownership**:
- Rojo owns `.luau` files — always edit locally, never via MCP
- RbxSync owns `.rbxjson` files — always edit locally, never via MCP

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

This project uses RbxSync alongside Rojo. While Rojo syncs Luau scripts, RbxSync provides **bidirectional sync for all instances** (Parts, GUIs, Models, etc.).

### How it works

- **Rojo**: Syncs `.luau` scripts to the Roblox DataModel (filesystem → Studio). Configured with `globIgnorePaths: ["**/*.rbxjson"]` so it ignores instance files
- **RbxSync**: Syncs instances bidirectionally (Studio ↔ filesystem as `.rbxjson` files). Handles ALL services (Workspace, Lighting, ReplicatedStorage, ServerScriptService, and any other). Only excludes CoreGui and CorePackages (Roblox internals)
- Both coexist in the same directories without conflict — Rojo manages `.luau` files, RbxSync manages `.rbxjson` files

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
- You can read and modify these files to change Part positions, GUI layouts, etc.
- Changes sync automatically to Studio when RbxSync is running
- **Always edit local files** (`.luau` for scripts, `.rbxjson` for instances) — never modify them through other means
- **This project is designed for AI-first development** — instance editing should be done through AI tools, not by manually editing `.rbxjson` files

### RbxSync File Structure

Instances are stored under `src/` mirroring the Roblox DataModel hierarchy:

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
- **`_meta.rbxjson`** always contains `""ClassName""` and `""Properties""` — edit properties here to change the instance
- To find an instance, **search by folder/file name** in `src/`, not just in script directories
- Changes to `.rbxjson` files sync to Studio automatically when RbxSync is running

### Sync Workflow

Roxlit manages sync automatically. Here's what you need to know:

**Editing scripts (.luau)**: Just edit the files. Rojo syncs them to Studio in real-time.

**Editing instances (.rbxjson)**: Local `.rbxjson` files may be up to 30s stale. Before editing, ask the user to confirm the current state in Studio or trigger a manual extract from the Roxlit launcher. After editing, Roxlit auto-syncs to Studio (~2s).

**Reading current Studio state**: Local `.rbxjson` files are updated every ~30s by auto-extract. They are suitable for exploration but should not be treated as the source of truth before making edits. If the user just made changes in Studio, ask them to trigger a manual extract from the Roxlit launcher.

**File ownership**:
- Rojo owns `.luau` files — always edit locally
- RbxSync owns `.rbxjson` files — always edit locally

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
  ServerScriptService/          → Server scripts (.luau by Rojo, .rbxjson by RbxSync)
  StarterPlayer/
    StarterPlayerScripts/       → Client scripts (.luau by Rojo, .rbxjson by RbxSync)
  ReplicatedStorage/            → Shared modules + RemoteEvents, etc.
  Workspace/                    → 3D world (Parts, Models, Terrain)
  Lighting/                     → Lighting & post-processing effects
  ServerStorage/                → Server-only assets
  StarterGui/                   → GUI templates
  StarterPack/                  → Starter tools
  SoundService/                 → Audio
  <AnyOtherService>/            → Any Roblox service appears here automatically
```

Rojo and RbxSync coexist in the same directories, separated by **file type**:
- `.luau` files → managed by Rojo (scripts)
- `.rbxjson` files → managed by RbxSync (instances)

RbxSync syncs ALL services automatically. Only CoreGui and CorePackages (Roblox internals) are excluded.

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
