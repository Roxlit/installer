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

/// Returns the Debug module for studio-only logging.
/// `Debug.print()` / `Debug.warn()` only output in Studio, silent in production.
pub fn debug_module() -> &'static str {
    r#"--!strict
-- Debug logging module. Use Debug.print() instead of print() so logs
-- are visible in Studio but stripped in production.

local RunService = game:GetService("RunService")

local IS_STUDIO = RunService:IsStudio()

local Debug = {}

function Debug.print(...: any)
	if IS_STUDIO then
		print(...)
	end
end

function Debug.warn(...: any)
	if IS_STUDIO then
		warn(...)
	end
end

return Debug
"#
}

/// Returns the rbxsync.json configuration.
/// Only excludes Roblox internals (CoreGui, CorePackages). RbxSync extracts instances
/// (.rbxjson) from Studio for backup/exploration. Scripts (.luau) are managed by Rojo.
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
  }}
}}
"#
    )
}

/// Context version — bump this whenever ai_context() content changes significantly.
/// ensure_ai_context() compares this against the marker in the existing file to decide
/// whether to regenerate. Format: same as Cargo.toml version.
pub const CONTEXT_VERSION: &str = "0.7.0";

/// Debug plugin version — bump to force re-installation when plugin code changes.
pub const DEBUG_PLUGIN_VERSION: &str = "1.0.1";

/// Returns the Luau source code for the RoxlitDebug Studio plugin.
/// Captures LogService.MessageOut and sends batches to the Roxlit log server via HTTP.
pub fn debug_plugin_luau() -> String {
    format!(
        r#"--!strict
-- RoxlitDebug v{version} — Studio Output capture for AI debugging
-- Sends LogService output to the Roxlit launcher log server.
-- If the launcher is not running, the plugin silently does nothing.

local HttpService = game:GetService("HttpService")
local LogService = game:GetService("LogService")

local SERVER_URL = "http://127.0.0.1:19556"
local BATCH_INTERVAL = 0.5
local BATCH_MAX = 50
local HEALTH_INTERVAL = 10

local buffer: {{[string]: any}} = {{}}
local serverAlive = false
local lastHealthCheck = 0

local function checkHealth()
	local ok, _ = pcall(function()
		HttpService:GetAsync(SERVER_URL .. "/health")
	end)
	serverAlive = ok
	lastHealthCheck = os.clock()
end

local function flushBuffer()
	if #buffer == 0 or not serverAlive then
		return
	end

	local batch = buffer
	buffer = {{}}

	pcall(function()
		local json = HttpService:JSONEncode(batch)
		HttpService:PostAsync(SERVER_URL .. "/log", json, Enum.HttpContentType.ApplicationJson)
	end)
end

local function levelFromType(messageType: Enum.MessageType): string
	if messageType == Enum.MessageType.MessageError then
		return "error"
	elseif messageType == Enum.MessageType.MessageWarning then
		return "warn"
	end
	return "info"
end

LogService.MessageOut:Connect(function(message: string, messageType: Enum.MessageType)
	if not serverAlive then
		return
	end

	table.insert(buffer, {{
		message = message,
		level = levelFromType(messageType),
		timestamp = os.clock(),
	}})

	if #buffer >= BATCH_MAX then
		flushBuffer()
	end
end)

-- Initial health check
checkHealth()

-- Periodic flush + health check
task.spawn(function()
	while true do
		task.wait(BATCH_INTERVAL)

		if os.clock() - lastHealthCheck >= HEALTH_INTERVAL then
			checkHealth()
		end

		flushBuffer()
	end
end)
"#,
        version = DEBUG_PLUGIN_VERSION
    )
}

