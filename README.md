# Roxlit

Open-source launcher that connects AI coding tools to Roblox Studio. One click.

Works with Claude Code, Cursor, Windsurf, and any AI tool that reads project context files.

<!-- TODO: Add screenshot of the launcher here -->
<!-- ![Roxlit Launcher](https://raw.githubusercontent.com/Roxlit/installer/main/.github/screenshots/launcher.png) -->

## What it does

Roxlit automates the entire setup process for AI-powered Roblox development:

- **Rojo + Aftman** installed and configured (no terminal needed)
- **MCP server** (via RbxSync) so AI tools can execute Luau in Studio, insert models, and extract games to files
- **7 AI context packs** that teach your AI how to write correct Luau code
- **Debug logging** for every installation step

After the initial setup, it becomes your daily launcher. One click starts `rojo serve`, opens your editor, and shows live sync logs.

## Download

Get the latest release from [roxlit.dev](https://roxlit.dev) or the [Releases page](https://github.com/Roxlit/installer/releases).

Windows x64 only for now. No code signing certificate yet, so Windows SmartScreen will show a warning. [Source code is right here](https://github.com/Roxlit/installer) if you want to verify or build it yourself.

## Why AI context packs matter

Without context, AI tools write bad Roblox code. They use `wait()` instead of `task.wait()`, put game logic in LocalScripts, skip server validation, and hallucinate APIs that don't exist.

Roxlit's context packs fix this. They're curated documentation files that get installed into your project (`.roxlit/context/`), covering:

- Roblox services and their correct usage
- Luau data types and strict typing patterns
- Client-server architecture and security model
- Common patterns (DataStores, RemoteEvents, UI)
- Physics, constraints, and spatial queries
- Networking and replication

The AI reads these files automatically and writes code that follows best practices.

## How it works

**First run (installer wizard):**

1. Pick your AI tool (Claude Code, Cursor, Windsurf, etc.)
2. Select your project folder
3. Roxlit detects what's already installed and what's missing
4. One click installs everything

**Every run after that (launcher):**

1. Open Roxlit
2. Click "Start" to launch `rojo serve` + your editor
3. Code with AI, changes sync to Studio in real time

## Tech stack

| Component | Technology |
|-----------|-----------|
| Desktop app | [Tauri v2](https://tauri.app) |
| Frontend | React 19, TypeScript, Tailwind CSS v4 |
| Backend | Rust |
| File sync | [Rojo](https://rojo.space) |
| MCP server | [RbxSync](https://github.com/Roxlit/rbxsync) (fork with broken tools disabled) |
| Builds | GitHub Actions (NSIS installer) |

## Building from source

```bash
# Prerequisites: Node.js 18+, Rust 1.70+, Tauri CLI

git clone https://github.com/Roxlit/installer.git
cd installer
npm ci
cd src-tauri && cargo check && cd ..
npm run tauri dev
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The highest-impact contribution is improving the AI context packs. If you notice the AI getting something wrong (deprecated APIs, bad patterns, missing best practices), submit a PR.

## Project structure

```
src/                          # React + TypeScript frontend
├── components/steps/         # Installer wizard UI
├── components/launcher/      # Launcher UI
└── hooks/                    # useInstaller, useLauncher
src-tauri/                    # Rust backend
├── src/commands/             # Tauri IPC commands
└── src/templates/            # AI context packs + templates
```

## License

[MIT](LICENSE)

## Links

- **Website**: [roxlit.dev](https://roxlit.dev)
- **Feedback**: [GitHub Discussions](https://github.com/Roxlit/installer/discussions)
- **Blog**: [roxlit.dev/blog](https://roxlit.dev/blog)
