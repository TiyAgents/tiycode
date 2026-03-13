import { matchModelIcon } from "@/shared/lib/llm-brand-matcher";
import { cn } from "@/shared/lib/utils";
import { LocalLlmIcon } from "@/shared/ui/local-llm-icon";

type ModelBrandIconProps = {
  className?: string;
  displayName?: string;
  modelId: string;
};

export function ModelBrandIcon({ className, displayName, modelId }: ModelBrandIconProps) {
  const slug = matchModelIcon(modelId, displayName);

  if (slug) {
    return <LocalLlmIcon className={cn("text-app-muted", className)} slug={slug} title={displayName || modelId} />;
  }

  const initial = getDisplayInitial(displayName) || getDisplayInitial(modelId);

  return (
    <div
      className={cn(
        "flex shrink-0 items-center justify-center rounded text-[10px] font-semibold text-app-muted",
        className,
      )}
    >
      {initial}
    </div>
  );
}

function getDisplayInitial(value?: string) {
  const candidate = value?.trim();
  return candidate ? candidate.charAt(0).toUpperCase() : "?";
}
