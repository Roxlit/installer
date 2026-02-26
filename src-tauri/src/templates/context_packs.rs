/// Context packs: curated Roblox documentation for AI assistants.
/// Each function returns a static markdown string with accurate, up-to-date
/// information about a specific Roblox topic. These are written to
/// `.roxlit/context/` inside the user's project.

/// Index file — the AI reads this first to know what context packs are available.
pub fn index() -> &'static str {
    r#"# Roxlit Context Packs

> Last updated: 2025-06 | Roblox Engine v638+

These files contain curated Roblox/Luau documentation to help you write correct code. **Read the relevant file before writing code** that touches that system.

## Available Packs

| File | Topic |
|------|-------|
| `datastore.md` | DataStoreService: throttling, session locking, retry patterns |
| `remote-events.md` | RemoteEvent/RemoteFunction: security, validation, rate limiting |
| `player-lifecycle.md` | PlayerAdded, CharacterAdded, respawn, death, BindToClose |
| `workspace-physics.md` | Parts, CFrame, terrain, collision groups, physics |
| `replication.md` | What replicates, FilteringEnabled rules, RunContext |
| `services-reference.md` | Service properties, enums, valid ranges |
| `studio-ui.md` | Studio UI: panel locations, testing modes, localized names, troubleshooting |

## How to Use

1. When asked to implement something involving a topic above, read that file first
2. Follow the patterns shown — they handle edge cases that commonly cause bugs
3. Pay attention to "NEVER" and "ALWAYS" callouts — these prevent the most common mistakes
4. When in doubt, prefer the server-authoritative pattern
"#
}

/// DataStoreService: throttling, patterns, anti-patterns.
pub fn datastore() -> &'static str {
    r#"# DataStoreService

## Throttling Limits

DataStore requests are throttled per-server. The budget is:

| Operation | Budget per minute |
|-----------|------------------|
| GetAsync | 60 + numPlayers × 10 |
| SetAsync | 60 + numPlayers × 10 |
| UpdateAsync | 60 + numPlayers × 10 |
| RemoveAsync | 60 + numPlayers × 10 |
| GetSortedAsync | 5 + numPlayers × 2 |
| ListKeysAsync | 5 + numPlayers × 2 |

- `numPlayers` = players in that specific server, not the whole game
- Each key can only be updated once every 6 seconds
- Key max length: 50 characters
- Value max size: 4 MB (serialized JSON)
- Data store name max length: 50 characters

## ALWAYS Use pcall

DataStore calls are HTTP requests that **will** fail. Network errors, throttling, and service outages are normal.

```luau
local ok, result = pcall(function()
    return store:GetAsync(key)
end)
if not ok then
    warn(`DataStore error: {result}`)
    -- Handle the failure (retry, use cached data, etc.)
end
```

## UpdateAsync vs SetAsync

**ALWAYS prefer UpdateAsync** for data that can be modified concurrently:

```luau
-- GOOD: UpdateAsync reads the current value, transforms it, and writes atomically
store:UpdateAsync(key, function(currentData)
    currentData = currentData or { coins = 0 }
    currentData.coins += rewardAmount
    return currentData
end)

-- BAD: SetAsync can overwrite data from another server
local data = store:GetAsync(key)  -- Another server could write between these two calls
data.coins += rewardAmount
store:SetAsync(key, data)         -- Overwrites the other server's changes
```

Use SetAsync ONLY for:
- Initial data creation where no prior value exists
- Complete data replacement where you intentionally want to overwrite

## Session Locking Pattern

Prevents data corruption when a player joins a new server before the old one finishes saving:

```luau
local LOCK_EXPIRE = 1800 -- 30 minutes

store:UpdateAsync(key, function(data)
    if data and data._lockServerId and data._lockServerId ~= game.JobId then
        local lockAge = os.time() - (data._lockTime or 0)
        if lockAge < LOCK_EXPIRE then
            return nil -- Abort: another server owns this data
        end
    end
    -- Claim the lock
    data = data or {}
    data._lockServerId = game.JobId
    data._lockTime = os.time()
    return data
end)
```

## Retry with Exponential Backoff

```luau
local function retryAsync<T>(callback: () -> T, maxRetries: number?): (boolean, T | string)
    local retries = maxRetries or 3
    for attempt = 1, retries do
        local ok, result = pcall(callback)
        if ok then
            return true, result
        end
        if attempt < retries then
            task.wait(2 ^ attempt) -- 2s, 4s, 8s
        else
            return false, result
        end
    end
    return false, "Max retries exceeded" :: any
end
```

## BindToClose for Saving on Shutdown

```luau
game:BindToClose(function()
    -- You have ~30 seconds before the server shuts down
    local threads = {}
    for _, player in Players:GetPlayers() do
        table.insert(threads, task.spawn(function()
            savePlayerData(player)
        end))
    end
    -- Wait for all saves to complete
    for _, thread in threads do
        if coroutine.status(thread) ~= "dead" then
            task.wait()
        end
    end
end)
```

## Anti-Patterns

