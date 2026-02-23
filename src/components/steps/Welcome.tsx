import { motion } from "framer-motion";
import { ArrowRight } from "lucide-react";
import { RoxlitIcon } from "../ToolIcons";

interface WelcomeProps {
  onNext: () => void;
}

export function Welcome({ onNext }: WelcomeProps) {
  return (
    <motion.div
      className="flex flex-1 flex-col items-center justify-center px-8"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.3 }}
    >
      {/* Logo with glow */}
      <div className="relative">
        <div className="absolute inset-0 blur-[40px]">
          <div className="h-full w-full rounded-full bg-emerald-500/20" />
        </div>
        <RoxlitIcon className="relative h-16 w-16 text-emerald-400" />
      </div>

      <h1 className="mt-8 text-2xl font-bold tracking-tight">
        Roxlit Installer
      </h1>

      <p className="mt-3 max-w-sm text-center text-sm leading-relaxed text-zinc-400">
        Set up AI-powered Roblox development in a few clicks. This installer
        will configure Rojo, create your project structure, and generate AI
        context files.
      </p>

      <div className="mt-4 flex flex-wrap justify-center gap-2 text-[11px] text-zinc-500">
        <span className="rounded-full border border-white/5 bg-white/[0.02] px-3 py-1">
          Rojo sync
        </span>
        <span className="rounded-full border border-white/5 bg-white/[0.02] px-3 py-1">
          Studio plugin
        </span>
        <span className="rounded-full border border-white/5 bg-white/[0.02] px-3 py-1">
          AI context
        </span>
        <span className="rounded-full border border-white/5 bg-white/[0.02] px-3 py-1">
          Project structure
        </span>
      </div>

      <button
        onClick={onNext}
        className="mt-10 flex items-center gap-2 rounded-lg bg-emerald-500 px-6 py-2.5 text-sm font-semibold text-black transition-colors hover:bg-emerald-400"
      >
        Get Started
        <ArrowRight className="h-4 w-4" />
      </button>
    </motion.div>
  );
}
