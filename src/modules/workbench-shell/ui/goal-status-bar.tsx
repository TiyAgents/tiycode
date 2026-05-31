"use client";

import { useCallback, useState, useEffect, useRef } from "react";
import { goalGetState, goalPause, goalResume, goalClear } from "@/services/bridge/agent-commands";
import { threadStore, useStore, shallowEqual } from "@/modules/workbench-shell/model/thread-store";
import type { ThreadRunStatus } from "@/modules/workbench-shell/model/types";
import { useT } from "@/i18n";

type Props = {
  threadId: string;
};

type GoalStatus = "active" | "paused" | "budget_limited" | "complete";

export type GoalTimerTransitionInput = {
  isTimerRunning: boolean;
  previousElapsedSeconds: number;
  previousBaseElapsedSeconds: number;
  previousStartedAtMs: number | null;
  nowMs: number;
};

export type GoalTimerTransition = {
  elapsedSeconds: number;
  baseElapsedSeconds: number;
  startedAtMs: number | null;
};

export function isGoalTimerRunning(
  threadStatus: ThreadRunStatus | undefined,
  goalStatus: GoalStatus | undefined,
): boolean {
  return threadStatus === "running" && goalStatus === "active";
}

export function computeGoalTimerTransition({
  isTimerRunning,
  previousElapsedSeconds,
  previousBaseElapsedSeconds,
  previousStartedAtMs,
  nowMs,
}: GoalTimerTransitionInput): GoalTimerTransition {
  if (!isTimerRunning) {
    return {
      elapsedSeconds: previousElapsedSeconds,
      baseElapsedSeconds: previousElapsedSeconds,
      startedAtMs: null,
    };
  }

  const startedAtMs = previousStartedAtMs ?? nowMs;
  const baseElapsedSeconds = previousStartedAtMs === null
    ? previousElapsedSeconds
    : previousBaseElapsedSeconds;
  const elapsedSeconds = baseElapsedSeconds + Math.floor((nowMs - startedAtMs) / 1000);

  return {
    elapsedSeconds,
    baseElapsedSeconds,
    startedAtMs,
  };
}

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
  const elapsedRef = useRef(0);
  const timerBaseElapsedRef = useRef(0);
  const timerStartedAtRef = useRef<number | null>(null);
  const goalIdentityRef = useRef<string | null>(null);
  const accountedSecondsRef = useRef<number | null>(null);
  const timerRunIdRef = useRef<string | null>(null);

  const isTimerRunning = isGoalTimerRunning(threadStatus?.status, goal?.status);

  useEffect(() => {
    const accountedSeconds = goal?.timeUsedSeconds ?? 0;
    const runId = threadStatus?.runId ?? null;
    const isProgressingThreadStatus = threadStatus?.status === "running"
      || threadStatus?.status === "waiting_approval"
      || threadStatus?.status === "needs_reply";
    const shouldReset = goal?.status !== "active"
      || goalIdentityRef.current !== (goal?.id ?? null)
      || accountedSecondsRef.current !== accountedSeconds
      || (isProgressingThreadStatus && runId !== null && timerRunIdRef.current !== runId);

    if (!shouldReset) return;

    goalIdentityRef.current = goal?.status === "active" ? goal.id : null;
    accountedSecondsRef.current = accountedSeconds;
    timerRunIdRef.current = isProgressingThreadStatus ? runId : null;
    elapsedRef.current = 0;
    timerBaseElapsedRef.current = 0;
    timerStartedAtRef.current = null;
    setElapsed(0);
  }, [goal?.id, goal?.status, goal?.timeUsedSeconds, threadStatus?.runId, threadStatus?.status]);

  // Real-time timer: tick only while the run is actively progressing.
  // User-action states such as waiting_approval / needs_reply freeze elapsed
  // locally, then running resumes from the frozen value.
  useEffect(() => {
    const syncElapsed = (nowMs: number) => {
      const next = computeGoalTimerTransition({
        isTimerRunning,
        previousElapsedSeconds: elapsedRef.current,
        previousBaseElapsedSeconds: timerBaseElapsedRef.current,
        previousStartedAtMs: timerStartedAtRef.current,
        nowMs,
      });
      elapsedRef.current = next.elapsedSeconds;
      timerBaseElapsedRef.current = next.baseElapsedSeconds;
      timerStartedAtRef.current = next.startedAtMs;
      setElapsed(next.elapsedSeconds);
    };

    syncElapsed(Date.now());

    if (!isTimerRunning) {
      return;
    }

    const interval = setInterval(() => {
      syncElapsed(Date.now());
    }, 1000);
    return () => clearInterval(interval);
  }, [isTimerRunning]);

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

  const totalSeconds = (goal.timeUsedSeconds ?? 0) + elapsed;
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
  const shouldShowTimer = goal.status === "active" || goal.status === "complete" || totalSeconds > 0;

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
        {/* Timer */}
        {shouldShowTimer && (
          <span className="shrink-0 whitespace-nowrap tabular-nums text-muted-foreground">
            {t("goal.time.elapsed", { time: timeDisplay })}
          </span>
        )}

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