- **No batch operations**: You cannot get/set multiple keys in one call. Loop through them individually
- **No queries**: You cannot search by value. Use OrderedDataStore or maintain your own index
- **No transactions across keys**: UpdateAsync is atomic for ONE key only
- **No partial updates**: You must read the full value and write the full value back
- **NEVER store Instance references**: They cannot be serialized. Store names/paths instead
- **NEVER use player.Name as the key**: Names can change. Use `player.UserId`
"#
}

/// RemoteEvent/RemoteFunction: security, validation, patterns.
pub fn remote_events() -> &'static str {
    r#"# RemoteEvent & RemoteFunction

## The #1 Rule

**The server MUST validate ALL input from the client.** Exploiters can fire any RemoteEvent with any arguments. Never trust the client.

## OnServerEvent: First Argument is Always Player

```luau
-- SERVER
remote.OnServerEvent:Connect(function(player: Player, arg1, arg2)
    -- `player` is injected by the engine — the client cannot spoof it
    -- arg1, arg2... are what the client sent — NEVER trust them
end)

-- CLIENT
remote:FireServer(arg1, arg2) -- Do NOT send the player, it's automatic
```

## Type Checking Arguments

Exploiters can send any Luau type. Always validate:

```luau
remote.OnServerEvent:Connect(function(player: Player, targetName: unknown, amount: unknown)
    -- Validate types
    if typeof(targetName) ~= "string" then return end
    if typeof(amount) ~= "number" then return end

    -- Validate ranges
    if amount ~= amount then return end -- NaN check
    if amount <= 0 or amount > 100 then return end
    if math.floor(amount) ~= amount then return end -- Integer check
    if #targetName > 50 then return end -- Length check

    -- Now safe to use
end)
```

## Rate Limiting

Prevent exploiters from spamming remotes:

```luau
local lastFired: { [Player]: number } = {}
local COOLDOWN = 0.5 -- seconds

remote.OnServerEvent:Connect(function(player: Player, ...)
    local now = os.clock()
    if lastFired[player] and now - lastFired[player] < COOLDOWN then
        return -- Too fast, ignore
    end
    lastFired[player] = now

    -- Process the event...
end)

-- Clean up on leave
Players.PlayerRemoving:Connect(function(player: Player)
    lastFired[player] = nil
end)
```

## Complete Pattern: Damage System

```luau
-- SERVER (ServerScriptService)
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Players = game:GetService("Players")

local damageRemote = Instance.new("RemoteEvent")
damageRemote.Name = "RequestDamage"
damageRemote.Parent = ReplicatedStorage

local MAX_DAMAGE = 50
local MAX_RANGE = 20
local COOLDOWN = 0.3

local lastAttack: { [Player]: number } = {}

damageRemote.OnServerEvent:Connect(function(player: Player, targetPlayer: unknown)
    -- 1. Rate limit
    local now = os.clock()
    if lastAttack[player] and now - lastAttack[player] < COOLDOWN then
        return
    end
    lastAttack[player] = now

    -- 2. Validate argument type (exploiters could send anything)
    if typeof(targetPlayer) ~= "Instance" or not targetPlayer:IsA("Player") then
        return
    end
    local target: Player = targetPlayer :: Player

    -- 3. Validate target exists and is alive
    local targetChar = target.Character
    local attackerChar = player.Character
    if not targetChar or not attackerChar then return end

    local targetHumanoid = targetChar:FindFirstChildOfClass("Humanoid")
    local attackerHumanoid = attackerChar:FindFirstChildOfClass("Humanoid")
    if not targetHumanoid or targetHumanoid.Health <= 0 then return end
    if not attackerHumanoid or attackerHumanoid.Health <= 0 then return end

    -- 4. Validate range (server-side distance check)
    local targetRoot = targetChar:FindFirstChild("HumanoidRootPart")
    local attackerRoot = attackerChar:FindFirstChild("HumanoidRootPart")
    if not targetRoot or not attackerRoot then return end

    local distance = (targetRoot.Position - attackerRoot.Position).Magnitude
    if distance > MAX_RANGE then return end

    -- 5. Apply damage (server-authoritative)
    targetHumanoid:TakeDamage(MAX_DAMAGE)
end)

Players.PlayerRemoving:Connect(function(player: Player)
    lastAttack[player] = nil
end)
```

## RemoteFunction Caveats

- `InvokeClient` is **dangerous**: if the client errors or disconnects, the server thread hangs forever
- NEVER use `InvokeClient` in production. Use RemoteEvent + RemoteEvent for two-way communication
- `InvokeServer` is safe: the server always responds (or errors, which you can pcall)

```luau
-- SAFE: Client invokes server
local result = remoteFunc:InvokeServer(someArg)

-- DANGEROUS: Server invokes client — AVOID
-- remoteFunc:InvokeClient(player, someArg) -- Can hang forever
```

## Anti-Patterns

- **NEVER create RemoteEvents from the client**: Exploiters control client-created instances
- **NEVER use RemoteEvent.Name to determine action**: Use separate remotes or validate a string action parameter
- **NEVER pass functions through remotes**: Functions cannot cross the network boundary
- **NEVER assume argument count**: Exploiters can send more or fewer arguments than expected
"#
}

/// Player lifecycle: join, character, respawn, death, cleanup.
pub fn player_lifecycle() -> &'static str {
    r#"# Player Lifecycle

## The Complete Flow

