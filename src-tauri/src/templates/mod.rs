/// Returns the default.project.json content for Rojo.
pub fn project_json(project_name: &str) -> String {
    format!(
        r#"{{
  "name": "{project_name}",
  "tree": {{
    "$className": "DataModel",
    "ServerScriptService": {{
      "$className": "ServerScriptService",
      "Server": {{
        "$path": "src/server"
      }}
    }},
    "StarterPlayer": {{
      "$className": "StarterPlayer",
      "StarterPlayerScripts": {{
        "$className": "StarterPlayerScripts",
        "Client": {{
          "$path": "src/client"
        }}
      }}
    }},
    "ReplicatedStorage": {{
      "$className": "ReplicatedStorage",
      "Shared": {{
        "$path": "src/shared"
      }}
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

/// Returns the AI context file content with Roblox/Luau development instructions.
/// This is the same content regardless of AI tool — only the filename changes.
pub fn ai_context(project_name: &str) -> String {
    format!(
        r#"# {project_name}

Roblox game project using Rojo for file syncing. Write Luau code in `src/` and Rojo syncs it to Roblox Studio in real time.

## Tech Stack

- **Language**: Luau (Roblox's typed Lua dialect)
- **Sync tool**: Rojo (filesystem <-> Roblox DataModel)
- **Type checking**: Strict mode (`--!strict`)

## Project Structure

```
src/
  server/    → ServerScriptService (runs on the Roblox server)
  client/    → StarterPlayerScripts (runs on each player's device)
  shared/    → ReplicatedStorage (accessible from both server and client)
```

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
"#
    )
}
