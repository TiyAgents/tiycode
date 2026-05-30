"use client";

import { useCallback, useState, useEffect, useRef } from "react";
import { goalGetState, goalPause, goalResume, goalClear } from "@/services/bridge/agent-commands";
import { threadStore, useStore, shallowEqual } from "@/modules/workbench-shell/model/thread-store";
import { useT } from "@/i18n";

type Props = {
  threadId: string;
};

function formatDuration(t: ReturnType<typeof useT>, totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return t("goal.time.hoursMinutes", { hours, minutes });
  } else if (minutes > 0) {
    return t("goal.time.minutesSeconds", { minutes, seconds });
  } else {
    return t("goal.time.seconds", { seconds });
  }
}

export function GoalStatusBar({ threadId }: Props) {
  const t = useT();
  const goal = useStore(threadStore, (s) => s.goalState[threadId] ?? null, shallowEqual);
  const threadStatus = useStore(
    threadStore,
    (s) => s.threadStatuses[threadId],
    shallowEqual,
  );
  const [loading, setLoading] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const tickStartRef = useRef(Date.now());

  const isRunning = threadStatus?.status === "running";

  // Real-time timer: tick every second while the goal is active and the thread is running
  useEffect(() => {
    if (isRunning && goal?.status === "active") {
      tickStartRef.current = Date.now();
      setElapsed(0);
      const interval = setInterval(() => {
        setElapsed(Math.floor((Date.now() - tickStartRef.current) / 1000));
      }, 1000);
      return () => clearInterval(interval);
    } else {
      setElapsed(0);
    }
  }, [isRunning, goal?.status]);

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

  const totalSeconds = (goal.timeUsedSeconds ?? 0) +
    (isRunning && goal.status === "active" ? elapsed : 0);
  const timeDisplay = formatDuration(t, totalSeconds);

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
  const shouldShowTimer = goal.status === "complete" || totalSeconds > 0;

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
      <span className="font-medium text-muted-foreground">{t(statusKey)}</span>

      {/* Objective — truncated */}
      <span className="truncate max-w-md text-foreground/80" title={goal.objective}>
        {goal.objective}
      </span>

      {/* Timer */}
      {shouldShowTimer && (
        <span className="text-muted-foreground whitespace-nowrap ml-2">
          {t("goal.time.elapsed", { time: timeDisplay })}
        </span>
      )}

      {/* Progress */}
      <span className="text-muted-foreground ml-auto tabular-nums">
        {displayTurnCount}/{goal.maxTurns} max turns
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
  );
}
