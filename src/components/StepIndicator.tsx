import { cn } from "@/lib/utils";
import { WIZARD_STEPS, type WizardStep } from "@/lib/types";
import { Check } from "lucide-react";

interface StepIndicatorProps {
  currentStep: WizardStep;
}

export function StepIndicator({ currentStep }: StepIndicatorProps) {
  const currentIndex = WIZARD_STEPS.findIndex((s) => s.key === currentStep);

  return (
    <div className="flex items-center justify-center gap-2 px-6 py-4">
      {WIZARD_STEPS.map((step, i) => {
        const isCompleted = i < currentIndex;
        const isCurrent = i === currentIndex;

        return (
          <div key={step.key} className="flex items-center gap-2">
            <div
              className={cn(
                "flex h-6 w-6 items-center justify-center rounded-full text-[10px] font-medium transition-all duration-300",
                isCompleted &&
                  "bg-emerald-500 text-black",
                isCurrent &&
                  "bg-emerald-500/20 text-emerald-400 ring-1 ring-emerald-500/50",
                !isCompleted &&
                  !isCurrent &&
                  "bg-white/5 text-zinc-600"
              )}
            >
              {isCompleted ? (
                <Check className="h-3 w-3" />
              ) : (
                i + 1
              )}
            </div>
            {i < WIZARD_STEPS.length - 1 && (
              <div
                className={cn(
                  "h-px w-6 transition-colors duration-300",
                  i < currentIndex ? "bg-emerald-500/50" : "bg-white/10"
                )}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}
