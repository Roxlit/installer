import { useReducer, useCallback } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  WizardStep,
  AiTool,
  DetectionResult,
  SetupEvent,
  InstallConfig,
} from "@/lib/types";

interface InstallerState {
  step: WizardStep;
  aiTool: AiTool | null;
  projectName: string;
  projectParentDir: string;
  detection: DetectionResult | null;
  isDetecting: boolean;
  installEvents: SetupEvent[];
  installFinished: boolean;
  installError: string | null;
}

type Action =
  | { type: "RESET" }
  | { type: "SET_STEP"; step: WizardStep }
  | { type: "SET_AI_TOOL"; tool: AiTool }
  | { type: "SET_PROJECT_NAME"; name: string }
  | { type: "SET_PROJECT_DIR"; dir: string }
  | { type: "DETECT_START" }
  | { type: "DETECT_DONE"; result: DetectionResult }
  | { type: "DETECT_ERROR"; error: string }
  | { type: "INSTALL_EVENT"; event: SetupEvent }
  | { type: "INSTALL_ERROR"; error: string };

const defaultParentDir = "~/RobloxProjects";

const initialState: InstallerState = {
  step: "welcome",
  aiTool: null,
  projectName: "my-roblox-game",
  projectParentDir: defaultParentDir,
  detection: null,
  isDetecting: false,
  installEvents: [],
  installFinished: false,
  installError: null,
};

function reducer(state: InstallerState, action: Action): InstallerState {
  switch (action.type) {
    case "RESET":
      return initialState;
    case "SET_STEP":
      return { ...state, step: action.step };
    case "SET_AI_TOOL":
      return { ...state, aiTool: action.tool };
    case "SET_PROJECT_NAME":
      return { ...state, projectName: action.name };
    case "SET_PROJECT_DIR":
      return { ...state, projectParentDir: action.dir };
    case "DETECT_START":
      return { ...state, isDetecting: true, detection: null };
    case "DETECT_DONE":
      return { ...state, isDetecting: false, detection: action.result };
    case "DETECT_ERROR":
      return {
        ...state,
        isDetecting: false,
        installError: action.error,
      };
    case "INSTALL_EVENT":
      return {
        ...state,
        installEvents: [...state.installEvents, action.event],
        installFinished: action.event.event === "finished",
      };
    case "INSTALL_ERROR":
      return { ...state, installError: action.error };
    default:
      return state;
  }
}

export function useInstaller() {
  const [state, dispatch] = useReducer(reducer, initialState);

  const reset = useCallback(() => {
    dispatch({ type: "RESET" });
  }, []);

  const goToStep = useCallback((step: WizardStep) => {
    dispatch({ type: "SET_STEP", step });
  }, []);

  const setAiTool = useCallback((tool: AiTool) => {
    dispatch({ type: "SET_AI_TOOL", tool });
  }, []);

  const setProjectName = useCallback((name: string) => {
    dispatch({ type: "SET_PROJECT_NAME", name });
  }, []);

  const pickDirectory = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      dispatch({ type: "SET_PROJECT_DIR", dir: selected as string });
    }
  }, []);

  const runDetection = useCallback(async () => {
    dispatch({ type: "DETECT_START" });
    try {
      const result = await invoke<DetectionResult>("detect_environment");
      dispatch({ type: "DETECT_DONE", result });
    } catch (err) {
      dispatch({
        type: "DETECT_ERROR",
        error: err instanceof Error ? err.message : String(err),
      });
    }
  }, []);

  const runInstallation = useCallback(async () => {
    if (!state.aiTool || !state.detection) return;

    const projectPath = state.projectParentDir
      ? `${state.projectParentDir}/${state.projectName}`
      : state.projectName;

    const config: InstallConfig = {
      aiTool: state.aiTool,
      projectPath,
      projectName: state.projectName,
      skipAftman: state.detection.aftmanInstalled,
      skipRojo: state.detection.rojoInstalled,
      skipRbxsync: state.detection.rbxsyncInstalled || state.detection.os === "linux",
      pluginsPath: state.detection.studioPluginsPath,
    };

    let hasError = false;

    const channel = new Channel<SetupEvent>();
    channel.onmessage = (event) => {
      dispatch({ type: "INSTALL_EVENT", event });
      if (event.event === "error") {
        hasError = true;
      }
      if (event.event === "finished" && !hasError) {
        dispatch({ type: "SET_STEP", step: "complete" });
      }
    };

    try {
      await invoke("run_installation", { config, onEvent: channel });
    } catch (err) {
      dispatch({
        type: "INSTALL_ERROR",
        error: err instanceof Error ? err.message : String(err),
      });
    }
  }, [state.aiTool, state.detection, state.projectParentDir, state.projectName]);

  const projectFullPath = state.projectParentDir
    ? `${state.projectParentDir}/${state.projectName}`
    : state.projectName;

  return {
    ...state,
    projectFullPath,
    reset,
    goToStep,
    setAiTool,
    setProjectName,
    pickDirectory,
    runDetection,
    runInstallation,
  };
}
