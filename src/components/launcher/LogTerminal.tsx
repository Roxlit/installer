import { useEffect, useRef } from "react";

interface LogTerminalProps {
  logs: string[];
}

function getLineClass(line: string): string {
  if (line.includes("[err]")) return "text-yellow-400/80";
  if (line.startsWith("[rbxsync]")) return "text-blue-400/70";
  if (line.startsWith("[rojo]")) return "text-emerald-400/70";
  return "text-zinc-400/70";
}

export function LogTerminal({ logs }: LogTerminalProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs.length]);

  return (
    <div className="flex flex-1 flex-col overflow-hidden rounded-lg border border-white/5 bg-black/60">
      <div className="flex items-center gap-2 border-b border-white/5 px-3 py-1.5">
        <div className="h-2 w-2 rounded-full bg-zinc-600" />
        <span className="text-[10px] font-medium uppercase tracking-wider text-zinc-600">
          Terminal
        </span>
      </div>
      <div className="flex-1 overflow-y-auto p-3 font-mono text-xs leading-5">
        {logs.length === 0 ? (
          <span className="text-zinc-600">
            Waiting for servers to start...
          </span>
        ) : (
          logs.map((line, i) => (
            <div key={i} className={getLineClass(line)}>
              {line}
            </div>
          ))
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
