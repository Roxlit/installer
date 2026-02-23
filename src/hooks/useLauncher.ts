import { useReducer, useCallback, useRef } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import type { ProjectEntry, RojoEvent, RojoStatus } from "@/lib/types";

const MAX_LOGS = 500;

interface LauncherState {
  project: ProjectEntry | null;
  rojoStatus: RojoStatus;
  rojoPort: number | null;
  rojoLogs: string[];
  error: string | null;
}

type Action =
  | { type: "SET_PROJECT"; project: ProjectEntry }
  | { type: "ROJO_STARTING" }
  | { type: "ROJO_STARTED"; port: number }
  | { type: "ROJO_OUTPUT"; line: string; stream: string }
  | { type: "ROJO_STOPPED"; code: number | null }
  | { type: "ROJO_ERROR"; message: string }
  | { type: "CLEAR_LOGS" };

const initialState: LauncherState = {
  project: null,
  rojoStatus: "stopped",
  rojoPort: null,
  rojoLogs: [],
  error: null,
};

function reducer(state: LauncherState, action: Action): LauncherState {
  switch (action.type) {
    case "SET_PROJECT":
      return { ...state, project: action.project };
    case "ROJO_STARTING":
      return {
        ...state,
        rojoStatus: "starting",
        rojoPort: null,
        rojoLogs: [],
        error: null,
      };
    case "ROJO_STARTED":
      return { ...state, rojoStatus: "running", rojoPort: action.port };
    case "ROJO_OUTPUT": {
      const prefix = action.stream === "stderr" ? "[err] " : "";
      const logs = [...state.rojoLogs, `${prefix}${action.line}`];
      return {
        ...state,
        rojoLogs: logs.length > MAX_LOGS ? logs.slice(-MAX_LOGS) : logs,
      };
    }
    case "ROJO_STOPPED":
      return { ...state, rojoStatus: "stopped", rojoPort: null };
    case "ROJO_ERROR":
      return { ...state, rojoStatus: "error", error: action.message };
    case "CLEAR_LOGS":
      return { ...state, rojoLogs: [] };
    default:
      return state;
  }
}

export function useLauncher() {
  const [state, dispatch] = useReducer(reducer, initialState);
  const channelRef = useRef<Channel<RojoEvent> | null>(null);

  const setProject = useCallback((project: ProjectEntry) => {
    dispatch({ type: "SET_PROJECT", project });
  }, []);

  const startRojo = useCallback(async () => {
    if (!state.project) return;

    dispatch({ type: "ROJO_STARTING" });

    const channel = new Channel<RojoEvent>();
    channelRef.current = channel;

    channel.onmessage = (event) => {
      switch (event.event) {
        case "output":
          dispatch({
            type: "ROJO_OUTPUT",
            line: event.data.line,
            stream: event.data.stream,
          });
          break;
        case "started":
          dispatch({ type: "ROJO_STARTED", port: event.data.port });
          break;
        case "stopped":
          dispatch({ type: "ROJO_STOPPED", code: event.data.code });
          channelRef.current = null;
          break;
        case "error":
          dispatch({ type: "ROJO_ERROR", message: event.data.message });
          channelRef.current = null;
          break;
      }
    };

    try {
      await invoke("start_rojo", {
        projectPath: state.project.path,
        onEvent: channel,
      });
    } catch (err) {
      dispatch({
        type: "ROJO_ERROR",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }, [state.project]);

  const stopRojo = useCallback(async () => {
    try {
      await invoke("stop_rojo");
      dispatch({ type: "ROJO_STOPPED", code: null });
    } catch (err) {
      dispatch({
        type: "ROJO_ERROR",
        message: err instanceof Error ? err.message : String(err),
      });
    }
    channelRef.current = null;
  }, []);

  const startDevelopment = useCallback(async () => {
    if (!state.project) return;

    // Start rojo first
    await startRojo();

    // Then open editor
    try {
      await invoke("open_in_editor", {
        editor: state.project.aiTool,
        path: state.project.path,
      });
    } catch {
      // Editor open failure is non-critical
    }
  }, [state.project, startRojo]);

  const openEditor = useCallback(async () => {
    if (!state.project) return;
    try {
      await invoke("open_in_editor", {
        editor: state.project.aiTool,
        path: state.project.path,
      });
    } catch {
      // Non-critical
    }
  }, [state.project]);

  const clearLogs = useCallback(() => {
    dispatch({ type: "CLEAR_LOGS" });
  }, []);

  return {
    ...state,
    setProject,
    startRojo,
    stopRojo,
    startDevelopment,
    openEditor,
    clearLogs,
  };
}
