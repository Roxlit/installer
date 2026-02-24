import { useReducer, useCallback, useRef } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  ProjectEntry,
  RojoEvent,
  RojoStatus,
  RbxSyncEvent,
  RbxSyncStatus,
} from "@/lib/types";

const MAX_LOGS = 500;
const MAX_AUTO_RESTARTS = 3;
const RESTART_WINDOW_MS = 60_000; // reset counter after 1 min of stability
const RESTART_DELAY_MS = 2_000;

interface LauncherState {
  project: ProjectEntry | null;
  rojoStatus: RojoStatus;
  rojoPort: number | null;
  rbxsyncStatus: RbxSyncStatus;
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
  | { type: "RBXSYNC_STARTING" }
  | { type: "RBXSYNC_STARTED" }
  | { type: "RBXSYNC_OUTPUT"; line: string; stream: string }
  | { type: "RBXSYNC_STOPPED"; code: number | null }
  | { type: "RBXSYNC_ERROR"; message: string }
  | { type: "RBXSYNC_UNAVAILABLE" }
  | { type: "CLEAR_LOGS" };

const initialState: LauncherState = {
  project: null,
  rojoStatus: "stopped",
  rojoPort: null,
  rbxsyncStatus: "stopped",
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
    case "RBXSYNC_STARTING":
      return { ...state, rbxsyncStatus: "starting" };
    case "RBXSYNC_STARTED":
      return { ...state, rbxsyncStatus: "running" };
    case "RBXSYNC_OUTPUT": {
      const prefix =
        action.stream === "stderr"
          ? "[rbxsync] [err] "
          : "[rbxsync] ";
      const logs = [...state.logs, `${prefix}${action.line}`];
      return {
        ...state,
        logs: logs.length > MAX_LOGS ? logs.slice(-MAX_LOGS) : logs,
      };
    }
    case "RBXSYNC_STOPPED":
      return { ...state, rbxsyncStatus: "stopped" };
    case "RBXSYNC_ERROR":
      return { ...state, rbxsyncStatus: "error", error: action.message };
    case "RBXSYNC_UNAVAILABLE":
      return { ...state, rbxsyncStatus: "unavailable" };
    case "CLEAR_LOGS":
      return { ...state, logs: [] };
    default:
      return state;
  }
}