```
Player joins server
  └─ Players.PlayerAdded fires
       └─ player.CharacterAdded fires (character spawns)
            └─ Character has: Humanoid, HumanoidRootPart, Head, etc.
                 └─ Humanoid.Died fires (player dies)
                      └─ Character is destroyed after RespawnTime
                           └─ player.CharacterAdded fires again (respawn)

Player leaves server
  └─ Players.PlayerRemoving fires
       └─ Save data, clean up state
```

## PlayerAdded: Handle Existing Players

Players might already be in the server when your script runs (especially in Studio):

```luau
local Players = game:GetService("Players")

local function onPlayerAdded(player: Player)
    -- Setup logic here
    print(`{player.Name} joined`)

    -- Connect CharacterAdded
    player.CharacterAdded:Connect(function(character)
        onCharacterAdded(player, character)
    end)

    -- Handle character that already exists (rare, but possible)
    if player.Character then
        onCharacterAdded(player, player.Character)
    end
end

-- Connect for future players
Players.PlayerAdded:Connect(onPlayerAdded)

-- Handle players already in the server
for _, player in Players:GetPlayers() do
    task.spawn(onPlayerAdded, player)
end
```

## CharacterAdded: Safe Access to Parts

```luau
local function onCharacterAdded(player: Player, character: Model)
    -- WaitForChild is necessary — parts load asynchronously
    local humanoid = character:WaitForChild("Humanoid") :: Humanoid
    local rootPart = character:WaitForChild("HumanoidRootPart") :: BasePart

    -- Set properties
    humanoid.MaxHealth = 200
    humanoid.Health = 200

    -- Handle death
    humanoid.Died:Connect(function()
        print(`{player.Name} died`)
        -- Do NOT access character parts here — they may already be destroying
    end)
end
```

## NEVER Access Character Parts Without Checking

```luau
-- BAD: Character might be nil (player is respawning)
local root = player.Character.HumanoidRootPart -- Error if Character is nil

-- GOOD: Check at every step
local character = player.Character
if not character then return end
local root = character:FindFirstChild("HumanoidRootPart")
if not root then return end
-- Now safe to use root
```

## Respawn Handling

```luau
-- Change respawn time
Players.RespawnTime = 5 -- seconds (default is 5)

-- Do something specific on respawn (not first spawn)
local hasSpawned: { [Player]: boolean } = {}

player.CharacterAdded:Connect(function(character)
    if hasSpawned[player] then
        -- This is a respawn
        print(`{player.Name} respawned`)
    else
        -- First spawn
        hasSpawned[player] = true
    end
end)
```

## Animation Loading

```luau
local function onCharacterAdded(player: Player, character: Model)
    local humanoid = character:WaitForChild("Humanoid") :: Humanoid
    local animator = humanoid:WaitForChild("Animator") :: Animator

    -- Load animation
    local animAsset = Instance.new("Animation")
    animAsset.AnimationId = "rbxassetid://123456789"
    local animTrack = animator:LoadAnimation(animAsset)

    -- Play it
    animTrack:Play()

    -- Stop it later
    task.delay(3, function()
        animTrack:Stop()
    end)
end
```

## BindToClose: Save Data on Server Shutdown

The server shuts down when all players leave, during updates, or when force-closed. You have ~30 seconds:

```luau
local Players = game:GetService("Players")

Players.PlayerRemoving:Connect(function(player: Player)
    savePlayerData(player)
end)

-- BindToClose handles the case where the server shuts down
-- while players are still connected (Players.PlayerRemoving
-- might not fire for all players during a shutdown)
game:BindToClose(function()
    for _, player in Players:GetPlayers() do
        task.spawn(savePlayerData, player)
    end
    task.wait(3) -- Give saves time to complete
end)
```

## Anti-Patterns

- **NEVER assume Character exists**: Always nil-check `player.Character`
- **NEVER store references to character parts long-term**: The character is destroyed on death and a new one is created
- **NEVER use `player.Character = nil` to kill**: Use `humanoid.Health = 0` or `humanoid:TakeDamage()`
- **NEVER skip WaitForChild for character parts**: Parts load asynchronously, even HumanoidRootPart
"#
}

/// Workspace, Parts, CFrame, terrain, physics.
pub fn workspace_physics() -> &'static str {
    r#"# Workspace & Physics

## Part Properties

| Property | Type | Notes |
|----------|------|-------|
| `Size` | Vector3 | Min 0.05 per axis, max 2048 |
| `Position` | Vector3 | World position of the part's center |
| `CFrame` | CFrame | Position + orientation (preferred over Position) |
| `Anchored` | boolean | If true, physics doesn't affect this part |
| `CanCollide` | boolean | If true, other parts collide with it |
| `CanQuery` | boolean | If true, raycasts can hit it |
| `CanTouch` | boolean | If true, .Touched event fires |
| `Transparency` | number | 0 = opaque, 1 = invisible |
| `Material` | Enum.Material | SmoothPlastic, Wood, Metal, Glass, etc. |
| `Color` | Color3 | Use Color3.fromRGB() or Color3.fromHex() |
| `Massless` | boolean | If true, doesn't contribute to assembly mass |

## CFrame: ALWAYS Use for Positioning Assemblies

