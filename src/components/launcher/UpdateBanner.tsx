import { Download, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import type { UpdateInfo } from "@/lib/types";

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

interface UpdateBannerProps {
  update: UpdateInfo;
  onDismiss: () => void;
}

export function UpdateBanner({ update, onDismiss }: UpdateBannerProps) {
  return (
    <div className="flex items-center justify-between rounded-md border border-emerald-500/20 bg-emerald-500/[0.05] px-3 py-2">
      <span className="text-xs text-emerald-400">
        Update available: <span className="font-semibold">v{update.version}</span>
      </span>
      <div className="flex items-center gap-1">
        <button
          onClick={() => openExternal(update.htmlUrl)}
          className="flex items-center gap-1 rounded px-2 py-1 text-xs font-medium text-emerald-400 transition-colors hover:bg-emerald-500/10"
        >
          <Download className="h-3 w-3" />
          Download
        </button>
        <button
          onClick={onDismiss}
          className="rounded p-1 text-zinc-500 transition-colors hover:bg-white/5 hover:text-zinc-300"
          title="Dismiss this update"
        >
          <X className="h-3 w-3" />
        </button>
      </div>
    </div>
  );
}
