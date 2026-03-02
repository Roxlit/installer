import { useState, useEffect } from "react";
import { AnimatePresence } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { Titlebar } from "./components/Titlebar";
import { StepIndicator } from "./components/StepIndicator";
import { Welcome } from "./components/steps/Welcome";
import { SelectTool } from "./components/steps/SelectTool";
import { SelectProject } from "./components/steps/SelectProject";
import { Detecting } from "./components/steps/Detecting";
import { Installing } from "./components/steps/Installing";
import { Complete } from "./components/steps/Complete";
import { Launcher } from "./components/launcher/Launcher";
import { useInstaller } from "./hooks/useInstaller";
import { useLauncher } from "./hooks/useLauncher";
import { useUpdateChecker } from "./hooks/useUpdateChecker";
import { Recovery } from "./components/steps/Recovery";
import { TOOL_OPTIONS } from "./lib/types";
import type { AppMode, DiscoveredProject, ProjectEntry, RoxlitConfig } from "./lib/types";

export default function App() {
  const [mode, setMode] = useState<AppMode>("loading");
  const [config, setConfig] = useState<RoxlitConfig | null>(null);
  const [updateDelayDays, setUpdateDelayDays] = useState(7);
  const [discoveredProjects, setDiscoveredProjects] = useState<DiscoveredProject[]>([]);
  const installer = useInstaller();
  const launcher = useLauncher();
  const { update, dismissUpdate } = useUpdateChecker(config);

  // Boot: 3-layer fallback — config → disk scan → wizard
  useEffect(() => {
    async function boot() {
      // Layer 1: Try loading existing config
      try {
        const loadedConfig = await invoke<RoxlitConfig | null>("load_config");
        if (loadedConfig && loadedConfig.projects.length > 0) {
          // Validate that at least one project path still exists on disk
          const validProjects: ProjectEntry[] = [];
          for (const project of loadedConfig.projects) {
            const exists = await invoke<boolean>("check_project_exists", {
              path: project.path,
            });
            if (exists) {
              validProjects.push(project);
            }
          }

          if (validProjects.length > 0) {
            const updatedConfig = { ...loadedConfig, projects: validProjects };
            setConfig(updatedConfig);
            setUpdateDelayDays(updatedConfig.updateDelayDays ?? 7);
            const active =
              validProjects.find(
                (p) => p.path === updatedConfig.lastActiveProject
              ) ?? validProjects[0];
            launcher.setProject(active);
            setMode("launcher");
            return;
          }
          // All project paths are gone — fall through to disk scan
        }
      } catch {
        // Config load failed — fall through to disk scan
      }

      // Layer 2: No valid config. Scan default parent dir for existing projects.
      try {
        const discovered = await invoke<DiscoveredProject[]>(
          "scan_for_projects",
          { parentDir: "~/RobloxProjects" }
        );
        if (discovered.length > 0) {
          setDiscoveredProjects(discovered);
          setMode("recovery");
          return;
        }
      } catch {
        // Scan failed — fall through to installer
      }

      // Layer 3: Nothing found. First-time user — show full wizard.
      setMode("installer");
    }

    boot();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleGoToLauncher = async (project: ProjectEntry) => {
    launcher.setProject(project);
    setMode("launcher");
    try {
      await invoke("set_active_project", { path: project.path });
    } catch {
      // Non-critical
    }
    // Auto-start development
    setTimeout(() => {
      launcher.startDevelopment();
    }, 300);
  };

  const handleProjectSwitch = async (project: ProjectEntry) => {
    await launcher.stopAll();
    launcher.setProject(project);
    try {
      await invoke("set_active_project", { path: project.path });
    } catch {
      // Non-critical — project switch still works without persisting
    }
  };

  const handleNewProject = async () => {
    // Stop running servers before switching to installer
    await launcher.stopAll();
    installer.reset();
    setMode("installer");
  };

  const handleUpdateDelayChange = async (days: number) => {
    setUpdateDelayDays(days);
    setConfig((prev) =>
      prev ? { ...prev, updateDelayDays: days } : prev
    );
    try {
      await invoke("save_settings", { updateDelayDays: days });
    } catch {
      // Silent failure — settings save is non-critical
    }
  };

  const aiToolName =
    TOOL_OPTIONS.find((t) => t.id === installer.aiTool)?.name ??
    "your AI tool";

  // Loading state
  if (mode === "loading") {
    return (
      <div className="flex h-screen flex-col overflow-hidden rounded-lg border border-white/10 bg-[#09090b]">
        <Titlebar />
        <div className="flex flex-1 items-center justify-center">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-emerald-500 border-t-transparent" />
        </div>
      </div>
    );
  }

  // Launcher mode
  if (mode === "launcher" && launcher.project) {
    return (
      <div className="flex h-screen flex-col overflow-hidden rounded-lg border border-white/10 bg-[#09090b]">
        <Titlebar title="Roxlit" />
        <Launcher
          projectName={launcher.project.name}
          projectPath={launcher.project.path}
          aiTool={launcher.project.aiTool}
          rojoStatus={launcher.rojoStatus}
          rojoPort={launcher.rojoPort}
          logs={launcher.logs}
          error={launcher.error}
          update={update}
          updateDelayDays={updateDelayDays}
          onStartDevelopment={launcher.startDevelopment}
          onStopAll={launcher.stopAll}
          onOpenEditor={launcher.openEditor}
          onNewProject={handleNewProject}
          onDismissUpdate={dismissUpdate}
          onUpdateDelayChange={handleUpdateDelayChange}
          allProjects={config?.projects ?? []}
          onProjectSwitch={handleProjectSwitch}
        />
      </div>
    );
  }

  // Recovery mode — config lost but projects found on disk
  if (mode === "recovery") {
    return (
      <div className="flex h-screen flex-col overflow-hidden rounded-lg border border-white/10 bg-[#09090b]">
        <Titlebar title="Roxlit" />
        <Recovery
          discoveredProjects={discoveredProjects}
          onRecovered={(project) => {
            launcher.setProject(project);
            setMode("launcher");
            setTimeout(() => launcher.startDevelopment(), 300);
          }}
          onStartFresh={() => {
            installer.reset();
            setMode("installer");
          }}
          onRescan={(projects) => setDiscoveredProjects(projects)}
        />
      </div>
    );
  }

  // Installer mode
  return (
    <div className="flex h-screen flex-col overflow-hidden rounded-lg border border-white/10 bg-[#09090b]">
      <Titlebar title="Roxlit Installer" />
      <StepIndicator currentStep={installer.step} />

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <AnimatePresence mode="wait">
          {installer.step === "welcome" && (
            <Welcome
              key="welcome"
              onNext={() => installer.goToStep("selectTool")}
            />
          )}

          {installer.step === "selectTool" && (
            <SelectTool
              key="selectTool"
              selected={installer.aiTool}
              onSelect={installer.setAiTool}
              onNext={() => installer.goToStep("selectProject")}
              onBack={() => installer.goToStep("welcome")}
            />
          )}

          {installer.step === "selectProject" && (
            <SelectProject
              key="selectProject"
              projectName={installer.projectName}
              parentDir={installer.projectParentDir}
              fullPath={installer.projectFullPath}
              onNameChange={installer.setProjectName}
              onPickDirectory={installer.pickDirectory}
              onNext={() => installer.goToStep("detecting")}
              onBack={() => installer.goToStep("selectTool")}
            />
          )}

          {installer.step === "detecting" && (
            <Detecting
              key="detecting"
              detection={installer.detection}
              isDetecting={installer.isDetecting}
              onDetect={installer.runDetection}
              onNext={() => installer.goToStep("installing")}
              onBack={() => installer.goToStep("selectProject")}
            />
          )}

          {installer.step === "installing" && (
            <Installing
              key="installing"
              events={installer.installEvents}
              error={installer.installError}
              onInstall={installer.runInstallation}
            />
          )}

          {installer.step === "complete" && (
            <Complete
              key="complete"
              projectPath={installer.projectFullPath}
              projectName={installer.projectName}
              aiTool={installer.aiTool!}
              aiToolName={aiToolName}
              onGoToLauncher={handleGoToLauncher}
            />
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
