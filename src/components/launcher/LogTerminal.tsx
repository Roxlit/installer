import { useEffect, useRef, useState } from "react";
import { Copy, Check } from "lucide-react";

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
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs.length]);

  function handleCopy() {
    navigator.clipboard.writeText(logs.join("\n")).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-white/5 bg-black/60">
      <div className="flex shrink-0 items-center justify-between border-b border-white/5 px-3 py-1.5">
        <div className="flex items-center gap-2">
          <div className="h-2 w-2 rounded-full bg-zinc-600" />
          <span className="text-[10px] font-medium uppercase tracking-wider text-zinc-600">
            Terminal
          </span>
        </div>
        {logs.length > 0 && (
          <button
            onClick={handleCopy}
            className="flex items-center gap-1 text-[10px] text-zinc-600 transition-colors hover:text-zinc-400"
            title="Copy all logs"
          >
            {copied ? (
              <>
                <Check className="h-3 w-3" />
                Copied
              </>
            ) : (
              <>
                <Copy className="h-3 w-3" />
                Copy All
              </>
            )}
          </button>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3 font-mono text-xs leading-5">
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
