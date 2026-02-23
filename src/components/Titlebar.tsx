import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, X } from "lucide-react";
import { RoxlitIcon } from "./ToolIcons";

export function Titlebar() {
  const appWindow = getCurrentWindow();

  return (
    <div
      data-tauri-drag-region
      className="flex h-10 shrink-0 items-center border-b border-white/10 bg-black/80 px-4"
    >
      <RoxlitIcon className="h-4 w-4 text-emerald-400" />
      <span className="ml-2 text-xs text-zinc-400">Roxlit Installer</span>

      <div className="ml-auto flex gap-1">
        <button
          onClick={() => appWindow.minimize()}
          className="flex h-7 w-7 items-center justify-center rounded text-zinc-500 transition-colors hover:bg-white/10 hover:text-zinc-300"
          aria-label="Minimize"
        >
          <Minus className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={() => appWindow.close()}
          className="flex h-7 w-7 items-center justify-center rounded text-zinc-500 transition-colors hover:bg-red-500/20 hover:text-red-400"
          aria-label="Close"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}
