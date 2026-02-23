import { motion } from "framer-motion";
import { Check, ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";

/** Opens a URL, with WSL fallback for development. */
async function openExternal(url: string) {
  try {
    await openUrl(url);
  } catch {
    // WSL fallback: try wslview, then window.open
    try {
      await invoke("open_url_fallback", { url });
    } catch {
      window.open(url, "_blank");
    }
  }
}

interface CompleteProps {
  projectPath: string;
  aiToolName: string;
}

// Placeholder — update when the Discord server is created
const DISCORD_INVITE_URL = "https://discord.gg/roxlit";

export function Complete({ projectPath, aiToolName }: CompleteProps) {
  return (
    <motion.div
      className="flex flex-1 flex-col items-center justify-center px-8"
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.4 }}
    >
      {/* Success icon with glow */}
      <div className="relative">
        <div className="absolute inset-0 blur-[30px]">
          <div className="h-full w-full rounded-full bg-emerald-500/30" />
        </div>
        <div className="relative flex h-16 w-16 items-center justify-center rounded-full bg-emerald-500">
          <Check className="h-8 w-8 text-black" strokeWidth={3} />
        </div>
      </div>

      <h2 className="mt-6 text-xl font-bold">All set!</h2>
      <p className="mt-2 max-w-sm text-center text-sm text-zinc-400">
        Your project is ready. Open it in {aiToolName} and start building your
        Roblox game with AI.
      </p>

      {/* Project path */}
      <div className="mt-4 rounded-lg border border-white/5 bg-white/[0.02] px-4 py-2 font-mono text-xs text-emerald-400">
        {projectPath}
      </div>

      {/* Action buttons */}
      <div className="mt-8 flex flex-col gap-3">
        <button
          onClick={() => {
            openExternal(DISCORD_INVITE_URL);
          }}
          className="flex items-center gap-2 rounded-lg bg-[#5865F2] px-6 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-[#4752C4]"
        >
          Join our Discord
          <ExternalLink className="h-3.5 w-3.5" />
        </button>

        <button
          onClick={() => {
            openExternal("https://roxlit.dev");
          }}
          className="flex items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.03] px-6 py-2.5 text-sm text-zinc-300 transition-colors hover:bg-white/[0.06]"
        >
          Visit roxlit.dev
          <ExternalLink className="h-3.5 w-3.5" />
        </button>
      </div>

      <p className="mt-6 text-[11px] text-zinc-600">
        Next steps: Open Roblox Studio, then open this project in {aiToolName}.
      </p>
    </motion.div>
  );
}