/// Returns the debug plugin as a binary .rbxm file (Roblox Binary Model format).
/// Studio only loads .rbxm for local plugins — .rbxmx (XML) is rejected.
pub fn debug_plugin_rbxm() -> Vec<u8> {
    let source = debug_plugin_luau();
    let name = "RoxlitDebug";

    let mut buf = Vec::new();

    // ── File Header (32 bytes) ──
    buf.extend_from_slice(b"<roblox!");
    buf.extend_from_slice(&[0x89, 0xFF, 0x0D, 0x0A, 0x1A, 0x0A]); // signature
    buf.extend_from_slice(&0u16.to_le_bytes());   // version
    buf.extend_from_slice(&1i32.to_le_bytes());   // num classes
    buf.extend_from_slice(&1i32.to_le_bytes());   // num instances
    buf.extend_from_slice(&[0u8; 8]);             // reserved

    // ── INST chunk (defines the Script class) ──
    {
        let mut d = Vec::new();
        d.extend_from_slice(&0i32.to_le_bytes()); // classID
        rbxm_string(&mut d, "Script");
        d.push(0);                                 // objectFormat (regular)
        d.extend_from_slice(&1i32.to_le_bytes()); // instanceCount
        d.extend_from_slice(&0i32.to_be_bytes()); // referent 0 (transformed, interleaved BE)
        rbxm_chunk(&mut buf, b"INST", &d);
    }

    // ── PROP: Name (String = 0x01) ──
    {
        let mut d = Vec::new();
        d.extend_from_slice(&0i32.to_le_bytes());
        rbxm_string(&mut d, "Name");
        d.push(0x01);
        rbxm_string(&mut d, name);
        rbxm_chunk(&mut buf, b"PROP", &d);
    }

    // ── PROP: Source (String = 0x01) ──
    {
        let mut d = Vec::new();
        d.extend_from_slice(&0i32.to_le_bytes());
        rbxm_string(&mut d, "Source");
        d.push(0x01);
        rbxm_string(&mut d, &source);
        rbxm_chunk(&mut buf, b"PROP", &d);
    }

    // ── PRNT chunk (parent relationships) ──
    {
        let mut d = Vec::new();
        d.push(0);                                 // version
        d.extend_from_slice(&1i32.to_le_bytes()); // count
        d.extend_from_slice(&0i32.to_be_bytes()); // child ref 0 (transformed, interleaved BE)
        d.extend_from_slice(&1i32.to_be_bytes()); // parent ref -1 → transformed = 1 (no parent)
        rbxm_chunk(&mut buf, b"PRNT", &d);
    }

    // ── END chunk ──
    rbxm_chunk(&mut buf, b"END\0", b"</roblox>");

    buf
}

/// Write a length-prefixed UTF-8 string (u32 LE length + bytes).
fn rbxm_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

/// Write an uncompressed chunk: 4-byte name + header + raw data.
fn rbxm_chunk(buf: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    buf.extend_from_slice(name);
    buf.extend_from_slice(&0u32.to_le_bytes());                  // compressedLen (0 = raw)
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());  // uncompressedLen
    buf.extend_from_slice(&[0u8; 4]);                            // reserved
    buf.extend_from_slice(data);
}

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
## RbxSync (Instance Snapshots + MCP)

This project uses RbxSync alongside Rojo. While Rojo syncs Luau scripts, RbxSync provides instance snapshots and an **MCP server** for real-time interaction with Studio.

**IMPORTANT: You have DIRECT ACCESS to Roblox Studio via MCP tools.** You can read instances, create objects, set properties, execute Luau code, and run tests — all in real-time. Do NOT tell the user "I can't see your screen" or ask them to check things manually when you can use MCP tools to check yourself.

### How it works

- **Rojo**: Syncs `.luau` scripts in ALL services (real-time, filesystem → Studio). One-directional.
- **RbxSync**: Periodically extracts instances from Studio to local `.rbxjson` files (~30s). These are read-only snapshots for browsing and backup.
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

**Debugging & testing — MANDATORY WORKFLOW:**

Every time you create or modify something, you MUST verify it works. Never say "done" or "it should work" without verifying.

**The debugging loop:** create/edit → verify with `get_instance` → playtest with `run_test` → read errors → fix → repeat

Tools:
- `get_instance` — verify an instance or script exists in Studio's DataModel. Always use this after creating anything.
- `run_test` — starts a playtest session in Studio, captures ALL console output (prints, warnings, errors), stops the session, and returns the full output. This is your #1 debugging tool.
- `run_code` — execute arbitrary Luau in Studio to inspect runtime state (check property values, verify SoundIds, test expressions). Use this for quick checks without a full playtest.

**CRITICAL — `run_code` rules:**
- Each `run_code` call is a **separate execution context**. Local variables do NOT persist between calls.
- To reference instances created in a previous call, use full paths: `workspace:FindFirstChild("Car")`, NOT a local variable from before.
- For complex multi-instance creation (models, vehicles, GUIs), do it in a **single `run_code` call** with all the code. Do NOT split into multiple calls expecting variables to carry over.
- If the code is too long for one call, have each call save its work to the DataModel (parent instances to workspace) and the next call finds them by path.
- **NEVER call `:Destroy()` on existing instances to "rebuild from scratch"** unless the user explicitly asks. If something looks wrong, inspect it first — don't delete and redo. Destroying work wastes time and tokens.

