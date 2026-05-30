"use client";

import { useCallback, useState } from "react";
import { goalGetState, goalPause, goalResume, goalClear } from "@/services/bridge/agent-commands";
import { threadStore, useStore, shallowEqual } from "@/modules/workbench-shell/model/thread-store";

type Props = {
  threadId: string;
};

export function GoalStatusBar({ threadId }: Props) {
  const goal = useStore(threadStore, (s) => s.goalState[threadId] ?? null, shallowEqual);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    // Re-fetch goal state from backend and sync to threadStore so the
    // status bar reflects the latest state after pause/resume/clear.
    try {
      const g = await goalGetState(threadId);
      threadStore.setState((prev) => ({
        goalState: { ...prev.goalState, [threadId]: g },
      }));
    } catch {
      // Goal not found or error — clear from store
      threadStore.setState((prev) => ({
        goalState: { ...prev.goalState, [threadId]: null },
      }));
    }
  }, [threadId]);

  if (!goal) return null;

  const statusLabel =
    goal.status === "active" ? "活跃"
    : goal.status === "paused" ? "已暂停"
    : goal.status === "budget_limited" ? "预算耗尽"
    : goal.status === "complete" ? "已完成"
    : goal.status;

  const statusColor =
    goal.status === "active" ? "bg-blue-500"
    : goal.status === "paused" ? "bg-yellow-500"
    : goal.status === "budget_limited" ? "bg-red-500"
    : "bg-green-500";

  const progress = goal.maxTurns > 0 ? Math.min((goal.turnsUsed / goal.maxTurns) * 100, 100) : 0;
  const progressBarWidth = `${progress}%`;

  return (
    <div className="flex items-center gap-2 px-6 py-1.5 text-xs border-b border-border/50 bg-muted/30 shrink-0 relative">
      {/* Progress bar background */}
      <div className="absolute inset-x-0 bottom-0 h-0.5 bg-muted/50">
        <div
          className="h-full bg-accent transition-all duration-300"
          style={{ width: progressBarWidth }}
        />
      </div>

      {/* Status dot + label */}
      <span className={`inline-block w-2 h-2 rounded-full ${statusColor}`} />
      <span className="font-medium text-muted-foreground">{statusLabel}</span>

      {/* Objective — truncated */}
      <span className="truncate max-w-md text-foreground/80" title={goal.objective}>
        {goal.objective}
      </span>

      {/* Progress */}
      <span className="text-muted-foreground ml-auto tabular-nums">
        {goal.turnsUsed}/{goal.maxTurns} turns
      </span>

      {/* Action buttons */}
      <span className="flex gap-1 ml-2">
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
            title="暂停目标"
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
            title="恢复目标"
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
            title="清除目标"
          >
            ✕
          </button>
        )}
      </span>
    </div>
  );
}