When parts are welded together (an "assembly"), setting `Position` only moves that one part and breaks welds. Use `CFrame` instead:

```luau
-- BAD: Breaks welds in assemblies
part.Position = Vector3.new(0, 10, 0)

-- GOOD: Moves the entire assembly correctly
part.CFrame = CFrame.new(0, 10, 0)

-- Move a model (set PrimaryPart first!)
model.PrimaryPart = model:FindFirstChild("MainPart")
model:PivotTo(CFrame.new(0, 10, 0))
```

## Common CFrame Operations

```luau
-- Create at position
local cf = CFrame.new(10, 5, -20)

-- Create at position, looking at target
local cf = CFrame.lookAt(Vector3.new(0, 10, 0), Vector3.new(10, 0, 0))

-- Rotate (angles in radians)
local cf = CFrame.Angles(0, math.rad(90), 0) -- 90° around Y axis

-- Combine position + rotation
local cf = CFrame.new(10, 5, 0) * CFrame.Angles(0, math.rad(45), 0)

-- Relative offset (move 5 studs forward in the part's facing direction)
local newCf = part.CFrame * CFrame.new(0, 0, -5) -- -Z is forward

-- Get look direction
local lookVector = part.CFrame.LookVector -- Unit vector pointing forward

-- Lerp between two CFrames (smooth interpolation)
local result = cf1:Lerp(cf2, 0.5) -- Halfway between cf1 and cf2
```

## Raycasting

```luau
local origin = Vector3.new(0, 50, 0)
local direction = Vector3.new(0, -100, 0) -- Downward

local params = RaycastParams.new()
params.FilterType = Enum.RaycastFilterType.Exclude
params.FilterDescendantsInstances = { character } -- Ignore the player's character

local result = workspace:Raycast(origin, direction, params)
if result then
    local hitPart = result.Instance
    local hitPosition = result.Position
    local hitNormal = result.Normal
    local hitMaterial = result.Material
end
```

## Collision Groups

```luau
local PhysicsService = game:GetService("PhysicsService")

-- Register groups (do this once, usually in a server script)
PhysicsService:RegisterCollisionGroup("Players")
PhysicsService:RegisterCollisionGroup("Bullets")

-- Set collision rules
PhysicsService:CollisionGroupSetCollidable("Players", "Bullets", false) -- Bullets pass through players

-- Assign parts to groups
part.CollisionGroup = "Players"
bullet.CollisionGroup = "Bullets"
```

## Terrain

```luau
local terrain = workspace.Terrain

-- Fill a region with material
local region = Region3.new(
    Vector3.new(-50, 0, -50), -- Min corner
    Vector3.new(50, 10, 50)   -- Max corner
)
terrain:FillRegion(region:ExpandToGrid(4), 4, Enum.Material.Grass)

-- Fill a sphere
terrain:FillBall(Vector3.new(0, 10, 0), 20, Enum.Material.Water)

-- Read terrain at a position
local material, occupancy = terrain:ReadVoxels(region:ExpandToGrid(4), 4)
```

## PrimaryPart

Every Model that you want to move should have a PrimaryPart set:

```luau
-- Set it
model.PrimaryPart = model:FindFirstChild("RootPart")

-- Then move the entire model
model:PivotTo(CFrame.new(0, 10, 0))

-- Get model's position
local modelCFrame = model:GetPivot()
```

## Workspace Properties

| Property | Default | Notes |
|----------|---------|-------|
| `Gravity` | 196.2 | Studs/sec². Earth-like gravity |
| `FallenPartsDestroyHeight` | -500 | Y level where parts are destroyed |
| `StreamingEnabled` | true | Instance streaming for large places |

## Instance Organization

ALWAYS group related parts into a Model. Never leave loose Parts or Scripts in Workspace root.

### Structure Pattern

```
-- GOOD: Everything grouped in a Model
Door (Model, PrimaryPart = DoorPart)
  ├── DoorPart (Part, the part that moves)
  │   └── ProximityPrompt (triggers interaction)
  ├── Frame (Part)
  ├── Wall (Part)
  └── DoorController (Script, controls this door)

-- BAD: Loose parts scattered in Workspace
Workspace/
  Door (Part)
  DoorFrame (Part)
  DoorWall (Part)
  DoorController (Script)  ← where does this belong? unclear
```

### Rules

- **Group related parts in a Model**: Door + frame + wall + script = one Model
- **Set PrimaryPart**: Every Model needs a PrimaryPart for `PivotTo()` to work
- **Scripts inside their Model**: A script that controls a door goes inside the Door Model. Game-wide scripts go in ServerScriptService
- **Name descriptively**: `DoorPart`, `Frame`, `Wall` — not `Part`, `Part2`
- **Nested Models for complex objects**: A house could have sub-Models for each room, each door, etc.

```luau
-- Creating a well-organized Model
local doorModel = Instance.new("Model")
doorModel.Name = "Door"
doorModel.Parent = workspace

local doorPart = Instance.new("Part")
doorPart.Name = "DoorPart"
doorPart.Parent = doorModel

doorModel.PrimaryPart = doorPart -- ALWAYS set this

local prompt = Instance.new("ProximityPrompt")
prompt.Parent = doorPart -- Prompt goes on the interactable part
```

