"use client";

import { useCallback, useState } from "react";
import { goalGetState, goalPause, goalResume, goalClear } from "@/services/bridge/agent-commands";
import { threadStore, useStore, shallowEqual } from "@/modules/workbench-shell/model/thread-store";
import { useT } from "@/i18n";

type Props = {
  threadId: string;
};

export function GoalStatusBar({ threadId }: Props) {
  const t = useT();
  const goal = useStore(threadStore, (s) => s.goalState[threadId] ?? null, shallowEqual);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    // Re-fetch goal state from backend and sync to threadStore.
    try {
      const g = await goalGetState(threadId);
      threadStore.setState((prev) => ({
        goalState: { ...prev.goalState, [threadId]: g },
      }));
    } catch {
      threadStore.setState((prev) => ({
        goalState: { ...prev.goalState, [threadId]: null },
      }));
    }
  }, [threadId]);

  if (!goal) return null;

  const statusKey = (() => {
    switch (goal.status) {
      case "active": return "goal.status.active";
      case "paused": return "goal.status.paused";
      case "budget_limited": return "goal.status.budgetLimited";
      case "complete": return "goal.status.complete";
      default: return "goal.status.active";
    }
  })();

  const statusColor =
    goal.status === "active" ? "bg-blue-500"
    : goal.status === "paused" ? "bg-yellow-500"
    : goal.status === "budget_limited" ? "bg-red-500"
    : "bg-green-500";

  const progress = goal.maxTurns > 0 ? Math.min((goal.turnsUsed / goal.maxTurns) * 100, 100) : 0;
  const progressBarWidth = `${progress}%`;
  const displayTurnCount = goal.maxTurns > 0
    ? Math.min(
        goal.status === "active" ? goal.turnsUsed + 1 : Math.max(goal.turnsUsed, 1),
        goal.maxTurns,
      )
    : Math.max(goal.turnsUsed, 1);

  return (
    <div className="flex items-center gap-3 px-6 py-1.5 text-xs border-b border-border/50 bg-muted/30 shrink-0 relative overflow-hidden">
      {/* Progress bar background */}
      <div className="absolute inset-x-0 bottom-0 h-0.5 bg-muted/50">
        <div
          className="h-full bg-accent transition-all duration-300"
          style={{ width: progressBarWidth }}
        />
      </div>

      <div className="flex min-w-0 flex-1 items-center gap-2">
        {/* Status dot + label */}
        <span className={`inline-block w-2 h-2 rounded-full shrink-0 ${statusColor}`} />
        <span className="font-medium text-muted-foreground shrink-0">{t(statusKey)}</span>

        {/* Objective — truncated */}
        <span className="min-w-0 flex-1 truncate text-foreground/80" title={goal.objective}>
          {goal.objective}
        </span>
      </div>

      <div className="flex shrink-0 items-center gap-3 whitespace-nowrap">
        {/* Progress */}
        <span className="shrink-0 whitespace-nowrap tabular-nums text-muted-foreground">
          {displayTurnCount}/{goal.maxTurns} max turns
        </span>

        {/* Action buttons */}
        <span className="flex shrink-0 gap-1">
          {goal.status === "active" && (
            <button
              className="px-1.5 py-0.5 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors disabled:opacity-50"
              disabled={loading}
              onClick={async () => {
                setLoading(true);
                try {
                  await goalPause(threadId);
                  await refresh();
                } finally {
                  setLoading(false);
                }
              }}
              title={t("goal.action.pause")}
            >
              ⏸
            </button>
          )}
          {goal.status === "paused" && (
            <button
              className="px-1.5 py-0.5 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors disabled:opacity-50"
              disabled={loading}
              onClick={async () => {
                setLoading(true);
                try {
                  await goalResume(threadId);
                  await refresh();
                } finally {
                  setLoading(false);
                }
              }}
              title={t("goal.action.resume")}
            >
              ▶
            </button>
          )}
          {(goal.status === "active" || goal.status === "paused") && (
            <button
              className="px-1.5 py-0.5 rounded hover:bg-muted text-muted-foreground hover:text-destructive transition-colors disabled:opacity-50"
              disabled={loading}
              onClick={async () => {
                setLoading(true);
                try {
                  await goalClear(threadId);
                  threadStore.setState((prev) => ({
                    goalState: { ...prev.goalState, [threadId]: null },
                  }));
                } finally {
                  setLoading(false);
                }
              }}
              title={t("goal.action.clear")}
            >
              ✕
            </button>
          )}
        </span>
      </div>
    </div>
  );
}