export function useLauncher() {
  const [state, dispatch] = useReducer(reducer, initialState);
  const rojoChannelRef = useRef<Channel<RojoEvent> | null>(null);
  const rbxsyncChannelRef = useRef<Channel<RbxSyncEvent> | null>(null);
  const projectRef = useRef<ProjectEntry | null>(null);
  const stopRequestedRef = useRef(false);
  const restartCountRef = useRef(0);
  const lastCrashTimeRef = useRef(0);
  const restartTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // RbxSync auto-restart refs
  const rbxsyncStopRequestedRef = useRef(false);
  const rbxsyncRestartCountRef = useRef(0);
  const rbxsyncLastCrashTimeRef = useRef(0);
  const rbxsyncRestartTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );

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

  // Track whether we already triggered extraction for this session
  const extractionTriggeredRef = useRef(false);

  // --- RbxSync event handler ---
  function createRbxSyncEventHandler(): (event: RbxSyncEvent) => void {
    return (event: RbxSyncEvent) => {
      switch (event.event) {
        case "output":
          dispatch({
            type: "RBXSYNC_OUTPUT",
            line: event.data.line,
            stream: event.data.stream,
          });
          // Auto-extract when Studio connects for the first time
          if (
            !extractionTriggeredRef.current &&
            event.data.line.includes("Studio registered")
          ) {
            extractionTriggeredRef.current = true;
            const project = projectRef.current;
            if (project) {
              dispatch({
                type: "RBXSYNC_OUTPUT",
                line: "Extracting full DataModel from Studio...",
                stream: "stdout",
              });
              invoke("extract_rbxsync", { projectPath: project.path })
                .then((result) => {
                  dispatch({
                    type: "RBXSYNC_OUTPUT",
                    line: `Extraction complete: ${result}`,
                    stream: "stdout",
                  });
                })
                .catch((err) => {
                  dispatch({
                    type: "RBXSYNC_OUTPUT",
                    line: `Extraction failed: ${err instanceof Error ? err.message : String(err)}`,
                    stream: "stderr",
                  });
                });
            }
          }
          break;
        case "started":
          dispatch({ type: "RBXSYNC_STARTED" });
          setTimeout(() => {
            rbxsyncRestartCountRef.current = 0;
          }, RESTART_WINDOW_MS);
          break;
        case "stopped": {
          rbxsyncChannelRef.current = null;
          const wasRequested = rbxsyncStopRequestedRef.current;
          rbxsyncStopRequestedRef.current = false;

          if (!wasRequested && projectRef.current) {
            const now = Date.now();
            if (
              now - rbxsyncLastCrashTimeRef.current >
              RESTART_WINDOW_MS
            ) {
              rbxsyncRestartCountRef.current = 0;
            }
            rbxsyncLastCrashTimeRef.current = now;
            rbxsyncRestartCountRef.current++;

            if (rbxsyncRestartCountRef.current <= MAX_AUTO_RESTARTS) {
              dispatch({
                type: "RBXSYNC_OUTPUT",
                line: `RbxSync crashed (exit code ${event.data.code}). Restarting automatically (${rbxsyncRestartCountRef.current}/${MAX_AUTO_RESTARTS})...`,
                stream: "stderr",
              });
              dispatch({ type: "RBXSYNC_STARTING" });

              const projectPath = projectRef.current.path;
              rbxsyncRestartTimerRef.current = setTimeout(() => {
                const ch = new Channel<RbxSyncEvent>();
                rbxsyncChannelRef.current = ch;
                ch.onmessage = createRbxSyncEventHandler();
                invoke("start_rbxsync", {
                  projectPath,
                  onEvent: ch,
                }).catch((err) => {
                  dispatch({
                    type: "RBXSYNC_ERROR",
                    message:
                      err instanceof Error ? err.message : String(err),
                  });
                });
              }, RESTART_DELAY_MS);
              return;
            }
            dispatch({
              type: "RBXSYNC_OUTPUT",
              line: `RbxSync crashed ${MAX_AUTO_RESTARTS} times. Click "Start Development" to try again.`,
              stream: "stderr",
            });
          }
          dispatch({ type: "RBXSYNC_STOPPED", code: event.data.code });
          break;
        }
        case "error":
          dispatch({ type: "RBXSYNC_ERROR", message: event.data.message });
          rbxsyncChannelRef.current = null;
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

  const startRbxSync = useCallback(async () => {
    const project = projectRef.current;
    if (!project) return;

    dispatch({ type: "RBXSYNC_STARTING" });
    rbxsyncStopRequestedRef.current = false;
    rbxsyncRestartCountRef.current = 0;
    extractionTriggeredRef.current = false;

    const channel = new Channel<RbxSyncEvent>();
    rbxsyncChannelRef.current = channel;
    channel.onmessage = createRbxSyncEventHandler();

    try {
      await invoke("start_rbxsync", {
        projectPath: project.path,
        onEvent: channel,
      });
    } catch (err) {
      // If rbxsync binary not found, mark as unavailable
      const msg = err instanceof Error ? err.message : String(err);
      if (
        msg.includes("not found") ||
        msg.includes("No such file") ||
        msg.includes("os error 2")
      ) {
        dispatch({ type: "RBXSYNC_UNAVAILABLE" });
      } else {
        dispatch({ type: "RBXSYNC_ERROR", message: msg });
      }
    }
  }, []);

  const stopAll = useCallback(async () => {
    stopRequestedRef.current = true;
    rbxsyncStopRequestedRef.current = true;

    if (restartTimerRef.current) {
      clearTimeout(restartTimerRef.current);
      restartTimerRef.current = null;
    }
    if (rbxsyncRestartTimerRef.current) {
      clearTimeout(rbxsyncRestartTimerRef.current);
      rbxsyncRestartTimerRef.current = null;
    }

    // Stop both in parallel
    const results = await Promise.allSettled([
      invoke("stop_rojo"),
      invoke("stop_rbxsync"),
    ]);

    dispatch({ type: "ROJO_STOPPED", code: null });
    if (state.rbxsyncStatus !== "unavailable") {
      dispatch({ type: "RBXSYNC_STOPPED", code: null });
    }

    rojoChannelRef.current = null;
    rbxsyncChannelRef.current = null;

    // Report errors if both failed
    for (const result of results) {
      if (result.status === "rejected") {
        const msg =
          result.reason instanceof Error
            ? result.reason.message
            : String(result.reason);
        // Ignore "not running" errors
        if (!msg.includes("not running") && !msg.includes("already")) {
          dispatch({ type: "ROJO_ERROR", message: msg });
        }
      }
    }
  }, [state.rbxsyncStatus]);

  const startDevelopment = useCallback(async () => {
    const project = projectRef.current;
    if (!project) return;

    // Stop any running servers first (silently)
    await stopAll();

    // Start rojo and rbxsync in parallel
    await Promise.all([startRojo(), startRbxSync()]);

    // Then open editor
    try {
      await invoke("open_in_editor", {
        editor: project.aiTool,
        path: project.path,
      });
    } catch {
      // Editor open failure is non-critical
    }
  }, [startRojo, startRbxSync, stopAll]);

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