### Script Placement Guide

| Script type | Where it goes | Example |
|-------------|---------------|---------|
| Controls a specific object | Inside that object's Model | DoorController inside Door Model |
| Game-wide system | `scripts/ServerScriptService/` | RoundManager, DataSaveSystem |
| Player-specific client logic | `scripts/StarterPlayer/StarterPlayerScripts/` | CameraController, InputHandler |
| Character-specific logic | `scripts/StarterPlayer/StarterCharacterScripts/` | AnimationController |
| Shared utilities | `scripts/ReplicatedStorage/` | Utility modules used by both server and client |
| GUI logic | `scripts/StarterGui/` | Menu handlers, HUD updates |

## Z-Fighting (Overlapping Surfaces)

When two surfaces occupy the same position, the renderer flickers between them (Z-fighting). This is common when building frames, walls, doors, and decorative trim.

**ALWAYS offset overlapping surfaces by at least 0.01 studs.** Or make one part slightly thicker so surfaces don't share the same plane.

```luau
-- BAD: Wall and frame share the same surface — Z-fighting
wall.Size = Vector3.new(10, 10, 1)
frame.Size = Vector3.new(2, 7, 1) -- Same depth as wall
frame.Position = wall.Position -- Surfaces overlap

-- GOOD: Frame is slightly thicker than the wall
wall.Size = Vector3.new(10, 10, 1)
frame.Size = Vector3.new(2, 7, 1.05) -- 0.05 studs thicker — no overlap
frame.Position = wall.Position
```

Rules:
- **Frames/trim around doors or windows**: Make the frame 0.05–0.1 studs thicker than the wall on each visible side
- **Decals on Parts**: Use Decal or SurfaceGui objects instead of overlapping thin Parts
- **Floors on terrain**: Raise floor parts at least 0.05 studs above terrain level

## Anti-Patterns

- **NEVER set Position on welded parts**: Use CFrame or PivotTo
- **NEVER forget to set PrimaryPart on models**: PivotTo won't work correctly without it
- **NEVER use deprecated BodyVelocity/BodyForce**: Use LinearVelocity, AlignPosition, VectorForce constraints instead
- **NEVER create thousands of unanchored parts**: Causes massive physics lag. Anchor parts that don't need physics
- **NEVER place two surfaces at the exact same position**: Causes Z-fighting (texture flickering). Offset by at least 0.01 studs
"#
}

/// Replication rules: what replicates, FilteringEnabled, RunContext.
pub fn replication() -> &'static str {
    r#"# Replication

## The Core Rule

**FilteringEnabled is always on.** The client CANNOT modify the server's DataModel. Changes the client makes to instances are local-only (other clients and the server don't see them).

## What Replicates (Server → Client)

| Container | Replicates to clients? | Notes |
|-----------|----------------------|-------|
| `Workspace` | Yes | All descendants replicate |
| `Lighting` | Yes | Properties + descendants |
| `ReplicatedFirst` | Yes | Loads before anything else |
| `ReplicatedStorage` | Yes | Shared modules and assets |
| `StarterGui` | Yes | Cloned into PlayerGui on spawn |
| `StarterPack` | Yes | Cloned into Backpack on spawn |
| `StarterPlayer` | Yes | StarterPlayerScripts, StarterCharacterScripts |
| `SoundService` | Yes | Ambient sounds |
| `Chat` | Yes | Chat system |
| `Teams` | Yes | Team data |

## What Does NOT Replicate (Server-Only)

| Container | Notes |
|-----------|-------|
| `ServerScriptService` | Server scripts — clients NEVER see these |
| `ServerStorage` | Server assets — clients NEVER see these |

**NEVER put secrets, admin tools, or server logic in ReplicatedStorage.** Exploiters can read everything that replicates.

## Client → Server: Nothing Replicates Automatically

Changes made on the client do NOT reach the server. To send data from client to server, use:
- `RemoteEvent:FireServer()` for one-way messages
- `RemoteFunction:InvokeServer()` for request-response

## Property Replication Direction

| Scenario | What happens |
|----------|-------------|
| Server sets `part.Color` | Change replicates to ALL clients |
| Client sets `part.Color` | Only visible to THAT client. Server and other clients see the old color |
| Server creates an Instance in Workspace | Replicates to all clients |
| Client creates an Instance in Workspace | Only visible to that client |
| Server destroys an Instance | Destroyed for everyone |
| Client destroys a replicated Instance | Only hidden locally, server still has it |

## RunContext (Modern Script Execution)

Instead of Script vs LocalScript, modern Roblox uses `RunContext`:

| RunContext | Where it runs | Equivalent to |
|-----------|---------------|---------------|
| `Server` | Server only | Script |
| `Client` | Each client | LocalScript |
| `Legacy` | Depends on container | Old behavior |

With Rojo, file extensions determine this:
- `*.server.luau` → RunContext.Server
- `*.client.luau` → RunContext.Client
- `*.luau` → ModuleScript (runs where required from)

## Common Replication Mistakes

### Mistake 1: Putting server data in ReplicatedStorage
```luau
-- BAD: Clients can see admin list
local admins = ReplicatedStorage.AdminList -- Exploiters read this

-- GOOD: Keep it in ServerStorage or ServerScriptService
local admins = ServerStorage.AdminList
```

