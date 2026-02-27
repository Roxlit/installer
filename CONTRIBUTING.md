# Contributing to Roxlit

Thanks for your interest in contributing! Roxlit is open-source and community contributions are welcome.

## How to Contribute

1. Fork the repo
2. Create a branch from `dev` (never from `main`)
3. Make your changes
4. Open a PR to `dev`

## AI Context Packs

The highest-impact area for contributions is improving the AI context packs in `src-tauri/src/templates/context_packs.rs`. These are curated Roblox documentation files that get installed into every Roxlit project (`.roxlit/context/`). They teach AI assistants how to write correct Roblox code.

### What Makes a Good Context Pack Entry

**Document things the AI consistently gets wrong.** The AI has general knowledge of Roblox, but it makes the same mistakes over and over. Context packs fix that.

Good entries:

- **Correct orientations**: "Cylinders have their axis along X by default. For wheels, use `(0, 0, 0)`. For headlights facing forward, use `(0, -90, 0)`." — The AI gets this wrong every time without guidance.
- **Common property pitfalls**: "Cabin walls around a VehicleSeat MUST have `CanCollide = false`, otherwise players can't reach the seat." — The AI always forgets this.
- **API gotchas**: "Never set `Position` on welded parts — it breaks the assembly. Use `CFrame` instead." — The AI doesn't know this distinction.
- **"Don't guess" rules**: "NEVER invent asset IDs. Search the web for real ones." — The AI hallucinates IDs constantly.
- **Enum/property tables**: Valid values, ranges, defaults. The AI guesses wrong values without a reference.

### What Does NOT Belong in Context Packs

**Don't document how to build complex systems from scratch.** If the community already has a battle-tested open-source solution, point the AI to it instead.

Bad entries:

- A full vehicle physics system with HingeConstraints, torque balance, suspension — **A-Chassis exists.** Tell the AI to search for it.
- A complete inventory system with UI and DataStore — **community frameworks exist.** Link to them.
- A combat system with hitboxes, cooldowns, animations — **already solved.** Don't reinvent it.

The rule: **if it took the community years to get right, the AI shouldn't try to build it in one conversation.**

### The Litmus Test

Before adding something to a context pack, ask:

1. **Does the AI get this wrong without guidance?** If yes, document it. If the AI already handles it correctly, skip it.
2. **Is this a recurring mistake or a one-off?** Context packs fix patterns, not individual bugs.
3. **Is there an existing community solution?** If yes, tell the AI to search for it instead of documenting how to build it.
4. **Would this help across many projects?** Context packs are installed in every project. Don't add niche game-specific knowledge.

### Format Guidelines

- Use clear headings and short paragraphs
- Include code examples showing BAD vs GOOD patterns
- Use tables for properties, enums, valid ranges
- Bold the "NEVER" and "ALWAYS" rules — they prevent the most common mistakes
- Keep it concise: the AI has limited context window. Every line should earn its place

### How to Test Your Changes

1. Run `cargo check` in `src-tauri/` to verify compilation
2. Read through your addition and ask: "Would this prevent a real mistake I've seen the AI make?"
3. If you're documenting a pitfall, try asking an AI to do the task WITHOUT your context — does it fail? That confirms the entry is needed

## Code Contributions

### Setup

```bash
cd installer
npm ci                    # Frontend dependencies
cd src-tauri && cargo check  # Rust backend
```

### Architecture

- `src/` — React + TypeScript frontend (installer wizard + launcher UI)
- `src-tauri/src/commands/` — Rust backend commands (Tauri IPC)
- `src-tauri/src/templates/` — AI context templates + context packs

### Branch Convention

- `dev` — development branch, all PRs target here
- `main` — stable, deployable. Only receives merges from `dev`
- `feat/<name>` — large features, branch from `dev`

### Before Submitting

- `cargo check` passes
- `npx tsc --noEmit` passes (frontend type check)
- Commit messages are descriptive (what changed and why)
