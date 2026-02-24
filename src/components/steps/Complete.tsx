import { motion } from "framer-motion";
import { Check, Rocket } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { AiTool, ProjectEntry } from "@/lib/types";
import { TOOL_OPTIONS } from "@/lib/types";

interface CompleteProps {
  projectPath: string;
  projectName: string;
  aiTool: AiTool;
  aiToolName: string;
  onGoToLauncher: (project: ProjectEntry) => void;
}

export function Complete({
  projectPath,
  projectName,
  aiTool,
  aiToolName,
  onGoToLauncher,
}: CompleteProps) {
  const [saved, setSaved] = useState(false);

  const toolOption = TOOL_OPTIONS.find((t) => t.id === aiTool);
  const contextFile = toolOption?.contextFile ?? "AI-CONTEXT.md";

  // Save project config on mount
  useEffect(() => {
    const project: ProjectEntry = {
      name: projectName,
      path: projectPath,
      aiTool,
      createdAt: new Date().toISOString(),
    };
    invoke("save_project", { project })
      .then(() => setSaved(true))
      .catch(() => setSaved(true)); // Continue even if save fails
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  function handleLaunch() {
    onGoToLauncher({
      name: projectName,
      path: projectPath,
      aiTool,
      createdAt: new Date().toISOString(),
    });
  }

  return (
    <motion.div
      className="flex flex-1 flex-col items-center justify-center px-8"
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.4 }}
    >
      {/* Success icon */}
      <div className="relative">
        <div className="absolute inset-0 blur-[30px]">
          <div className="h-full w-full rounded-full bg-emerald-500/30" />
        </div>
        <div className="relative flex h-16 w-16 items-center justify-center rounded-full bg-emerald-500">
          <Check className="h-8 w-8 text-black" strokeWidth={3} />
        </div>
      </div>

      <h2 className="mt-6 text-xl font-bold">You're all set!</h2>
      <p className="mt-2 max-w-sm text-center text-sm text-zinc-400">
        Everything is installed and ready. Launch the development environment to
        start building with {aiToolName}.
      </p>

      {/* What was installed */}
      <div className="mt-6 space-y-1.5">
        {[
          "Aftman (toolchain manager)",
          "Rojo (script sync with Studio)",
          "RbxSync (instance sync with Studio)",
          `${contextFile} (AI context)`,
          "Roblox project structure",
        ].map((item) => (
          <div
            key={item}
            className="flex items-center gap-2 text-sm text-zinc-400"
          >
            <Check className="h-3.5 w-3.5 text-emerald-400" />
            {item}
          </div>
        ))}
      </div>

      {/* Primary action */}
      <button
        onClick={handleLaunch}
        disabled={!saved}
        className="mt-8 flex items-center gap-2 rounded-lg bg-emerald-500 px-8 py-3 text-sm font-semibold text-black transition-colors hover:bg-emerald-400 disabled:opacity-50"
      >
        <Rocket className="h-4 w-4" />
        Start Development
      </button>

      <p className="mt-3 text-[11px] text-zinc-600">
        This will open {aiToolName} and start the Rojo sync server.
      </p>
    </motion.div>
  );
}
