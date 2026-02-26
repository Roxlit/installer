import { useState, useRef, useEffect } from "react";
import { Settings } from "lucide-react";

const DELAY_OPTIONS = [
  { value: 0, label: "Immediate" },
  { value: 1, label: "1 day" },
  { value: 3, label: "3 days" },
  { value: 7, label: "7 days" },
  { value: 14, label: "14 days" },
];

interface SettingsPopoverProps {
  updateDelayDays: number;
  onUpdateDelayChange: (days: number) => void;
}

export function SettingsPopover({
  updateDelayDays,
  onUpdateDelayChange,
}: SettingsPopoverProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 text-xs text-zinc-500 transition-colors hover:text-zinc-300"
        title="Settings"
      >
        <Settings className="h-3.5 w-3.5" />
      </button>

      {open && (
        <div className="absolute bottom-full left-0 mb-2 w-48 rounded-lg border border-white/10 bg-zinc-900 p-3 shadow-xl">
          <label className="block text-xs font-medium text-zinc-400">
            Update delay
          </label>
          <select
            value={updateDelayDays}
            onChange={(e) => {
              onUpdateDelayChange(Number(e.target.value));
              setOpen(false);
            }}
            className="mt-1.5 w-full rounded-md border border-white/10 bg-white/[0.03] px-2 py-1.5 text-xs text-zinc-300 outline-none focus:border-emerald-500/50"
          >
            {DELAY_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>
      )}
    </div>
  );
}
