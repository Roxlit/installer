import { motion } from "framer-motion";
import { FolderOpen, Check, Rocket, RefreshCw, FolderSearch } from "lucide-react";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { DiscoveredProject, ProjectEntry } from "@/lib/types";
import { TOOL_OPTIONS } from "@/lib/types";

interface RecoveryProps {
  discoveredProjects: DiscoveredProject[];
  onRecovered: (project: ProjectEntry) => void;
  onStartFresh: () => void;
  onRescan: (projects: DiscoveredProject[]) => void;
}

export function Recovery({
  discoveredProjects,
  onRecovered,
  onStartFresh,
  onRescan,
}: RecoveryProps) {
  const [selected, setSelected] = useState<Set<number>>(
    new Set(discoveredProjects.map((_, i) => i))
  );
  const [saving, setSaving] = useState(false);
  const [scanning, setScanning] = useState(false);

  function toggleProject(index: number) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
  }

  async function handleBrowse() {
    const dir = await open({ directory: true, multiple: false });
    if (!dir) return;

    setScanning(true);
    try {
      const found = await invoke<DiscoveredProject[]>("scan_for_projects", {
        parentDir: dir as string,
      });
      if (found.length > 0) {
        // Merge with existing, dedup by path
        const existingPaths = new Set(discoveredProjects.map((p) => p.path));
        const newProjects = found.filter((p) => !existingPaths.has(p.path));
        const merged = [...discoveredProjects, ...newProjects];
        onRescan(merged);
        setSelected(new Set(merged.map((_, i) => i)));
      }
    } catch {
      // Silent — directory might not exist or be readable
    }
    setScanning(false);
  }

  async function handleRecover() {
    setSaving(true);
    let firstProject: ProjectEntry | null = null;

    for (const index of Array.from(selected).sort()) {
      const dp = discoveredProjects[index];
      const project: ProjectEntry = {
        name: dp.name,
        path: dp.path,
        aiTool: dp.aiTool,
        createdAt: new Date().toISOString(),
      };
      try {
        await invoke("save_project", { project });
      } catch {
        // Continue even if one fails
      }
      if (!firstProject) firstProject = project;
    }

    if (firstProject) {
      onRecovered(firstProject);
    }
    setSaving(false);
  }

  return (
    <motion.div
      className="flex flex-1 flex-col px-8 py-6"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.3 }}
    >
      <div className="flex items-center gap-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-full bg-emerald-500/10">
          <RefreshCw className="h-5 w-5 text-emerald-400" />
        </div>
        <div>
          <h2 className="text-lg font-semibold">Welcome back!</h2>
          <p className="text-sm text-zinc-400">
            We found {discoveredProjects.length} existing project
            {discoveredProjects.length > 1 ? "s" : ""} on your machine.
          </p>
        </div>
      </div>

      <div className="mt-6 flex-1 space-y-2 overflow-y-auto">
        {discoveredProjects.map((project, i) => {
          const isSelected = selected.has(i);
          const toolName =
            TOOL_OPTIONS.find((t) => t.id === project.aiTool)?.name ??
            "Unknown";
          return (
            <button
              key={project.path}
              onClick={() => toggleProject(i)}
              className={`flex w-full items-center gap-3 rounded-lg border p-3 text-left transition-all ${
                isSelected
                  ? "border-emerald-500/30 bg-emerald-500/[0.05]"
                  : "border-white/5 bg-white/[0.02]"
              }`}
            >
              <div
                className={`flex h-5 w-5 shrink-0 items-center justify-center rounded border ${
                  isSelected
                    ? "border-emerald-500 bg-emerald-500"
                    : "border-white/20"
                }`}
              >
                {isSelected && <Check className="h-3 w-3 text-black" />}
              </div>
              <FolderOpen className="h-4 w-4 shrink-0 text-zinc-500" />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{project.name}</div>
                <div className="truncate font-mono text-xs text-zinc-500">
                  {project.path}
                </div>
              </div>
              <span className="shrink-0 text-xs text-zinc-500">{toolName}</span>
            </button>
          );
        })}
      </div>

      <div className="mt-4 flex items-center justify-between border-t border-white/5 pt-4">
        <div className="flex items-center gap-3">
          <button
            onClick={onStartFresh}
            className="text-sm text-zinc-500 transition-colors hover:text-zinc-300"
          >
            Start fresh instead
          </button>
          <button
            onClick={handleBrowse}
            disabled={scanning}
            className="flex items-center gap-1.5 text-sm text-zinc-500 transition-colors hover:text-zinc-300"
          >
            <FolderSearch className="h-3.5 w-3.5" />
            {scanning ? "Scanning..." : "Scan another folder"}
          </button>
        </div>
        <button
          onClick={handleRecover}
          disabled={selected.size === 0 || saving}
          className="flex items-center gap-2 rounded-lg bg-emerald-500 px-5 py-2 text-sm font-semibold text-black transition-colors hover:bg-emerald-400 disabled:opacity-50"
        >
          <Rocket className="h-4 w-4" />
          {saving ? "Restoring..." : "Restore & Launch"}
        </button>
      </div>
    </motion.div>
  );
}
