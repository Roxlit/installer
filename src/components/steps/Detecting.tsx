import { useEffect } from "react";
import { motion } from "framer-motion";
import { ArrowRight, Check, X, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { DetectionResult } from "@/lib/types";

interface DetectingProps {
  detection: DetectionResult | null;
  isDetecting: boolean;
  onDetect: () => void;
  onNext: () => void;
  onBack: () => void;
}

interface CheckRow {
  label: string;
  status: "pass" | "fail" | "pending";
  detail?: string;
}

export function Detecting({
  detection,
  isDetecting,
  onDetect,
  onNext,
  onBack,
}: DetectingProps) {
  // Auto-run detection on mount
  useEffect(() => {
    if (!detection && !isDetecting) {
      onDetect();
    }
  }, [detection, isDetecting, onDetect]);

  const checks: CheckRow[] = detection
    ? [
        {
          label: "Operating System",
          status: "pass" as const,
          detail: detection.os,
        },
        {
          label: "Roblox Studio",
          status: detection.studioInstalled ? "pass" as const : "fail" as const,
          detail: detection.studioInstalled
            ? "Found"
            : "Not found — install Studio first",
        },
        {
          label: "Aftman",
          status: detection.aftmanInstalled ? "pass" as const : "pass" as const,
          detail: detection.aftmanInstalled
            ? detection.aftmanVersion ?? "Installed"
            : "Will be installed",
        },
        {
          label: "Rojo",
          status: detection.rojoInstalled ? "pass" as const : "pass" as const,
          detail: detection.rojoInstalled
            ? detection.rojoVersion ?? "Installed"
            : "Will be installed",
        },
        {
          label: "RbxSync",
          status: detection.os === "linux" ? "fail" as const : "pass" as const,
          detail: detection.os === "linux"
            ? "Not available on Linux"
            : detection.rbxsyncInstalled
              ? detection.rbxsyncVersion ?? "Installed"
              : "Will be installed",
        },
      ]
    : [
        { label: "Operating System", status: "pending" as const },
        { label: "Roblox Studio", status: "pending" as const },
        { label: "Aftman", status: "pending" as const },
        { label: "Rojo", status: "pending" as const },
        { label: "RbxSync", status: "pending" as const },
      ];

  return (
    <motion.div
      className="flex min-h-0 flex-1 flex-col px-8 py-6"
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -20 }}
      transition={{ duration: 0.3 }}
    >
      <div className="min-h-0 flex-1 overflow-hidden">
        <h2 className="text-lg font-semibold">Checking your system</h2>
        <p className="mt-1 text-sm text-zinc-400">
          Detecting installed tools and configuration.
        </p>

        <div className="mt-6 space-y-3">
          {checks.map((check, i) => (
            <motion.div
              key={check.label}
              className="flex items-center gap-3 rounded-lg border border-white/5 bg-white/[0.02] px-4 py-3"
              initial={{ opacity: 0, x: -10 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: i * 0.1 }}
            >
              <div className="flex h-6 w-6 items-center justify-center">
                {check.status === "pending" ? (
                  <Loader2 className="h-4 w-4 animate-spin text-zinc-500" />
                ) : check.status === "pass" ? (
                  <Check className="h-4 w-4 text-emerald-400" />
                ) : (
                  <X className="h-4 w-4 text-red-400" />
                )}
              </div>
              <div className="flex-1">
                <div className="text-sm">{check.label}</div>
                {check.detail && (
                  <div
                    className={cn(
                      "text-[11px]",
                      check.status === "fail"
                        ? "text-red-400"
                        : "text-zinc-500"
                    )}
                  >
                    {check.detail}
                  </div>
                )}
              </div>
            </motion.div>
          ))}
        </div>

        {/* Studio not found warning */}
        {detection && !detection.studioInstalled && (
          <div className="mt-4 rounded-lg border border-amber-500/20 bg-amber-500/[0.06] px-4 py-3 text-sm text-amber-400">
            Roblox Studio not found. The project and AI context files will still
            be created, but the Studio plugin won't be installed. You can install
            Studio later from{" "}
            <span className="font-medium text-amber-300">create.roblox.com</span>.
          </div>
        )}
      </div>

      {/* Navigation — always visible at bottom */}
      <div className="flex shrink-0 items-center justify-between border-t border-white/5 pt-4 mt-4">
        <button
          onClick={onBack}
          className="flex items-center gap-1.5 text-sm text-zinc-400 transition-colors hover:text-zinc-200"
        >
          Back
        </button>
        <button
          onClick={onNext}
          disabled={isDetecting}
          className={cn(
            "flex items-center gap-2 rounded-lg px-5 py-2 text-sm font-semibold transition-all",
            !isDetecting
              ? "bg-emerald-500 text-black hover:bg-emerald-400"
              : "cursor-not-allowed bg-white/5 text-zinc-600"
          )}
        >
          {isDetecting ? "Detecting..." : detection?.studioInstalled ? "Install" : "Continue anyway"}
          {!isDetecting && <ArrowRight className="h-3.5 w-3.5" />}
        </button>
      </div>
    </motion.div>
  );
}
