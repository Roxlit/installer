/// Context pack: session Studios (parallel chats, one Studio per worktree) and device
/// helpers for layout checks from the MCP. Written to `.roxlit/context/studio-sessions.md`.
pub fn studio_sessions() -> &'static str {
    r#"# Session Studios and Device Helpers

> Applies when several AI chats work on one machine, each in its own git worktree.

## Which Studio is yours

- A chat in a **git worktree** of a Rojo project gets its **own session Studio**:
  opened on its first Studio tool call (about 10 s), on a hidden desktop, synced to that
  worktree by its own `rojo serve`. The MCP lists it by its place file name,
  `roxlit-<token>.rbxl`.
- A hook rewrites `studio_id` on every Studio call to your session Studio. If it blocks
  with "call list_roblox_studios first", do exactly that and retry: the hook learns the
  id from that list.
- A chat in the **main checkout** is not routed: it uses the developer's own Studio.
  Confirm with the developer before changing their game.
- Your Studio closes about 2.5 min after your turn ends and reopens on your next Studio
  call. Anything that is not on disk (instances made with `execute_luau`, play state)
  is gone after a reopen. Put what must survive in the worktree's files.
- Playtests are capped at 2 per machine. If `start_stop_play` is refused for a slot, do
  work that does not need play, or wait with the command the message gives (in the
  background). Stop play as soon as you are done: that frees the slot at once.

## Device simulator from the MCP

`StudioDeviceSimulatorService` works from `execute_luau`, but only in the **Client**
datamodel of a running playtest (in Play mode the Edit datamodel is gone). It simulates
screen size, pixel density, on-screen keyboard and platform. It does **not** simulate
memory pressure, thermal throttling, frame rate or input latency: memory bugs stay
green in the simulator.

```lua
-- Run in the Client datamodel of a playtest.
-- preset: "phone" | "tablet" | "desktop"; orientation: "portrait" | "landscape"
local SHAPES = {
	phone = { device = "iphone_6_Plus", long = 842, short = 392 },
	tablet = { device = "ipad_9th_generation" },
	desktop = { device = "hd_1080" },
}
local function setDevice(preset, orientation)
	local sim = game:GetService("StudioDeviceSimulatorService")
	local shape = assert(SHAPES[preset], "unknown preset " .. tostring(preset))
	sim:SetDeviceAsync(shape.device)
	if shape.long then
		-- Long side first; the orientation call turns it.
		sim:SetResolutionAsync(shape.long, shape.short)
	end
	if preset ~= "desktop" then -- orientation errors on non-mobile devices
		sim:SetOrientationAsync(orientation == "portrait" and Enum.ScreenOrientation.Portrait
			or Enum.ScreenOrientation.LandscapeRight)
	end
	task.wait(0.5)
	return workspace.CurrentCamera.ViewportSize
end
return setDevice("phone", "portrait")
```

Measured viewports: phone 391x841 / 841x391, tablet 808x1078 / 1079x809, desktop
1918x1080 (always about one pixel short). `GetDeviceListAsync()` returns an array of
preset ids (44, e.g. `iphone_16`, `samsung_galaxy_a16`, `ipad_air_5th_generation`,
`hd_720`); print it to pick another shape.

### Device matrix

Check layouts in this order: phone portrait, phone landscape, tablet landscape,
desktop. For each, set the device, wait for the UI to settle, then check what matters
(`AbsolutePosition`/`AbsoluteSize` inside `GuiService:GetGuiInset()` and the safe area,
text not clipped, buttons at least 44 px). Prefer reading numbers over screenshots: a
number can be asserted, a screenshot has to be looked at.
"#
}
