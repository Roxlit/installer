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
/// RbxSync is used ONLY for MCP tools (run_code, run_test, insert_model).
/// Instance sync is handled entirely by Rojo via .model.json files.
pub fn rbxsync_json(project_name: &str) -> String {
    format!(
        r#"{{
  "name": "{project_name}",
  "tree": "./.rbxsync",
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
pub const CONTEXT_VERSION: &str = "0.8.0";

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

if serverAlive then
	print("[RoxlitDebug] v{version} loaded — connected to Roxlit launcher")
else
	print("[RoxlitDebug] v{version} loaded — launcher not detected (logs won't be captured)")
end

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
    let mcp_section = if mcp_available {
        r#"
## MCP Tools (Testing & Marketplace Only)

MCP tools connect to Roblox Studio via the RbxSync plugin. Use them ONLY for:

- `run_code` — Execute Luau in Studio. For quick checks, verifying state, debugging. NOT for building instances (use .model.json instead).
- `run_test` — Start a playtest, capture all console output, stop. Your #1 debugging tool.
- `insert_model` — Insert a marketplace asset by ID into Studio.

**Do NOT use MCP to create instances.** Write .model.json files instead — Rojo syncs them automatically.

### Debugging with MCP

**The loop:** edit files → `run_test` → read output → fix → repeat

- `run_code` to verify an instance exists: `print(workspace:FindFirstChild("Door"))`
- `run_test` with duration to capture script errors and print output
- Each `run_code` call is a separate context — local variables don't persist between calls

"#
    } else {
        r#"
## Debugging Without MCP

Roxlit captures Studio output automatically:

1. **Make sure your scripts have Debug.print() calls** — without them, logs are empty
2. Ask the user to playtest (F5 in Studio)
3. Read `.roxlit/logs/latest.log` — search `[studio-err]` for errors first
4. If logs are empty: RoxlitDebug plugin not loaded (restart Studio) or no Debug.print() calls
5. Fallback: ask the user to check Output panel in Studio

"#
    };

    let instance_section = r#"
## Creating Instances with .model.json (Rojo)

Rojo syncs **both scripts AND instances**. For instances (Parts, Models, GUIs, Folders), create `.model.json` files in `src/`. Rojo syncs them to Studio automatically, just like .luau files.

### Basic Format

`src/Workspace/SpawnPlatform.model.json`:
```json
{
  "ClassName": "Part",
  "Properties": {
    "Size": [20, 1, 20],
    "Position": [0, 0.5, 0],
    "Anchored": true,
    "Material": "SmoothPlastic",
    "Color3": [0.2, 0.8, 0.4]
  }
}
```

The filename (minus `.model.json`) becomes the instance Name. Rojo places it under the parent service mapped in the project file.

### Models with Children

`src/Workspace/Door.model.json`:
```json
{
  "ClassName": "Model",
  "Children": [
    {
      "Name": "DoorPart",
      "ClassName": "Part",
      "Properties": {
        "Size": [4, 6, 0.5],
        "Position": [0, 3, 0],
        "Anchored": true,
        "BrickColor": {"BrickColor": 194}
      }
    },
    {
      "Name": "Frame",
      "ClassName": "Part",
      "Properties": {
        "Size": [1, 7, 1],
        "Position": [-2.5, 3.5, 0],
        "Anchored": true,
        "Color3": [0.3, 0.3, 0.3]
      }
    },
    {
      "Name": "OpenPrompt",
      "ClassName": "ProximityPrompt",
      "Properties": {
        "ActionText": "Open",
        "HoldDuration": 0,
        "MaxActivationDistance": 10
      }
    }
  ]
}
```

### GUI Example

`src/StarterGui/MainMenu.model.json`:
```json
{
  "ClassName": "ScreenGui",
  "Properties": {
    "ResetOnSpawn": false,
    "ZIndexBehavior": "Sibling"
  },
  "Children": [
    {
      "Name": "TitleLabel",
      "ClassName": "TextLabel",
      "Properties": {
        "Size": {"UDim2": [[0.5, 0], [0.1, 0]]},
        "Position": {"UDim2": [[0.25, 0], [0.1, 0]]},
        "Text": "My Game",
        "TextScaled": true,
        "BackgroundTransparency": 1,
        "TextColor3": [1, 1, 1],
        "FontFace": {"Font": {"family": "rbxasset://fonts/families/GothamSSm.json", "weight": "Bold", "style": "Normal"}}
      }
    },
    {
      "Name": "PlayButton",
      "ClassName": "TextButton",
      "Properties": {
        "Size": {"UDim2": [[0.2, 0], [0.06, 0]]},
        "Position": {"UDim2": [[0.4, 0], [0.5, 0]]},
        "Text": "Play",
        "TextScaled": true,
        "BackgroundColor3": [0.2, 0.8, 0.4]
      }
    }
  ]
}
```

### Property Type Reference

**Implicit (Rojo infers the type):**
- Bool: `true` / `false`
- String: `"Hello"`
- Number: `15.0`
- Vector3: `[1.0, 2.0, 3.0]`
- Vector2: `[-50.0, 50.0]`
- Color3: `[0.5, 0.5, 0.5]` (floats 0-1, NOT 0-255)
- Content (asset IDs): `"rbxassetid://12345"`
- Enum (by name): `"SmoothPlastic"` for Material, `"Sibling"` for ZIndexBehavior
- Tags: `["tag1", "tag2"]`

**Explicit (you specify the type):**
- BrickColor: `{"BrickColor": 194}`
- Color3uint8: `{"Color3uint8": [163, 162, 165]}` (integers 0-255)
- Enum (by number): `{"Enum": 512}`
- CFrame: `{"CFrame": {"position": [0, 10, 0], "orientation": [[1,0,0],[0,1,0],[0,0,1]]}}`
- UDim: `{"UDim": [1.0, 32]}` (scale, offset)
- UDim2: `{"UDim2": [[-1.0, 100], [1.0, -100]]}` (X and Y UDim pairs)
- NumberRange: `{"NumberRange": [-36.0, 94.0]}`
- NumberSequence: `{"NumberSequence": {"keypoints": [{"time": 0.0, "value": 5.0, "envelope": 0.0}]}}`
- ColorSequence: `{"ColorSequence": {"keypoints": [{"time": 0.0, "color": [1, 1, 0.5]}]}}`
- Rect: `{"Rect": [[0.0, 5.0], [10.0, 15.0]]}`
- PhysicalProperties: `{"PhysicalProperties": "Default"}` or `{"PhysicalProperties": {"density": 0.5, "friction": 1.0, "elasticity": 0.0, "frictionWeight": 50.0, "elasticityWeight": 25.0}}`
- Font: `{"Font": {"family": "rbxasset://fonts/families/GothamSSm.json", "weight": "Bold", "style": "Normal"}}`

**Not supported by Rojo:** Terrain data, CSG unions, MeshPart geometry. For these, use Studio directly or `insert_model` via MCP.

### Rules

- **Scripts are ALWAYS .luau files**, never inside .model.json
- A script that controls a Model goes next to the .model.json as a .luau file, both inside the same `src/` subfolder
- Rojo syncs .model.json files in real-time just like .luau files — save the file and it appears in Studio
- For complex models with many parts, use one .model.json with nested Children — not separate files per part
- To delete an instance from Studio, delete the .model.json file
- .model.json files are versionable in git and diffable — this is a huge advantage over MCP-based creation

"#;

    format!(
        r#"{VERSION_MARKER} {CONTEXT_VERSION} -->
# {project_name}

Roblox game project using Rojo for file syncing. Write Luau code in `src/` and Rojo syncs it to Roblox Studio in real time.

## Tech Stack

- **Language**: Luau (Roblox's typed Lua dialect)
- **Sync tool**: Rojo (filesystem <-> Roblox DataModel, scripts AND instances)
- **Type checking**: Strict mode (`--!strict`)

## Project Structure

```
src/                                ← All game code and instances (synced by Rojo to Studio)
  ServerScriptService/                  → Server scripts
  StarterPlayer/
    StarterPlayerScripts/               → Client scripts (player join)
    StarterCharacterScripts/            → Client scripts (character spawn)
  ReplicatedStorage/                    → Shared modules (server + client)
  ReplicatedFirst/                      → Early client loading scripts
  ServerStorage/                        → Server-only modules and assets
  Workspace/                            → 3D world: Parts (.model.json), scripts (.luau)
  StarterGui/                           → GUI (.model.json) + LocalScripts (.luau)
  StarterPack/                          → Starter tools + scripts
```

**Scripts**: Create `.luau` files in `src/` — Rojo syncs them to Studio in real-time.

**Instances** (Parts, Models, GUIs, Folders): Create `.model.json` files in `src/` — Rojo syncs them too. See the "Creating Instances with .model.json" section below.

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

- **Group related parts in a Model**: A door with its frame and wall = one .model.json, not 3 loose Parts
- **Set PrimaryPart on every Model**: Required for `Model:PivotTo()` to work
- **Scripts that control a specific object go next to that object's .model.json**: DoorController.server.luau sits next to Door.model.json in the same folder
- **Game-wide systems go in `src/ServerScriptService/`**: Things like a round manager, data save system, or admin commands
- **Name everything descriptively**: `Door`, `DoorPart`, `Frame`, `Wall` — not `Part`, `Part2`, `Model`
- **Never leave Parts or Scripts loose in Workspace root**: Always organize under a Model or Folder

Example — a door with proximity interaction:
```
src/Workspace/
  Door.model.json              ← Model with DoorPart, Frame, Wall, ProximityPrompt
  Door/
    DoorController.server.luau ← Script that controls this specific door
```

## Key Rules

- **Language**: ALWAYS respond in the user's language. If they write in Spanish, respond in Spanish. If English, respond in English. Your explanations, instructions, and conversation must match the user's language. For code comments, default to English unless the user asks otherwise.
- **Never use `wait()`** → use `task.wait()` instead
- **Never trust the client** → validate everything on the server
- **Never store secrets in ReplicatedStorage** → clients can read it
- **Never call DataStore without `pcall()`** → DataStore calls can fail
- **Testing**: Scripts don't run in edit mode. Press Play (F5) for server + client, Run (F8) for server-only.
- **MANDATORY — Studio UI**: Before telling the user where to find ANYTHING in Studio (Output, panels, menus, buttons), you MUST first read `.roxlit/context/studio-ui.md`. Do NOT rely on your own knowledge of Studio — it is outdated and wrong (e.g., there is NO "View" tab in the new Studio UI). Read the file first, then answer.

## IMPORTANT: Use Existing Community Systems for Complex Features

**Before building any complex system from scratch, SEARCH THE WEB for existing open-source Roblox community solutions.** The Roblox community has spent years building and refining systems for common game features. Using a battle-tested community system and customizing it is ALWAYS better than reinventing the wheel.

**Systems you should NEVER build from scratch — search for existing ones first:**
- **Vehicle physics** (chassis, suspension, steering) → search `roblox open source vehicle chassis A-Chassis site:devforum.roblox.com`
- **Combat/weapon systems** → search `roblox open source combat framework site:devforum.roblox.com`
- **Inventory/backpack systems** → search `roblox inventory system open source`
- **Dialog/quest systems** → search `roblox quest system open source`
- **Round-based game framework** → search `roblox round system framework`

**What YOU should build:** everything on top of the community base — appearance, game-specific features, custom UI, sounds, effects, unique mechanics. The community system is the foundation; the user's creative vision is what you add.

**Always mention licensing**: tell the user to check the original source for license terms before using in a published game. Most community systems are free to use with credit, but the user should verify.
{mcp_section}{instance_section}## Studio Output Logs

Roxlit captures **all Roblox Studio output** (prints, warnings, errors) in real-time via a local plugin. When the user presses Play (F5), every `print()`, `warn()`, and `error()` from their scripts appears in `.roxlit/logs/latest.log` alongside Rojo logs.

### Log Prefixes

- `[studio]` — print() output (info)
- `[studio-warn]` — warn() output
- `[studio-err]` — runtime errors, script errors
- `[rojo]` / `[rojo-err]` — Rojo sync output

### *** MANDATORY: Debug.print() in EVERY Script — NO EXCEPTIONS ***

**If you write a script without Debug.print() calls, you are working blind.** You will not see errors, you will not know what executed, you will not be able to debug. This is the #1 cause of wasted time and failed implementations.

**RULE: Every script you create or modify MUST have Debug.print() calls.** No exceptions. No "I'll add them later." Add them NOW, on every script, every time.

```lua
local Debug = require(game.ReplicatedStorage.Debug)
```

The Debug module (`ReplicatedStorage/Debug.luau`) only outputs in Studio — silent in production. This prevents leaking internal state to players via the client console (F9). Use `Debug.print()` / `Debug.warn()` instead of raw `print()` / `warn()`.

**Format**: `Debug.print("[ScriptName] Description:", value)`

**MINIMUM required debug prints per script:**
1. **Script start**: `Debug.print("[ScriptName] Initialized")` — confirms the script loaded
2. **Every event/callback entry**: `Debug.print("[ScriptName] EventName fired:", relevantValue)` — confirms it triggered
3. **Before critical operations**: `Debug.print("[ScriptName] About to do X:", params)` — shows what's about to happen
4. **After critical operations**: `Debug.print("[ScriptName] X complete, result:", result)` — confirms success
5. **On error/edge cases**: `Debug.warn("[ScriptName] Unexpected:", details)` — catches problems early

**Example — a vehicle controller without logging vs with logging:**
```lua
-- BAD: No Debug.print(). If the car doesn't move, you have ZERO information about why.
seat.Changed:Connect(function()
    for _, hinge in hinges do
        hinge.AngularVelocity = seat.ThrottleFloat * maxSpeed
    end
end)

-- GOOD: Full visibility. If the car doesn't move, logs tell you exactly where it failed.
Debug.print("[VehicleCtrl] Initialized, seat:", seat.Name, "hinges:", #hinges)
seat.Changed:Connect(function(prop)
    if prop == "ThrottleFloat" or prop == "SteerFloat" then
        local throttle = seat.ThrottleFloat
        local steer = seat.SteerFloat
        Debug.print("[VehicleCtrl] Input — throttle:", throttle, "steer:", steer)
        for i, hinge in hinges do
            hinge.AngularVelocity = -throttle * maxSpeed
            hinge.MotorMaxTorque = math.abs(throttle) > 0.01 and driveTorque or brakeTorque
        end
        Debug.print("[VehicleCtrl] Applied — angVel:", -throttle * maxSpeed, "torque:", driveTorque)
    end
end)
```

**When to use raw `print()` instead:** Only for output that you intentionally want players to see in production (e.g., admin commands feedback). For all debugging and development logging, always use `Debug.print()`.

**Without debug prints, you are debugging blind. With them, you read `.roxlit/logs/latest.log` and see exactly what happened, what values were used, and where it failed.**

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
- `.roxlit/context/workspace-physics.md` — Parts, CFrame, raycasting, collision groups, **cylinder orientations, vehicles (USE community chassis!), Z-fighting**
- `.roxlit/context/replication.md` — What replicates, FilteringEnabled, client vs server
- `.roxlit/context/services-reference.md` — Service properties, enums, valid ranges
- `.roxlit/context/studio-ui.md` — **READ THIS before giving ANY Studio UI directions**: where panels are (Output, Explorer, etc.), mezzanine/toolbar layout, testing modes (F5/F8), troubleshooting ("my script isn't running")

Read `.roxlit/context/index.md` for an overview of all available packs.

## Roxlit Launcher

This project was set up with Roxlit. The Roxlit launcher manages Rojo automatically.

- **Session logs on disk**: Roxlit captures ALL output to `.roxlit/logs/latest.log` — Rojo AND Roblox Studio console (prints, warnings, errors). You can read this file to diagnose issues without asking the user to copy-paste. Previous sessions are saved as `.roxlit/logs/session-<timestamp>.log` (up to 10 retained).
- **Copy logs from UI**: The user can also click "Copy All" in the Roxlit launcher terminal to copy all logs and paste them here.
- **Do NOT remove or modify the Roxlit-generated sections above.** They are auto-updated by Roxlit when new versions are available.

{USER_NOTES_MARKER}

Add your own project-specific notes, rules, or instructions below this line. Roxlit will preserve this section when updating the context above.

Settings the AI will look for here:
- `Studio language: <language>` — so the AI uses correct localized names for Studio UI elements (e.g., `Studio language: Spanish`)

"#
    )
}
