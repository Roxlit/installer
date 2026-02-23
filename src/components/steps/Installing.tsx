import { useEffect } from "react";
import { motion } from "framer-motion";
import { Check, AlertTriangle, Loader2 } from "lucide-react";
import { ProgressBar } from "../ProgressBar";
import type { SetupEvent } from "@/lib/types";

interface InstallingProps {
  events: SetupEvent[];
  error: string | null;
  onInstall: () => void;
}

export function Installing({ events, error, onInstall }: InstallingProps) {
  // Start installation on mount
  useEffect(() => {
    if (events.length === 0) {
      onInstall();
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Calculate overall progress from events
  const lastStarted = [...events]
    .reverse()
    .find((e) => e.event === "stepStarted");
  const totalSteps =
    lastStarted?.event === "stepStarted" ? lastStarted.data.totalSteps : 5;
  const completedSteps = events.filter(
    (e) => e.event === "stepCompleted"
  ).length;
  const overallProgress = (completedSteps / totalSteps) * 100;

  // Build a readable list of what's happening
  const displayEvents = events.filter(
    (e) =>
      e.event === "stepStarted" ||
      e.event === "stepCompleted" ||
      e.event === "stepWarning" ||
      e.event === "error"
  );

  // Get the current in-progress detail (latest stepProgress)
  const latestProgress = [...events]
    .reverse()
    .find((e) => e.event === "stepProgress");

  return (
    <motion.div
      className="flex flex-1 flex-col px-8 py-6"
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -20 }}
      transition={{ duration: 0.3 }}
    >
      <h2 className="text-lg font-semibold">Setting up your environment</h2>
      <p className="mt-1 text-sm text-zinc-400">
        This will only take a moment. Please don't close this window.
      </p>

      <div className="mt-6 flex-1 space-y-2 overflow-y-auto">
        {displayEvents.map((event, i) => (
          <motion.div
            key={i}
            className="flex items-start gap-3 rounded-lg px-3 py-2"
            initial={{ opacity: 0, y: 5 }}
            animate={{ opacity: 1, y: 0 }}
          >
            <div className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center">
              {event.event === "stepCompleted" ? (
                <Check className="h-4 w-4 text-emerald-400" />
              ) : event.event === "stepWarning" ? (
                <AlertTriangle className="h-4 w-4 text-yellow-400" />
              ) : event.event === "error" ? (
                <AlertTriangle className="h-4 w-4 text-red-400" />
              ) : (
                <Loader2 className="h-4 w-4 animate-spin text-emerald-400" />
              )}
            </div>
            <div className="flex-1">
              <div className="text-sm">
                {event.event === "stepStarted" && event.data.description}
                {event.event === "stepCompleted" && event.data.detail}
                {event.event === "stepWarning" && event.data.message}
                {event.event === "error" && event.data.message}
              </div>
            </div>
          </motion.div>
        ))}

        {/* Current progress detail */}
        {latestProgress?.event === "stepProgress" && !error && (
          <div className="px-3 text-[11px] text-zinc-500">
            {latestProgress.data.detail}
          </div>
        )}
      </div>

      {/* Error display */}
      {error && (
        <div className="mt-4 rounded-lg border border-red-500/20 bg-red-500/[0.05] px-4 py-3">
          <div className="text-sm text-red-400">{error}</div>
          <button
            onClick={() => window.location.reload()}
            className="mt-2 text-xs text-red-300 underline hover:text-red-200"
          >
            Start over
          </button>
        </div>
      )}

      {/* Progress bar */}
      <div className="mt-4 pt-4">
        <div className="mb-2 flex items-center justify-between text-[11px] text-zinc-500">
          <span>
            {completedSteps} of {totalSteps} steps
          </span>
          <span>{Math.round(overallProgress)}%</span>
        </div>
        <ProgressBar progress={overallProgress} />
      </div>
    </motion.div>
  );
}
