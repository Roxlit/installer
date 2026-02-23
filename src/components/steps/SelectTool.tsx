import { motion } from "framer-motion";
import { ArrowLeft, ArrowRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { TOOL_OPTIONS, type AiTool } from "@/lib/types";
import {
  ClaudeIcon,
  CursorIcon,
  WindsurfIcon,
  VSCodeIcon,
} from "../ToolIcons";
import type { ComponentType } from "react";

const TOOL_ICONS: Record<string, ComponentType<{ className?: string }>> = {
  claude: ClaudeIcon,
  cursor: CursorIcon,
  vscode: VSCodeIcon,
  windsurf: WindsurfIcon,
};

const TOOL_COLORS: Record<string, string> = {
  claude: "text-[#D97757]",
  cursor: "text-white",
  vscode: "text-[#007ACC]",
  windsurf: "text-[#00B4D8]",
  other: "text-zinc-400",
};

interface SelectToolProps {
  selected: AiTool | null;
  onSelect: (tool: AiTool) => void;
  onNext: () => void;
  onBack: () => void;
}

export function SelectTool({ selected, onSelect, onNext, onBack }: SelectToolProps) {
  return (
    <motion.div
      className="flex flex-1 flex-col px-8 py-6"
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -20 }}
      transition={{ duration: 0.3 }}
    >
      <h2 className="text-lg font-semibold">Which AI tool do you use?</h2>
      <p className="mt-1 text-sm text-zinc-400">
        We'll generate the right context file for your tool.
      </p>

      <div className="mt-6 grid grid-cols-2 gap-3">
        {TOOL_OPTIONS.map((tool) => {
          const Icon = TOOL_ICONS[tool.id];
          const isSelected = selected === tool.id;

          return (
            <button
              key={tool.id}
              onClick={() => onSelect(tool.id)}
              className={cn(
                "flex items-center gap-3 rounded-xl border p-4 text-left transition-all",
                isSelected
                  ? "border-emerald-500/40 bg-emerald-500/[0.06]"
                  : "border-white/5 bg-white/[0.02] hover:border-white/10"
              )}
            >
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-white/10 bg-white/5">
                {Icon ? (
                  <Icon className={cn("h-5 w-5", TOOL_COLORS[tool.id])} />
                ) : (
                  <span className="text-lg text-zinc-400">?</span>
                )}
              </div>
              <div>
                <div className="text-sm font-medium">{tool.name}</div>
                <div className="text-[11px] text-zinc-500">
                  {tool.contextFile}
                </div>
              </div>
            </button>
          );
        })}
      </div>

      {/* Navigation */}
      <div className="mt-auto flex items-center justify-between pt-6">
        <button
          onClick={onBack}
          className="flex items-center gap-1.5 text-sm text-zinc-400 transition-colors hover:text-zinc-200"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          Back
        </button>
        <button
          onClick={onNext}
          disabled={!selected}
          className={cn(
            "flex items-center gap-2 rounded-lg px-5 py-2 text-sm font-semibold transition-all",
            selected
              ? "bg-emerald-500 text-black hover:bg-emerald-400"
              : "cursor-not-allowed bg-white/5 text-zinc-600"
          )}
        >
          Next
          <ArrowRight className="h-3.5 w-3.5" />
        </button>
      </div>
    </motion.div>
  );
}
