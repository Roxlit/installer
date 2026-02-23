import { motion } from "framer-motion";
import { ArrowLeft, ArrowRight, FolderOpen } from "lucide-react";
import { cn } from "@/lib/utils";

interface SelectProjectProps {
  projectName: string;
  parentDir: string;
  fullPath: string;
  onNameChange: (name: string) => void;
  onPickDirectory: () => void;
  onNext: () => void;
  onBack: () => void;
}

export function SelectProject({
  projectName,
  parentDir,
  fullPath,
  onNameChange,
  onPickDirectory,
  onNext,
  onBack,
}: SelectProjectProps) {
  // Only allow alphanumeric, hyphens, and underscores
  const isValidName = /^[a-zA-Z0-9_-]+$/.test(projectName) && projectName.length > 0;

  return (
    <motion.div
      className="flex flex-1 flex-col px-8 py-6"
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -20 }}
      transition={{ duration: 0.3 }}
    >
      <h2 className="text-lg font-semibold">Where should we create your project?</h2>
      <p className="mt-1 text-sm text-zinc-400">
        Choose a name and location for your Roblox project.
      </p>

      <div className="mt-6 space-y-4">
        {/* Project name */}
        <div>
          <label className="mb-1.5 block text-xs font-medium text-zinc-400">
            Project name
          </label>
          <input
            type="text"
            value={projectName}
            onChange={(e) => onNameChange(e.target.value)}
            placeholder="my-roblox-game"
            className={cn(
              "w-full rounded-lg border bg-white/[0.03] px-4 py-2.5 text-sm outline-none transition-colors",
              isValidName
                ? "border-white/10 focus:border-emerald-500/50"
                : "border-red-500/30 focus:border-red-500/50"
            )}
          />
          {!isValidName && projectName.length > 0 && (
            <p className="mt-1 text-[11px] text-red-400">
              Only letters, numbers, hyphens, and underscores allowed
            </p>
          )}
        </div>

        {/* Parent directory */}
        <div>
          <label className="mb-1.5 block text-xs font-medium text-zinc-400">
            Location
          </label>
          <div className="flex gap-2">
            <div className="flex-1 rounded-lg border border-white/10 bg-white/[0.03] px-4 py-2.5 text-sm text-zinc-400">
              {parentDir || "Select a directory..."}
            </div>
            <button
              onClick={onPickDirectory}
              className="flex items-center gap-2 rounded-lg border border-white/10 bg-white/[0.03] px-4 py-2.5 text-sm text-zinc-300 transition-colors hover:border-white/20 hover:bg-white/[0.06]"
            >
              <FolderOpen className="h-4 w-4" />
              Browse
            </button>
          </div>
        </div>

        {/* Full path preview */}
        <div className="rounded-lg border border-white/5 bg-white/[0.02] px-4 py-3">
          <div className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
            Project will be created at
          </div>
          <div className="mt-1 font-mono text-sm text-emerald-400">
            {fullPath}/
          </div>
        </div>
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
          disabled={!isValidName || !parentDir}
          className={cn(
            "flex items-center gap-2 rounded-lg px-5 py-2 text-sm font-semibold transition-all",
            isValidName && parentDir
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
