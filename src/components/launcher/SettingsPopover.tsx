import { useState, useRef, useEffect } from "react";
import { Settings, Zap, Check, Loader2, AlertTriangle } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

const DELAY_OPTIONS = [
  { value: 0, label: "Immediate" },
  { value: 1, label: "1 day" },
  { value: 3, label: "3 days" },
  { value: 7, label: "7 days" },
  { value: 14, label: "14 days" },
];

type McpSetupState = "idle" | "loading" | "done" | "not_available" | "error";

interface SettingsPopoverProps {
  updateDelayDays: number;
  onUpdateDelayChange: (days: number) => void;
  aiTool: string;
  projectPath: string;
}

export function SettingsPopover({
  updateDelayDays,
  onUpdateDelayChange,
  aiTool,
  projectPath,
}: SettingsPopoverProps) {
  const [open, setOpen] = useState(false);
  const [mcpState, setMcpState] = useState<McpSetupState>("idle");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  async function handleSetupMcp() {
    setMcpState("loading");
    try {
      const available = await invoke<boolean>("setup_studio_mcp", {
        aiTool,
        projectPath,
      });
      setMcpState(available ? "done" : "not_available");
    } catch {
      setMcpState("error");
    }
  }

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 text-xs text-zinc-500 transition-colors hover:text-zinc-300"
        title="Settings"
      >
        <Settings className="h-3.5 w-3.5" />
      </button>

      {open && (
        <div className="absolute bottom-full left-0 mb-2 w-56 rounded-lg border border-white/10 bg-zinc-900 p-3 shadow-xl">
          <label className="block text-xs font-medium text-zinc-400">
            Update delay
          </label>
          <select
            value={updateDelayDays}
            onChange={(e) => {
              onUpdateDelayChange(Number(e.target.value));
              setOpen(false);
            }}
            className="mt-1.5 w-full rounded-md border border-white/10 bg-white/[0.03] px-2 py-1.5 text-xs text-zinc-300 outline-none focus:border-emerald-500/50"
          >
            {DELAY_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>

          <div className="mt-3 border-t border-white/5 pt-3">
            <button
              onClick={handleSetupMcp}
              disabled={mcpState === "loading" || mcpState === "done"}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-xs text-zinc-400 transition-colors hover:bg-white/5 hover:text-zinc-200 disabled:opacity-50"
            >
              {mcpState === "loading" && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              {mcpState === "done" && <Check className="h-3.5 w-3.5 text-emerald-400" />}
              {mcpState === "not_available" && <AlertTriangle className="h-3.5 w-3.5 text-yellow-400" />}
              {mcpState === "error" && <AlertTriangle className="h-3.5 w-3.5 text-red-400" />}
              {(mcpState === "idle") && <Zap className="h-3.5 w-3.5" />}
              {mcpState === "idle" && "Configure Studio MCP"}
              {mcpState === "loading" && "Configuring..."}
              {mcpState === "done" && "Studio MCP configured"}
              {mcpState === "not_available" && "Update Roblox Studio"}
              {mcpState === "error" && "Failed — try manually"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
