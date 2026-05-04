import { useState } from "react";
import { X, Zap } from "lucide-react";
import mcpTutorial from "@/assets/mcp_tutorial.webp";

const DISMISSED_KEY = "mcp_banner_dismissed";

export function useMcpBanner() {
  const [dismissed, setDismissed] = useState(
    () => localStorage.getItem(DISMISSED_KEY) === "true"
  );

  function dismiss() {
    localStorage.setItem(DISMISSED_KEY, "true");
    setDismissed(true);
  }

  return { show: !dismissed, dismiss };
}

interface McpBannerProps {
  onDismiss: () => void;
}

export function McpBanner({ onDismiss }: McpBannerProps) {
  const [modalOpen, setModalOpen] = useState(false);

  return (
    <>
      <div className="flex items-center justify-between rounded-md border border-blue-500/20 bg-blue-500/[0.05] px-3 py-2">
        <div className="flex items-center gap-2">
          <Zap className="h-3.5 w-3.5 shrink-0 text-blue-400" />
          <span className="text-xs text-blue-400">
            Activate the Studio MCP to let your AI tool control Roblox Studio directly.
          </span>
        </div>
        <div className="ml-3 flex shrink-0 items-center gap-1">
          <button
            onClick={() => setModalOpen(true)}
            className="rounded px-2 py-1 text-xs font-medium text-blue-400 transition-colors hover:bg-blue-500/10"
          >
            How to activate
          </button>
          <button
            onClick={onDismiss}
            className="rounded p-1 text-zinc-500 transition-colors hover:bg-white/5 hover:text-zinc-300"
            title="Dismiss"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      </div>

      {modalOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
          onClick={() => setModalOpen(false)}
        >
          <div
            className="relative mx-4 w-full max-w-xl rounded-xl border border-white/10 bg-zinc-900 p-5 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-3 flex items-center justify-between">
              <h3 className="text-sm font-semibold">How to activate Studio MCP</h3>
              <button
                onClick={() => setModalOpen(false)}
                className="rounded p-1 text-zinc-500 transition-colors hover:bg-white/5 hover:text-zinc-300"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <p className="mb-3 text-xs text-zinc-400">
              In Roblox Studio, open the Assistant panel, click the menu icon, go to{" "}
              <span className="font-medium text-zinc-300">Manage MCP Servers</span>, and toggle{" "}
              <span className="font-medium text-zinc-300">Enable Studio as MCP server</span>.
            </p>
            <img
              src={mcpTutorial}
              alt="How to activate Studio MCP"
              className="w-full rounded-lg border border-white/5"
            />
            <button
              onClick={() => {
                setModalOpen(false);
                onDismiss();
              }}
              className="mt-4 w-full rounded-lg bg-emerald-500 py-2 text-sm font-semibold text-black transition-colors hover:bg-emerald-400"
            >
              Got it
            </button>
          </div>
        </div>
      )}
    </>
  );
}