### Mistake 2: Trusting client-side changes
```luau
-- BAD: Client sets their own health (server doesn't see this)
-- In a LocalScript:
humanoid.Health = 999 -- Only visual, server still has the real value

-- GOOD: Use a RemoteEvent to request server-side changes
```

### Mistake 3: Expecting client changes to sync to other clients
```luau
-- BAD: Creating a part on the client thinking others will see it
-- In a LocalScript:
local part = Instance.new("Part")
part.Parent = workspace -- Only THIS client sees it

-- GOOD: Fire a remote, let the server create it
```

## StreamingEnabled

When `workspace.StreamingEnabled = true` (default for new places):
- Clients only receive instances near their character
- Distant parts/models are not loaded on the client
- Use `WaitForChild` or `StreamingTarget` if you need specific instances
- `workspace:GetPartBoundsInRadius()` only returns loaded parts on client
"#
}

/// Common service properties, enums, valid ranges.
pub fn services_reference() -> &'static str {
    r#"# Services Quick Reference

## Lighting

| Property | Type | Range/Values | Default |
|----------|------|-------------|---------|
| `Ambient` | Color3 | RGB color | (127, 127, 127) |
| `Brightness` | number | 0–10 | 2 |
| `ColorShift_Bottom` | Color3 | RGB | (0, 0, 0) |
| `ColorShift_Top` | Color3 | RGB | (0, 0, 0) |
| `EnvironmentDiffuseScale` | number | 0–1 | 1 |
| `EnvironmentSpecularScale` | number | 0–1 | 1 |
| `GlobalShadows` | boolean | | true |
| `OutdoorAmbient` | Color3 | RGB | (127, 127, 127) |
| `ClockTime` | number | 0–24 | 14 |
| `GeographicLatitude` | number | -90 to 90 | 41.733 |
| `TimeOfDay` | string | "HH:MM:SS" | "14:00:00" |
| `Technology` | Enum.Technology | ShadowMap, Future, Voxel | ShadowMap |

### Post-Processing Effects (children of Lighting)

- `BloomEffect`: Intensity, Size, Threshold
- `BlurEffect`: Size (0–56)
- `ColorCorrectionEffect`: Brightness, Contrast, Saturation, TintColor
- `DepthOfFieldEffect`: FarIntensity, FocusDistance, InFocusRadius, NearIntensity
- `SunRaysEffect`: Intensity, Spread
- `Atmosphere`: Density, Offset, Color, Decay, Glare, Haze

## SoundService

| Property | Type | Default |
|----------|------|---------|
| `AmbientReverb` | Enum.ReverbType | NoReverb |
| `DistanceFactor` | number | 3.33 |
| `DopplerScale` | number | 1 |
| `RolloffScale` | number | 1 |
| `RespectFilteringEnabled` | boolean | true |

### Sound Properties

| Property | Type | Notes |
|----------|------|-------|
| `SoundId` | string | `"rbxassetid://123456"` |
| `Volume` | number | 0–10, default 0.5 |
| `PlaybackSpeed` | number | 0.01–10, default 1 |
| `Looped` | boolean | default false |
| `RollOffMode` | Enum.RollOffMode | Inverse, Linear, InverseTapered, LinearSquare |
| `RollOffMaxDistance` | number | Max hearing distance |
| `RollOffMinDistance` | number | Distance where falloff starts |
| `PlayOnRemove` | boolean | Plays when removed from DataModel |

## Common Enums

### Enum.Material (for Parts)
`Plastic` `SmoothPlastic` `Wood` `WoodPlanks` `Marble` `Slate` `Concrete` `Granite` `Brick` `Pebble` `Cobblestone` `CorrodedMetal` `DiamondPlate` `Foil` `Metal` `Grass` `Ice` `Sand` `Fabric` `Glass` `Neon` `ForceField` `LeafyGrass` `Limestone` `Pavement` `Asphalt` `Basalt` `CrackedLava` `Glacier` `Ground` `Mud` `Rock` `Salt` `Sandstone` `Snow`

### Enum.PartType (for Part.Shape)
`Block` `Ball` `Cylinder` `Wedge` `CornerWedge`

### Enum.HumanoidStateType
`Running` `Jumping` `Freefall` `Climbing` `Swimming` `Dead` `Physics` `Seated` `StrafingNoPhysics` `Ragdoll` `GettingUp` `FallingDown` `Landed` `PlatformStanding`

### Enum.KeyCode (common keys)
`W` `A` `S` `D` `Space` `LeftShift` `LeftControl` `E` `F` `Q` `R` `Tab` `Return` `Escape` `One` through `Nine` `F1` through `F12`

### Enum.UserInputType
`MouseButton1` `MouseButton2` `MouseButton3` `MouseMovement` `MouseWheel` `Touch` `Keyboard` `Gamepad1`

### Enum.EasingStyle (for TweenService)
`Linear` `Sine` `Back` `Quad` `Quart` `Quint` `Bounce` `Elastic` `Exponential` `Circular` `Cubic`

### Enum.EasingDirection
`In` `Out` `InOut`

## TweenService

