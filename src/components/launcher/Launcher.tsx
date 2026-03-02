import { useState, useRef, useEffect } from "react";
import { motion } from "framer-motion";
import {
  Play,
  Square,
  FolderOpen,
  Plus,
  ExternalLink,
  Code2,
  Loader2,
  ChevronDown,
  Check,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { LogTerminal } from "./LogTerminal";
import { UpdateBanner } from "./UpdateBanner";
import { SettingsPopover } from "./SettingsPopover";
import { TOOL_OPTIONS } from "@/lib/types";
import type { ProjectEntry, RojoStatus, UpdateInfo } from "@/lib/types";

async function openExternal(url: string) {
  try {
    await openUrl(url);
  } catch {
    try {
      await invoke("open_url_fallback", { url });
    } catch {
      window.open(url, "_blank");
    }
  }
}

interface LauncherProps {
  projectName: string;
  projectPath: string;
  aiTool: string;
  rojoStatus: RojoStatus;
  rojoPort: number | null;
  logs: string[];
  error: string | null;
  update: UpdateInfo | null;
  updateDelayDays: number;
  onStartDevelopment: () => void;
  onStopAll: () => void;
  onOpenEditor: () => void;
  onNewProject: () => void;
  onDismissUpdate: () => void;
  onUpdateDelayChange: (days: number) => void;
  allProjects: ProjectEntry[];
  onProjectSwitch: (project: ProjectEntry) => void;
}

function StatusDot({ status }: { status: RojoStatus }) {
  const colors: Record<string, string> = {
    stopped: "bg-zinc-500",
    starting: "bg-yellow-400 animate-pulse",
    running: "bg-emerald-400",
    error: "bg-red-400",
  };
  return <div className={`h-2 w-2 rounded-full ${colors[status] ?? "bg-zinc-500"}`} />;
}

function RojoStatusText({
  status,
  port,
}: {
  status: RojoStatus;
  port: number | null;
}) {
  switch (status) {
    case "stopped":
      return <span className="text-zinc-500">Rojo stopped</span>;
    case "starting":
      return <span className="text-yellow-400">Starting Rojo...</span>;
    case "running":
      return (
        <span className="text-emerald-400">
          Rojo running
          {port ? (
            <>
              {" on "}
              <button
                onClick={() => openExternal(`http://localhost:${port}`)}
                className="underline decoration-emerald-400/50 hover:decoration-emerald-400 transition-colors"
              >
                localhost:{port}
              </button>
            </>
          ) : ""}
        </span>
      );
    case "error":
      return <span className="text-red-400">Rojo error</span>;
  }
}

export function Launcher({
  projectName,
  projectPath,
  aiTool,
  rojoStatus,
  rojoPort,
  logs,
  error,
  update,
  updateDelayDays,
  onStartDevelopment,
  onStopAll,
  onOpenEditor,
  onNewProject,
  onDismissUpdate,
  onUpdateDelayChange,
  allProjects,
  onProjectSwitch,
}: LauncherProps) {
  const [editorLoading, setEditorLoading] = useState(false);
  const [projectDropdownOpen, setProjectDropdownOpen] = useState(false);
  const projectDropdownRef = useRef<HTMLDivElement>(null);
  const hasMultipleProjects = allProjects.length > 1;

  // Close project dropdown on outside click
  useEffect(() => {
    if (!projectDropdownOpen) return;
    function handleClick(e: MouseEvent) {
      if (
        projectDropdownRef.current &&
        !projectDropdownRef.current.contains(e.target as Node)
      ) {
        setProjectDropdownOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [projectDropdownOpen]);
  const toolName =
    TOOL_OPTIONS.find((t) => t.id === aiTool)?.name ?? "your AI tool";
  const isRunning = rojoStatus === "running" || rojoStatus === "starting";

  async function handleOpenEditor() {
    if (editorLoading) return;
    setEditorLoading(true);
    onOpenEditor();
    // Cooldown to prevent spam
    setTimeout(() => setEditorLoading(false), 3000);
  }

  async function handleStartDev() {
    if (isRunning) return;
    onStartDevelopment();
  }

  return (
    <motion.div
      className="flex min-h-0 flex-1 flex-col px-6 py-4"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
    >
      {/* Project info */}
      <div className="flex items-start justify-between">
        <div className="relative flex items-start gap-3" ref={projectDropdownRef}>
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-emerald-500/10 text-emerald-400">
            <FolderOpen className="h-5 w-5" />
          </div>
          <div>
            {hasMultipleProjects ? (
              <button
                onClick={() => setProjectDropdownOpen(!projectDropdownOpen)}
                className="flex items-center gap-1 text-base font-semibold transition-colors hover:text-emerald-400"
              >
                {projectName}
                <ChevronDown className={`h-4 w-4 text-zinc-500 transition-transform ${projectDropdownOpen ? "rotate-180" : ""}`} />
              </button>
            ) : (
              <h2 className="text-base font-semibold">{projectName}</h2>
            )}
            <p className="mt-0.5 font-mono text-xs text-zinc-500">
              {projectPath}
            </p>
            <p className="mt-0.5 text-xs text-zinc-500">{toolName}</p>
          </div>

          {/* Project dropdown */}
          {projectDropdownOpen && (
            <div className="absolute left-0 top-full z-10 mt-1 w-72 rounded-lg border border-white/10 bg-zinc-900 py-1 shadow-xl">
              {allProjects.map((project) => {
                const isActive = project.path === projectPath;
                const projToolName =
                  TOOL_OPTIONS.find((t) => t.id === project.aiTool)?.name ?? project.aiTool;
                return (
                  <button
                    key={project.path}
                    onClick={() => {
                      if (!isActive) {
                        onProjectSwitch(project);
                      }
                      setProjectDropdownOpen(false);
                    }}
                    className={`flex w-full items-center gap-3 px-3 py-2 text-left transition-colors ${
                      isActive
                        ? "bg-emerald-500/[0.05]"
                        : "hover:bg-white/[0.03]"
                    }`}
                  >
                    <div className="w-4 shrink-0">
                      {isActive && (
                        <Check className="h-3.5 w-3.5 text-emerald-400" />
                      )}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className={`text-sm font-medium ${isActive ? "text-emerald-400" : ""}`}>
                        {project.name}
                      </div>
                      <div className="truncate text-[11px] text-zinc-500">
                        {project.path} · {projToolName}
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </div>
        <button
          onClick={handleOpenEditor}
          disabled={editorLoading}
          className="flex items-center gap-1.5 rounded-md border border-white/10 bg-white/[0.03] px-3 py-1.5 text-xs text-zinc-400 transition-colors hover:bg-white/[0.06] hover:text-zinc-200 disabled:opacity-50"
          title={`Open in ${toolName}`}
        >
          {editorLoading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Code2 className="h-3.5 w-3.5" />
          )}
          {editorLoading ? "Opening..." : "Open Editor"}
        </button>
      </div>

      {/* Update banner */}
      {update && (
        <div className="mt-3">
          <UpdateBanner update={update} onDismiss={onDismissUpdate} />
        </div>
      )}

      {/* Main action + status */}
      <div className="mt-5 flex items-center gap-3">
        {!isRunning ? (
          <button
            onClick={handleStartDev}
            className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-emerald-500 py-3 text-sm font-semibold text-black transition-colors hover:bg-emerald-400"
          >
            <Play className="h-4 w-4" />
            Start Development
          </button>
        ) : (
          <button
            onClick={onStopAll}
            disabled={rojoStatus === "starting"}
            className="flex flex-1 items-center justify-center gap-2 rounded-lg border border-red-500/30 bg-red-500/10 py-3 text-sm font-semibold text-red-400 transition-colors hover:bg-red-500/20 disabled:opacity-60"
          >
            {rojoStatus === "starting" ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Square className="h-4 w-4" />
            )}
            {rojoStatus === "starting" ? "Starting..." : "Stop"}
          </button>
        )}
      </div>

      {/* Status bar */}
      <div className="mt-3 flex items-center gap-4 text-xs">
        <div className="flex items-center gap-2">
          <StatusDot status={rojoStatus} />
          <RojoStatusText status={rojoStatus} port={rojoPort} />
        </div>
      </div>

      {/* Error display */}
      {error && (
        <div className="mt-2 rounded-md border border-red-500/20 bg-red-500/[0.05] px-3 py-2 text-xs text-red-400">
          {error}
        </div>
      )}

      {/* Terminal */}
      <div className="mt-4 flex min-h-0 flex-1 flex-col">
        <LogTerminal logs={logs} />
      </div>

      {/* Bottom bar */}
      <div className="mt-3 flex shrink-0 items-center justify-between">
        <div className="flex items-center gap-3">
          <button
            onClick={onNewProject}
            className="flex items-center gap-1.5 text-xs text-zinc-500 transition-colors hover:text-zinc-300"
          >
            <Plus className="h-3.5 w-3.5" />
            New Project
          </button>
          <SettingsPopover
            updateDelayDays={updateDelayDays}
            onUpdateDelayChange={onUpdateDelayChange}
          />
        </div>
        <button
          onClick={() => openExternal("https://github.com/Roxlit/installer/discussions")}
          className="flex items-center gap-1.5 text-xs text-zinc-500 transition-colors hover:text-zinc-300"
        >
          Feedback
          <ExternalLink className="h-3 w-3" />
        </button>
      </div>
    </motion.div>
  );
}
