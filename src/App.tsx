import { AnimatePresence } from "framer-motion";
import { Titlebar } from "./components/Titlebar";
import { StepIndicator } from "./components/StepIndicator";
import { Welcome } from "./components/steps/Welcome";
import { SelectTool } from "./components/steps/SelectTool";
import { SelectProject } from "./components/steps/SelectProject";
import { Detecting } from "./components/steps/Detecting";
import { Installing } from "./components/steps/Installing";
import { Complete } from "./components/steps/Complete";
import { useInstaller } from "./hooks/useInstaller";
import { TOOL_OPTIONS } from "./lib/types";

export default function App() {
  const installer = useInstaller();

  const aiToolName =
    TOOL_OPTIONS.find((t) => t.id === installer.aiTool)?.name ?? "your AI tool";

  return (
    <div className="flex h-screen flex-col overflow-hidden rounded-lg border border-white/10 bg-[#09090b]">
      <Titlebar />
      <StepIndicator currentStep={installer.step} />

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
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
              aiToolName={aiToolName}
            />
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