```luau
local TweenService = game:GetService("TweenService")

local tweenInfo = TweenInfo.new(
    1,                          -- Duration (seconds)
    Enum.EasingStyle.Quad,      -- Easing style
    Enum.EasingDirection.Out,   -- Easing direction
    0,                          -- RepeatCount (0 = no repeat, -1 = infinite)
    false,                      -- Reverses
    0                           -- DelayTime
)

local tween = TweenService:Create(part, tweenInfo, {
    Position = Vector3.new(0, 20, 0),
    Transparency = 0.5,
})

tween:Play()
tween.Completed:Wait() -- Yields until done
```

## RunService

| Event | When it fires | Context |
|-------|--------------|---------|
| `Heartbeat` | Every frame, after physics | Server + Client |
| `RenderStepped` | Every frame, before rendering | **Client only** |
| `Stepped` | Every frame, before physics | Server + Client |

```luau
-- Check environment
RunService:IsServer()  -- true on server
RunService:IsClient()  -- true on client
RunService:IsStudio()  -- true in Studio (both server and client)
```

## UserInputService (Client Only)

```luau
local UIS = game:GetService("UserInputService")

UIS.InputBegan:Connect(function(input: InputObject, gameProcessed: boolean)
    if gameProcessed then return end -- Ignore if the user is typing in chat, etc.

    if input.KeyCode == Enum.KeyCode.E then
        -- E was pressed
    end
end)

-- Check if a key is held
local isShiftHeld = UIS:IsKeyDown(Enum.KeyCode.LeftShift)

-- Detect input device
local isMobile = UIS.TouchEnabled and not UIS.KeyboardEnabled
```
"#
}

/// Roblox Studio UI: layout, toolbar contents, panel locations, testing, troubleshooting.
/// Source: verified from actual Studio screenshots (Feb 2026), new Flexible UI.
pub fn studio_ui() -> &'static str {
    r#"# Roblox Studio UI

> Verified from actual Studio screenshots — Feb 2026, new Flexible UI (default since Jan 2026).

## How to Guide Users

**Studio may be in ANY language.** All menu names, button labels, and panel names are localized. The names in this document are the **English reference names** — the user may see completely different labels depending on their Studio language.

**Check the user's Studio language first:** Look in the project's AI context file (CLAUDE.md, .cursorrules, etc.) under "Your Notes" for a `Studio language:` entry. If it exists, use the correct localized names for that language. If it doesn't exist, **ask the user** what language their Studio is in, and tell them to add `Studio language: <language>` to the "Your Notes" section so you remember next time.

**Rules for giving UI directions:**
1. **Describe by position first**: "the 3rd group of icons in the toolbar" or "the far-left dropdown in the mezzanine"
2. **Describe by function**: "the panel where print() messages appear"
3. **Use English names only as reference** — when talking to the user, describe what the element does or where it is. If you know their Studio language, use the localized name. If not, describe by position/function and mention the English reference name in parentheses.
4. **Respond in the user's language** — always.

## Studio Layout — Three Zones at the Top

```
┌───────────────────────────────────────────────────────────────────────┐
│  File  Edit  View  Plugins  Test  Window  Help                        │  ← MENU BAR
├───────────────────────────────────────────────────────────────────────┤
│ [Test▾] ▶ ⏸▾ ⏹ ██ │ Home Avatar UI Script Model Plugins + │ [Collab] ⚙🔔👤 │ ← MEZZANINE
├───────────────────────────────────────────────────────────────────────┤
│  [Icon] [Icon] | [Icon] [Icon] | [Icon] ...  (changes per tab)       │  ← TOOLBAR
└───────────────────────────────────────────────────────────────────────┘
```

### 1. Menu Bar (topmost, below title bar)
Traditional dropdown menu: **File | Edit | View | Plugins | Test | Window | Help** (7 items).

Key menus:
- **Window**: Opens/closes ANY panel (Output, Explorer, Properties, etc.) — universal fallback when you can't find a panel
- **View**: Camera and viewport display options
- **Test**: Same testing options as the mezzanine play controls
- **There is no "View" tab** in the new UI — the old View tab was replaced by the Window menu

### 2. Mezzanine (below menu bar)
Three sections side by side:

**Left — Play/Test controls** (always visible regardless of tab):
- Test mode dropdown (4 options: Play, Run, Play Here, Start Server)
- Play ▶ (starts the selected test mode)
- Pause ⏸ (has dropdown: pause everything, or pause client only)
- Stop ⏹
- Switch Client/Server view (toggles the viewport between client and server perspective)
- **When a test is running**: a divider line appears and a **Step Forward** button shows up (advances one simulation frame — useful for debugging physics/animations frame by frame)

**Center — Tool tabs**: `Home | Avatar | UI | Script | Model | Plugins | +`
- The **+** button lets users create **custom toolbar tabs** with any combination of tools
- Clicking a tab changes the entire TOOLBAR row below
- Tabs are reorderable and customizable (right-click for options)

**Right — Account/collaboration**:
- Collaborate button (green), Settings gear, Notification bell, User avatar

### 3. Toolbar (below mezzanine)
A row of icon buttons that **changes entirely** based on which tab is selected. Buttons are grouped by function with thin vertical separators between groups.

## Toolbar Contents by Tab

