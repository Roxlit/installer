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
import { TOOL_OPTIONS } from "./lib/types";
import type { AppMode, ProjectEntry, RoxlitConfig } from "./lib/types";

export default function App() {
  const [mode, setMode] = useState<AppMode>("loading");
  const [config, setConfig] = useState<RoxlitConfig | null>(null);
  const installer = useInstaller();
  const launcher = useLauncher();
  const { update, dismissUpdate } = useUpdateChecker(config);

  // Boot: check for existing config
  useEffect(() => {
    invoke<RoxlitConfig | null>("load_config")
      .then((loadedConfig) => {
        if (loadedConfig && loadedConfig.projects.length > 0) {
          setConfig(loadedConfig);
          const active =
            loadedConfig.projects.find(
              (p) => p.path === loadedConfig.lastActiveProject
            ) ?? loadedConfig.projects[0];
          launcher.setProject(active);
          setMode("launcher");
        } else {
          setMode("installer");
        }
      })
      .catch(() => {
        setMode("installer");
      });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleGoToLauncher = (project: ProjectEntry) => {
    launcher.setProject(project);
    setMode("launcher");
    // Auto-start development
    setTimeout(() => {
      launcher.startDevelopment();
    }, 300);
  };

  const handleNewProject = async () => {
    // Stop running servers before switching to installer
    await launcher.stopAll();
    installer.reset();
    setMode("installer");
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
          rbxsyncStatus={launcher.rbxsyncStatus}
          autoSyncStatus={launcher.autoSyncStatus}
          logs={launcher.logs}
          error={launcher.error}
          update={update}
          onStartDevelopment={launcher.startDevelopment}
          onStopAll={launcher.stopAll}
          onOpenEditor={launcher.openEditor}
          onNewProject={handleNewProject}
          onDismissUpdate={dismissUpdate}
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
