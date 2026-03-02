export type WizardStep =
  | "welcome"
  | "selectTool"
  | "selectProject"
  | "detecting"
  | "installing"
  | "complete";

export type AiTool = "claude" | "cursor" | "vscode" | "windsurf" | "other";

export interface ToolOption {
  id: AiTool;
  name: string;
  description: string;
  contextFile: string;
}

export const TOOL_OPTIONS: ToolOption[] = [
  {
    id: "claude",
    name: "Claude Code",
    description: "Generates CLAUDE.md",
    contextFile: "CLAUDE.md",
  },
  {
    id: "cursor",
    name: "Cursor",
    description: "Generates .cursorrules",
    contextFile: ".cursorrules",
  },
  {
    id: "vscode",
    name: "VS Code + Copilot",
    description: "Generates copilot-instructions.md",
    contextFile: ".github/copilot-instructions.md",
  },
  {
    id: "windsurf",
    name: "Windsurf",
    description: "Generates .windsurfrules",
    contextFile: ".windsurfrules",
  },
  {
    id: "other",
    name: "Other",
    description: "Generates AI-CONTEXT.md",
    contextFile: "AI-CONTEXT.md",
  },
];

export const WIZARD_STEPS: { key: WizardStep; label: string }[] = [
  { key: "welcome", label: "Welcome" },
  { key: "selectTool", label: "AI Tool" },
  { key: "selectProject", label: "Project" },
  { key: "detecting", label: "Detect" },
  { key: "installing", label: "Install" },
  { key: "complete", label: "Done" },
];

export interface DetectionResult {
  os: string;
  studioInstalled: boolean;
  studioPluginsPath: string | null;
  rojoInstalled: boolean;
  rojoVersion: string | null;
  aftmanInstalled: boolean;
  aftmanVersion: string | null;
  rbxsyncInstalled: boolean; // Still needed for install step (rbxsync-mcp)
  rbxsyncVersion: string | null;
}

export type SetupEvent =
  | {
      event: "stepStarted";
      data: {
        step: string;
        description: string;
        stepIndex: number;
        totalSteps: number;
      };
    }
  | {
      event: "stepProgress";
      data: { step: string; progress: number; detail: string };
    }
  | {
      event: "stepCompleted";
      data: { step: string; detail: string };
    }
  | {
      event: "stepWarning";
      data: { step: string; message: string };
    }
  | {
      event: "error";
      data: { step: string; message: string };
    }
  | { event: "finished" };

export interface InstallConfig {
  aiTool: string;
  projectPath: string;
  projectName: string;
  skipAftman: boolean;
  skipRojo: boolean;
  skipRbxsync: boolean;
  pluginsPath: string | null;
}

// --- App Mode ---

export type AppMode = "loading" | "installer" | "launcher" | "recovery";

// --- Config types (matches Rust RoxlitConfig) ---

export interface ProjectEntry {
  name: string;
  path: string;
  aiTool: string;
  createdAt: string;
  placeId?: number | null;
  universeId?: number | null;
}

export interface RoxlitConfig {
  version: number;
  projects: ProjectEntry[];
  lastActiveProject: string | null;
  lastUpdateCheck?: string | null;
  dismissedVersion?: string | null;
  updateDelayDays?: number | null;
}

export interface DiscoveredProject {
  name: string;
  path: string;
  aiTool: string;
}

export interface UpdateInfo {
  version: string;
  publishedAt: string;
  htmlUrl: string;
  body: string;
}

// --- Rojo events (matches Rust RojoEvent) ---

export type RojoEvent =
  | { event: "output"; data: { line: string; stream: string } }
  | { event: "started"; data: { port: number } }
  | { event: "stopped"; data: { code: number | null } }
  | { event: "error"; data: { message: string } };

export type RojoStatus = "stopped" | "starting" | "running" | "error";