### Home tab
1. **Transform**: Select, Move, Scale, Rotate, Transform, Geometric constraints + snap settings (studs increment, degree increment)
2. **Insert**: Part, Terrain, Character (rig), GUI, Script, Import
3. **Appearance**: Material, Color, Group, Lock, Anchor
4. **Panels**: Explorer, Properties, Toolbox, Asset Manager

### Model tab
1. **Transform**: Select, Move, Scale, Rotate, Transform, Geometric + snap (degrees)
2. **Pivot**: Pivot, Reset pivot
3. **Insert**: Part, Effect
4. **Constraints**: Connection, Constraint, Weld
5. **Appearance**: Material, Color, Group, Lock, Anchor
6. **Solid modeling**: Align, Union, Intersect, Separate, Negate

### Avatar tab
1. **Transform**: Select, Move, Scale, Rotate, Transform, Geometric + snap settings
2. **Avatar**: Avatar setup/import
3. **Character**: Character (rig builder), Configure, Animation, Accessory

### UI tab
1. **UI elements**: GUI (ScreenGui), Frame, Label, TextInput, Appearance, Layout, UIConstraint
2. **Editor**: UI Editor panel

### Script tab
1. **Navigation**: Back, Forward (navigate between script locations)
2. **Script tools**: Script (insert new), Format, Find, Go to Line
3. **Panels**: Command Bar, **Output** — this is the primary place to open Output
4. **Debugging**: Breakpoints, Call Stack, Watch/Inspector
5. **Analysis**: Script Analysis, Script Activity

### Plugins tab
Shows installed Studio plugins. After a **thicker separator line**, third-party plugins appear (e.g., Rojo, RbxSync installed by Roxlit). Studio uses this visual separator to distinguish built-in tools from community/user-installed plugins.

## How to Open Key Panels

### Output (print/warn/error messages)
1. Click **Script** tab (center of mezzanine) → **Output** button in toolbar (3rd group, next to Command Bar)
2. OR: **Window** menu (in the menu bar) → Output
- Docks at the **bottom** of the screen

### Command Bar (execute Luau directly, like a REPL)
1. Click **Script** tab → **Command Bar** in toolbar (3rd group, next to Output)
2. OR: **Window** menu → Command Bar
- Appears at the **very bottom**, below Output

### Explorer (DataModel tree — every instance in the game)
- **Visible by default** on the **right side**
- If closed: **Home** tab toolbar (last group) → Explorer button
- OR: **Window** menu → Explorer

### Properties (edit selected instance)
- **Visible by default** on the **right side**, below Explorer
- If closed: **Home** tab toolbar (last group) → Properties button
- OR: **Window** menu → Properties

### Script Analysis (static Luau warnings/errors)
- Click **Script** tab → **Script Analysis** in toolbar (5th group)
- OR: **Window** menu → Script Analysis

## Testing Modes

**Scripts do NOT run in edit mode.** The user MUST start a test session first.

The test mode dropdown on the **far left of the mezzanine** has **4 options**:

| Mode | Shortcut | What it does |
|------|----------|-------------|
| **Play** | **F5** | Starts local server + client. Required for RemoteEvents, player lifecycle |
| **Run** | **F8** | Starts server only. For server-side testing without a player |
| **Play Here** | — | Starts Play but spawns the character at the camera position |
| **Start** | — | Starts a multi-client test server (for network testing) |

**Stop**: **Shift+F5** — stops the test, returns to edit mode.

Play/Test controls are **always visible** on the left side of the mezzanine, no matter which tab is selected.

## Common User Issues

### "My script isn't running"
1. **Did you press Play (F5)?** — scripts don't execute in edit mode. Look for the Play ▶ button on the far left of the mezzanine
2. Check the script is in the correct service (server → ServerScriptService, client → StarterPlayerScripts)
3. Open **Output** (Script tab → Output button in the toolbar, 3rd group) and look for errors
4. Verify the script synced from Rojo: check Explorer for the script under the correct service

### "I can't see print() output"
1. **Output panel might be closed** → Script tab → Output button in the toolbar (3rd group)
2. Make sure you pressed **Play (F5)** first — print() only runs during a test session
3. Check the Output filter — it might be hiding non-error messages

### "I can't find [button/panel]"
1. The toolbar is **fully customizable** — the user may have hidden, moved, or rearranged tabs and buttons
2. The **+** button in the mezzanine lets users add custom tabs — their layout may differ from defaults
3. Most panels can ALWAYS be found via the **Window** menu in the menu bar
4. If completely lost: right-click the mezzanine → reset to default layout

### "Changes aren't syncing to Studio"
1. Check Rojo is running (green status in Roxlit launcher)
2. Check the **Rojo plugin is connected**: Plugins tab → after the thick separator line, look for the Rojo button → should show "Connected"
3. If "Not Connected": click the Rojo button to reconnect
4. If the Rojo plugin doesn't appear: **restart Studio** (plugins load on startup only)

### "Instance sync (RbxSync) isn't working"
1. RbxSync plugin needs manual activation each session: **Plugins tab → after the thick separator → RbxSync → Sync/Connect**
2. Roxlit must be running `rbxsync serve` (check the launcher)
3. If plugin doesn't appear: restart Studio
"#
}

