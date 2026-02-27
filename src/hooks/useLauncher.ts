import { useReducer, useCallback, useRef } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import type { ProjectEntry, RojoEvent, RojoStatus } from "@/lib/types";

const MAX_LOGS = 500;
const MAX_AUTO_RESTARTS = 3;
const RESTART_WINDOW_MS = 60_000; // reset counter after 1 min of stability
const RESTART_DELAY_MS = 2_000;

interface LauncherState {
  project: ProjectEntry | null;
  rojoStatus: RojoStatus;
  rojoPort: number | null;
  logs: string[];
  error: string | null;
}

type Action =
  | { type: "SET_PROJECT"; project: ProjectEntry }
  | { type: "ROJO_STARTING"; keepLogs?: boolean }
  | { type: "ROJO_STARTED"; port: number }
  | { type: "ROJO_OUTPUT"; line: string; stream: string }
  | { type: "ROJO_STOPPED"; code: number | null }
  | { type: "ROJO_ERROR"; message: string }
  | { type: "CLEAR_LOGS" };

const initialState: LauncherState = {
  project: null,
  rojoStatus: "stopped",
  rojoPort: null,
  logs: [],
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
        logs: action.keepLogs ? state.logs : [],
        error: null,
      };
    case "ROJO_STARTED":
      return { ...state, rojoStatus: "running", rojoPort: action.port };
    case "ROJO_OUTPUT": {
      const prefix =
        action.stream === "stderr" ? "[rojo] [err] " : "[rojo] ";
      const logs = [...state.logs, `${prefix}${action.line}`];
      return {
        ...state,
        logs: logs.length > MAX_LOGS ? logs.slice(-MAX_LOGS) : logs,
      };
    }
    case "ROJO_STOPPED":
      return { ...state, rojoStatus: "stopped", rojoPort: null };
    case "ROJO_ERROR":
      return { ...state, rojoStatus: "error", error: action.message };
    case "CLEAR_LOGS":
      return { ...state, logs: [] };
    default:
      return state;
  }
}

export function useLauncher() {
  const [state, dispatch] = useReducer(reducer, initialState);
  const rojoChannelRef = useRef<Channel<RojoEvent> | null>(null);
  const projectRef = useRef<ProjectEntry | null>(null);
  const stopRequestedRef = useRef(false);
  const restartCountRef = useRef(0);
  const lastCrashTimeRef = useRef(0);
  const restartTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Keep projectRef in sync
  const setProject = useCallback((project: ProjectEntry) => {
    projectRef.current = project;
    dispatch({ type: "SET_PROJECT", project });
  }, []);

  // --- Rojo event handler ---
  function createRojoEventHandler(): (event: RojoEvent) => void {
    return (event: RojoEvent) => {
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
          setTimeout(() => {
            restartCountRef.current = 0;
          }, RESTART_WINDOW_MS);
          break;
        case "stopped": {
          rojoChannelRef.current = null;
          const wasRequested = stopRequestedRef.current;
          stopRequestedRef.current = false;

          if (!wasRequested && projectRef.current) {
            const now = Date.now();
            if (now - lastCrashTimeRef.current > RESTART_WINDOW_MS) {
              restartCountRef.current = 0;
            }
            lastCrashTimeRef.current = now;
            restartCountRef.current++;

            if (restartCountRef.current <= MAX_AUTO_RESTARTS) {
              dispatch({
                type: "ROJO_OUTPUT",
                line: `Rojo crashed (exit code ${event.data.code}). Restarting automatically (${restartCountRef.current}/${MAX_AUTO_RESTARTS})...`,
                stream: "stderr",
              });
              dispatch({ type: "ROJO_STARTING", keepLogs: true });

              const projectPath = projectRef.current.path;
              restartTimerRef.current = setTimeout(() => {
                const ch = new Channel<RojoEvent>();
                rojoChannelRef.current = ch;
                ch.onmessage = createRojoEventHandler();
                invoke("start_rojo", {
                  projectPath,
                  onEvent: ch,
                }).catch((err) => {
                  dispatch({
                    type: "ROJO_ERROR",
                    message:
                      err instanceof Error ? err.message : String(err),
                  });
                });
              }, RESTART_DELAY_MS);
              return;
            }
            dispatch({
              type: "ROJO_OUTPUT",
              line: `Rojo crashed ${MAX_AUTO_RESTARTS} times. Click "Start Development" to try again.`,
              stream: "stderr",
            });
          }
          dispatch({ type: "ROJO_STOPPED", code: event.data.code });
          break;
        }
        case "error":
          dispatch({ type: "ROJO_ERROR", message: event.data.message });
          rojoChannelRef.current = null;
          break;
      }
    };
  }

  const startRojo = useCallback(async () => {
    const project = projectRef.current;
    if (!project) return;

    dispatch({ type: "ROJO_STARTING" });
    stopRequestedRef.current = false;
    restartCountRef.current = 0;

    const channel = new Channel<RojoEvent>();
    rojoChannelRef.current = channel;
    channel.onmessage = createRojoEventHandler();

    try {
      await invoke("start_rojo", {
        projectPath: project.path,
        onEvent: channel,
      });
    } catch (err) {
      dispatch({
        type: "ROJO_ERROR",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }, []);

  const stopAll = useCallback(async () => {
    stopRequestedRef.current = true;

    if (restartTimerRef.current) {
      clearTimeout(restartTimerRef.current);
      restartTimerRef.current = null;
    }

    try {
      await invoke("stop_rojo");
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      if (!msg.includes("not running") && !msg.includes("already")) {
        dispatch({ type: "ROJO_ERROR", message: msg });
      }
    }

    dispatch({ type: "ROJO_STOPPED", code: null });
    rojoChannelRef.current = null;
  }, []);

  const startDevelopment = useCallback(async () => {
    const project = projectRef.current;
    if (!project) return;

    // Stop any running servers first (silently)
    await stopAll();

    // Start rojo
    await startRojo();

    // Then open editor
    try {
      await invoke("open_in_editor", {
        editor: project.aiTool,
        path: project.path,
      });
    } catch {
      // Editor open failure is non-critical
    }
  }, [startRojo, stopAll]);

  const openEditor = useCallback(async () => {
    const project = projectRef.current;
    if (!project) return;
    try {
      await invoke("open_in_editor", {
        editor: project.aiTool,
        path: project.path,
      });
    } catch {
      // Non-critical
    }
  }, []);

  const clearLogs = useCallback(() => {
    dispatch({ type: "CLEAR_LOGS" });
  }, []);

  return {
    ...state,
    setProject,
    startRojo,
    stopAll,
    startDevelopment,
    openEditor,
    clearLogs,
  };
}
