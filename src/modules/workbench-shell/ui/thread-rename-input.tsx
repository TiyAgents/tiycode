import { useCallback, useRef, useState } from "react";
import { Sparkles } from "lucide-react";
import { useT } from "@/i18n";
import { threadRegenerateTitle } from "@/services/bridge";
import type { RunModelPlanDto } from "@/shared/types/api";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { cn } from "@/shared/lib/utils";
import { DRAWER_LIST_ROW_CLASS } from "@/modules/workbench-shell/model/fixtures";
import type { ThreadStatus } from "@/modules/workbench-shell/model/types";
import { ThreadStatusIndicator } from "@/modules/workbench-shell/ui/thread-status-indicator";

type ThreadRenameInputProps = {
  threadId: string;
  initialName: string;
  isActive: boolean;
  status: ThreadStatus;
  modelPlan: RunModelPlanDto | null;
  onDone: (newTitle: string | null) => void;
};

export function ThreadRenameInput({
  threadId,
  initialName,
  isActive,
  status,
  modelPlan,
  onDone,
}: ThreadRenameInputProps) {
  const t = useT();
  const [value, setValue] = useState(initialName);
  const [isRegenerating, setRegenerating] = useState(false);
  const [regenError, setRegenError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // After regeneration completes, re-focus the input so the user
  // is not left in a stuck editing state without a focused input.
  const refocusAfterRegenerate = useCallback(() => {
    inputRef.current?.focus();
  }, []);

  const save = useCallback(() => {
    const trimmed = value.trim();
    onDone(trimmed || null);
  }, [value, onDone]);

  const cancel = useCallback(() => {
    onDone(null);
  }, [onDone]);

  const handleRegenerate = useCallback(() => {
    if (!modelPlan || isRegenerating) return;
    setRegenerating(true);
    setRegenError(null);
    void threadRegenerateTitle(threadId, modelPlan)
      .then((title) => {
        setValue(title);
      })
      .catch((error) => {
        const message = getInvokeErrorMessage(error, "Failed to regenerate title");
        console.warn("[thread] failed to regenerate title:", message);
        setRegenError(t("sidebar.regenerateTitleFailed"));
      })
      .finally(() => {
        setRegenerating(false);
        refocusAfterRegenerate();
      });
  }, [threadId, modelPlan, isRegenerating, refocusAfterRegenerate, t]);

  return (
    <div
      className={cn(
        `${DRAWER_LIST_ROW_CLASS} border pr-1.5`,
        isActive
          ? "border-app-border-strong bg-app-surface-active text-app-foreground"
          : "border-transparent bg-transparent text-app-muted",
      )}
    >
      <div className="flex min-w-0 flex-1 items-center gap-1">
        <ThreadStatusIndicator
          status={status}
          emphasis={isActive ? "default" : "subtle"}
        />
        <input
          ref={inputRef}
          autoFocus
          aria-label={t("sidebar.renameThread")}
          className="min-w-0 flex-1 truncate border-none bg-transparent text-[13px] leading-tight text-app-foreground outline-none placeholder:text-app-muted"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              if (!isRegenerating) save();
            } else if (e.key === "Escape") {
              e.preventDefault();
              cancel();
            }
          }}
          onBlur={(e) => {
            // Ignore blur when clicking the regenerate button
            // or while regeneration is in progress.
            if (isRegenerating) return;
            const related = e.relatedTarget as HTMLElement | null;
            if (related?.dataset.threadRegenerateBtn === "true") return;
            save();
          }}
          onFocus={(e) => e.target.select()}
        />
        <button
          type="button"
          data-thread-regenerate-btn="true"
          title={
            regenError
              ? regenError
              : modelPlan
                ? t("sidebar.regenerateTitle")
                : t("sidebar.noLiteModel")
          }
          disabled={!modelPlan || isRegenerating}
          className="flex size-6 shrink-0 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground disabled:cursor-not-allowed disabled:opacity-40"
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => {
            e.stopPropagation();
            handleRegenerate();
          }}
        >
          <Sparkles
            className={cn(
              "size-3.5",
              regenError && "text-red-500",
              isRegenerating && "animate-spin",
            )}
          />
        </button>
      </div>
    </div>
  );
}