**Testing tools — what works and what doesn't:**
- `run_test` with `duration` → **RELIABLE**. Starts a playtest, waits the specified seconds, captures all output, stops. Use this for automated checks (script errors, print output, initialization).
- `run_code` → **RELIABLE**. Executes Luau in edit mode. Good for creating instances, checking properties, quick validations.
- `run_test` with `background: true` → **UNRELIABLE**. Background playtests often drop after 1-2 seconds. Avoid using this.
- `bot_observe`, `bot_move`, `bot_action`, `bot_wait_for` → **DO NOT USE**. Bot tools are unstable and frequently fail with timeouts or connection errors. Do not waste tokens retrying them.

**For interactive testing** (player sitting in a vehicle, pressing keys, testing GUI interactions): ask the user to playtest manually (F5) and then read `.roxlit/logs/latest.log` for the Debug.print output. You cannot simulate player input via MCP.

**Common debugging patterns:**
- "car doesn't move" → `run_test`, read the output for errors (missing VehicleSeat? wrong property name?)
- "sound doesn't play" → `run_code` to check `game.Workspace.Model.Sound.SoundId` and verify it's not empty
- "script doesn't run" → `get_instance` to verify the script exists in the right location, then `run_test` to see if there are errors
- "GUI doesn't show" → `get_instance` to check ScreenGui.Enabled, then `run_test` for errors

**MANDATORY: Never tell the user "I can't see your screen" when you have MCP tools. Check yourself.**

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
## RbxSync (Instance Snapshots)

This project uses RbxSync alongside Rojo. While Rojo syncs Luau scripts, RbxSync periodically extracts instance snapshots from Studio to local `.rbxjson` files.

### How it works

- **Rojo**: Syncs `.luau` scripts (real-time, filesystem → Studio). One-directional.
- **RbxSync**: Periodically extracts instances from Studio to local `.rbxjson` files (~30s). These are **read-only snapshots** for browsing and backup — NOT for editing.

### IMPORTANT: Activating the RbxSync Plugin

The RbxSync plugin must be activated **once per Studio session**:
1. If this is the first time after installation, **restart Roblox Studio** so it loads the new plugin
2. Go to the Plugins tab
3. Find the RbxSync plugin and click "Sync" or "Connect"
4. The plugin connects to `rbxsync serve` (which Roxlit starts automatically in the project directory)

If the user reports that the RbxSync plugin doesn't appear, they need to restart Studio. Plugins are only loaded when Studio starts.

### Working with Instances

**Scripts** → always edit local `.luau` files in `scripts/` (Rojo syncs to Studio in real-time)

**Instances** (Parts, GUIs, Models, etc.) → edit directly in Roblox Studio. You CANNOT create or modify instances by editing local files — local `.rbxjson` files are auto-generated snapshots that get overwritten every ~30s.

**NEVER edit `.rbxjson` files directly** — they will be overwritten by the next Studio extract.
**NEVER create files in `src/`** — rbxsync manages this directory and overwrites it periodically.

### Reading Instance Data

Local `.rbxjson` files are useful for **reading** the project structure:
- Use Glob + Read to explore what instances exist, their properties, and hierarchy
- Files may be up to 30s stale — if freshness matters, ask the user to check in Studio

### RbxSync File Structure

Instance snapshots are stored under `src/`, mirroring the Roblox DataModel hierarchy:

```
src/
  Workspace/
    SpawnLocation/          ← Folder name = instance name
      _meta.rbxjson         ← Class + properties of SpawnLocation
      Decal.rbxjson         ← Child instance (simple, no children of its own)
    MyModel/
      _meta.rbxjson         ← Class + properties of MyModel
      Part1.rbxjson         ← Child Part
  Lighting/
    _meta.rbxjson           ← Lighting service properties
    Atmosphere.rbxjson      ← Post-processing effect
```

Rules:
- **Instances with children** → folder with `_meta.rbxjson` inside
- **Leaf instances** (no children) → single `.rbxjson` file
- **`_meta.rbxjson`** contains `""ClassName""` and `""Properties""`
- To find an instance, **search by folder/file name** in `src/`
- Rojo ignores `.rbxjson` files (`globIgnorePaths`), so both file types coexist without conflicts

### Backups

Roxlit automatically backs up `.rbxjson` files before each Studio extract:
- Location: `.roxlit/backups/<timestamp>/`
- Each backup preserves the full `src/` directory structure
- Maximum 20 backups retained (oldest are deleted)
- Each backup has a `manifest.json` listing the files

