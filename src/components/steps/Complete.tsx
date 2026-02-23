import { motion } from "framer-motion";
import {
  Check,
  ExternalLink,
  FolderOpen,
  Terminal,
  Copy,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import type { AiTool } from "@/lib/types";
import { TOOL_OPTIONS } from "@/lib/types";

/** Opens a URL, with WSL fallback for development. */
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

interface CompleteProps {
  projectPath: string;
  aiTool: AiTool;
  aiToolName: string;
}

const DISCORD_INVITE_URL = "https://discord.gg/roxlit";

export function Complete({ projectPath, aiTool, aiToolName }: CompleteProps) {
  const [editorOpened, setEditorOpened] = useState(false);
  const [editorError, setEditorError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const toolOption = TOOL_OPTIONS.find((t) => t.id === aiTool);
  const contextFile = toolOption?.contextFile ?? "AI-CONTEXT.md";

  const editorCommand =
    aiTool === "cursor"
      ? "cursor"
      : aiTool === "claude"
        ? "claude"
        : "code";

  async function handleOpenEditor() {
    try {
      await invoke("open_in_editor", { editor: aiTool, path: projectPath });
      setEditorOpened(true);
      setEditorError(null);
    } catch (err) {
      setEditorError(
        `Could not launch ${editorCommand}. Open the folder manually.`
      );
    }
  }

  async function handleCopyPath() {
    try {
      await navigator.clipboard.writeText(projectPath);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback: select text
    }
  }

  return (
    <motion.div
      className="flex flex-1 flex-col px-8 py-6"
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.4 }}
    >
      {/* Header */}
      <div className="flex items-center gap-3">
        <div className="relative">
          <div className="absolute inset-0 blur-[20px]">
            <div className="h-full w-full rounded-full bg-emerald-500/30" />
          </div>
          <div className="relative flex h-10 w-10 items-center justify-center rounded-full bg-emerald-500">
            <Check className="h-5 w-5 text-black" strokeWidth={3} />
          </div>
        </div>
        <div>
          <h2 className="text-lg font-bold">You're all set!</h2>
          <p className="text-sm text-zinc-400">
            Everything is installed and ready to go.
          </p>
        </div>
      </div>

      {/* What was installed */}
      <div className="mt-5 space-y-1.5">
        <p className="text-xs font-medium uppercase tracking-wider text-zinc-500">
          Installed
        </p>
        {[
          "Aftman (toolchain manager)",
          "Rojo (file sync with Studio)",
          `${contextFile} (AI context for ${aiToolName})`,
          "Roblox project structure",
        ].map((item) => (
          <div key={item} className="flex items-center gap-2 text-sm">
            <Check className="h-3.5 w-3.5 text-emerald-400" />
            <span className="text-zinc-300">{item}</span>
          </div>
        ))}
      </div>

      {/* Project path */}
      <div className="mt-5">
        <p className="text-xs font-medium uppercase tracking-wider text-zinc-500">
          Project folder
        </p>
        <div className="mt-1.5 flex items-center gap-2 rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
          <FolderOpen className="h-4 w-4 shrink-0 text-emerald-400" />
          <span className="flex-1 truncate font-mono text-xs text-emerald-400">
            {projectPath}
          </span>
          <button
            onClick={handleCopyPath}
            className="shrink-0 text-zinc-500 transition-colors hover:text-zinc-300"
            title="Copy path"
          >
            {copied ? (
              <Check className="h-3.5 w-3.5 text-emerald-400" />
            ) : (
              <Copy className="h-3.5 w-3.5" />
            )}
          </button>
        </div>
      </div>

      {/* Primary action: Open in editor */}
      <div className="mt-5">
        <button
          onClick={handleOpenEditor}
          disabled={editorOpened}
          className="flex w-full items-center justify-center gap-2 rounded-lg bg-emerald-500 px-4 py-2.5 text-sm font-semibold text-black transition-colors hover:bg-emerald-400 disabled:opacity-60"
        >
          {editorOpened ? (
            <>
              <Check className="h-4 w-4" />
              Opened in {aiToolName}
            </>
          ) : (
            <>
              <FolderOpen className="h-4 w-4" />
              Open in {aiToolName}
            </>
          )}
        </button>
        {editorError && (
          <p className="mt-1.5 text-xs text-red-400">{editorError}</p>
        )}
      </div>

      {/* Next steps */}
      <div className="mt-5 rounded-lg border border-white/5 bg-white/[0.02] px-4 py-3">
        <p className="text-xs font-medium uppercase tracking-wider text-zinc-500">
          Next steps
        </p>
        <div className="mt-2 space-y-2 text-sm text-zinc-400">
          <div className="flex items-start gap-2">
            <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-white/10 text-[10px] font-bold text-zinc-300">
              1
            </span>
            <span>
              Open <strong className="text-zinc-200">Roblox Studio</strong> and
              open any place
            </span>
          </div>
          <div className="flex items-start gap-2">
            <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-white/10 text-[10px] font-bold text-zinc-300">
              2
            </span>
            <div>
              In {aiToolName}, open the terminal and run:
              <div className="mt-1 flex items-center gap-2 rounded bg-black/40 px-2 py-1 font-mono text-xs text-emerald-400">
                <Terminal className="h-3 w-3 shrink-0 text-zinc-500" />
                rojo serve
              </div>
            </div>
          </div>
          <div className="flex items-start gap-2">
            <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-white/10 text-[10px] font-bold text-zinc-300">
              3
            </span>
            <span>
              In Studio, click{" "}
              <strong className="text-zinc-200">Connect</strong> in the Rojo
              plugin
            </span>
          </div>
          <div className="flex items-start gap-2">
            <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-emerald-500/20 text-[10px] font-bold text-emerald-400">
              4
            </span>
            <span>
              Start coding with AI — your changes sync to Studio in real time
            </span>
          </div>
        </div>
      </div>

      {/* Secondary actions */}
      <div className="mt-auto flex items-center justify-center gap-4 pt-4">
        <button
          onClick={() => openExternal(DISCORD_INVITE_URL)}
          className="flex items-center gap-1.5 text-xs text-zinc-500 transition-colors hover:text-[#5865F2]"
        >
          Join Discord
          <ExternalLink className="h-3 w-3" />
        </button>
        <span className="text-zinc-700">·</span>
        <button
          onClick={() => openExternal("https://roxlit.dev")}
          className="flex items-center gap-1.5 text-xs text-zinc-500 transition-colors hover:text-zinc-300"
        >
          roxlit.dev
          <ExternalLink className="h-3 w-3" />
        </button>
      </div>
    </motion.div>
  );
}