If the user reports lost changes to instances, check `.roxlit/backups/` for recent backups. You can diff the backup files against current files to find what changed.

### Debugging Without MCP

Without MCP tools, you cannot directly run code or tests in Studio. But Roxlit captures Studio output automatically:

1. **Ask them to playtest**: Press F5 (Play) in Studio to run the game
2. **Read the logs**: `.roxlit/logs/latest.log` captures Studio console output (prints, warnings, errors) in real-time — check `[studio-err]` for errors first
3. **If logs are empty**: The RoxlitDebug plugin may not be loaded yet — ask the user to restart Studio once so it picks up the plugin
4. **Fallback**: Ask the user to check the Output panel in Studio, or click "Copy All" in the Roxlit launcher terminal
5. **Analyze the error**: Once you have the error text, diagnose and fix it

Common patterns:
- `"X is not a valid member of Y"` → wrong property name or instance path
- `"attempt to index nil"` → a `FindFirstChild` or variable is nil, check the instance exists
- Script not running at all → verify the file is in the right `scripts/` subfolder and has the correct `.server.luau` or `.client.luau` extension

"#
    };

    format!(
        r#"{VERSION_MARKER} {CONTEXT_VERSION} -->
# {project_name}

Roblox game project using Rojo for file syncing. Write Luau code in `scripts/` and Rojo syncs it to Roblox Studio in real time.

## Tech Stack

- **Language**: Luau (Roblox's typed Lua dialect)
- **Sync tool**: Rojo (filesystem <-> Roblox DataModel)
- **Instance snapshots**: RbxSync (periodic Studio → local snapshots for backup/exploration)
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

**Instances** (Parts, GUIs, Models, etc.): Edit in Roblox Studio directly. Local `.rbxjson` files in `src/` are **read-only snapshots** from periodic Studio extracts (~30s) — useful for browsing and backup but NOT for editing. See the RbxSync section below.

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

## Instance Organization

When creating instances (Parts, Models, GUIs), follow these rules to keep the project clean from the start:

- **Group related parts in a Model**: A door with its frame and wall = one Model, not 3 loose Parts in Workspace
- **Set PrimaryPart on every Model**: Required for `Model:PivotTo()` to work
- **Scripts that control a specific object go INSIDE that object's Model**: A DoorController script belongs inside the Door Model, not loose in Workspace
- **Game-wide systems go in `scripts/ServerScriptService/`**: Things like a round manager, data save system, or admin commands
- **Name everything descriptively**: `Door`, `DoorPart`, `Frame`, `Wall` — not `Part`, `Part2`, `Model`
- **Never leave Parts or Scripts loose in Workspace root**: Always organize under a Model or Folder

Example — a door with proximity interaction:
```
Workspace/
  Door (Model, PrimaryPart = DoorPart)
    ├── DoorPart (Part) ← the part that rotates
    │   └── ProximityPrompt ← built-in Roblox class for "Press E" interactions
    ├── Frame (Part)
    ├── Wall (Part)
    └── DoorController (Script) ← controls this specific door
```

## Key Rules

- **Language**: ALWAYS respond in the user's language. If they write in Spanish, respond in Spanish. If English, respond in English. Your explanations, instructions, and conversation must match the user's language. For code comments, default to English unless the user asks otherwise.
- **Never use `wait()`** → use `task.wait()` instead
- **Never trust the client** → validate everything on the server
- **Never store secrets in ReplicatedStorage** → clients can read it
- **Never call DataStore without `pcall()`** → DataStore calls can fail
- **Testing**: Scripts don't run in edit mode. Press Play (F5) for server + client, Run (F8) for server-only.
- **MANDATORY — Studio UI**: Before telling the user where to find ANYTHING in Studio (Output, panels, menus, buttons), you MUST first read `.roxlit/context/studio-ui.md`. Do NOT rely on your own knowledge of Studio — it is outdated and wrong (e.g., there is NO "View" tab in the new Studio UI). Read the file first, then answer.
{rbxsync_section}## Studio Output Logs

Roxlit captures **all Roblox Studio output** (prints, warnings, errors) in real-time via a local plugin. When the user presses Play (F5), every `print()`, `warn()`, and `error()` from their scripts appears in `.roxlit/logs/latest.log` alongside Rojo and RbxSync logs.

### Log Prefixes

- `[studio]` — print() output (info)
- `[studio-warn]` — warn() output
- `[studio-err]` — runtime errors, script errors
- `[rojo]` / `[rojo-err]` — Rojo sync output
- `[rbxsync]` / `[rbxsync-err]` — RbxSync output

### MANDATORY: Code Instrumentation with Debug Module

Every Luau script you create or modify MUST include strategic debug prints for debugging visibility. This is NOT optional — it is how you (the AI) get runtime feedback.

**IMPORTANT:** Use `Debug.print()` / `Debug.warn()` instead of raw `print()` / `warn()`. The Debug module (in `ReplicatedStorage/Debug.luau`) only outputs in Studio — silent in production. This prevents leaking internal state to players via the client console (F9).

```lua
local Debug = require(game.ReplicatedStorage.Debug)
```

**Format**: `Debug.print("[ScriptName] Description:", value)`

**Where to add debug prints:**
- At script/function start: `Debug.print("[DoorController] Initialized")`
- Before important operations: `Debug.print("[DataManager] Saving data for:", player.Name)`
- After important operations: `Debug.print("[DataManager] Save complete, entries:", #data)`
- On errors/edge cases: `Debug.warn("[VehicleSeat] No wheels found in model:", model.Name)`
- With relevant values: `Debug.print("[RoundManager] Round started, players:", #players, "map:", mapName)`

**Example:**
```lua
--!strict
local Players = game:GetService("Players")
local Debug = require(game.ReplicatedStorage.Debug)

Debug.print("[GameManager] Script initialized")

Players.PlayerAdded:Connect(function(player: Player)
    Debug.print("[GameManager] Player joined:", player.Name, "total:", #Players:GetPlayers())
    -- game logic here
end)
```

**When to use raw `print()` instead:** Only for output that you intentionally want players to see in production (e.g., admin commands feedback). For all debugging and development logging, always use `Debug.print()`.

Without debug prints, debugging is guesswork. With them, you can read `.roxlit/logs/latest.log` and see exactly what happened.

### Debugging Workflow

1. **Read `.roxlit/logs/latest.log` FIRST** — the answer is almost always there
2. Search for `[studio-err]` to find runtime errors
3. Follow `[ScriptName]` prints to trace execution flow
4. If prints are missing, add more and ask the user to playtest again
5. **Never guess** — always read the actual error before attempting a fix

## Roblox Context Packs

This project includes curated Roblox documentation in `.roxlit/context/`. Before writing code that involves a specific system, **read the relevant file**:

- `.roxlit/context/datastore.md` — DataStoreService: throttling limits, session locking, retry patterns
- `.roxlit/context/remote-events.md` — RemoteEvent/Function: server validation, rate limiting, type checking
- `.roxlit/context/player-lifecycle.md` — PlayerAdded, CharacterAdded, respawn, death handling
- `.roxlit/context/workspace-physics.md` — Parts, CFrame, raycasting, collision groups, **cylinder orientations, vehicles (VehicleSeat + wheels), Z-fighting**
- `.roxlit/context/replication.md` — What replicates, FilteringEnabled, client vs server
- `.roxlit/context/services-reference.md` — Service properties, enums, valid ranges
- `.roxlit/context/studio-ui.md` — **READ THIS before giving ANY Studio UI directions**: where panels are (Output, Explorer, etc.), mezzanine/toolbar layout, testing modes (F5/F8), troubleshooting ("my script isn't running")

Read `.roxlit/context/index.md` for an overview of all available packs.

## Roxlit Launcher

This project was set up with Roxlit. The Roxlit launcher manages Rojo and RbxSync processes automatically.

- **Session logs on disk**: Roxlit captures ALL output to `.roxlit/logs/latest.log` — Rojo, RbxSync, AND Roblox Studio console (prints, warnings, errors). You can read this file to diagnose issues without asking the user to copy-paste. Previous sessions are saved as `.roxlit/logs/session-<timestamp>.log` (up to 10 retained).
- **Copy logs from UI**: The user can also click "Copy All" in the Roxlit launcher terminal to copy all logs and paste them here.
- **Do NOT remove or modify the Roxlit-generated sections above.** They are auto-updated by Roxlit when new versions are available.

{USER_NOTES_MARKER}

Add your own project-specific notes, rules, or instructions below this line. Roxlit will preserve this section when updating the context above.

Settings the AI will look for here:
- `Studio language: <language>` — so the AI uses correct localized names for Studio UI elements (e.g., `Studio language: Spanish`)

"#
    )
}
