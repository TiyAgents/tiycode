import { useState, useEffect } from "react";

import type { ThreadRunStatus } from "@/modules/workbench-shell/model/types";

/* ------------------------------------------------------------------ */
/*  Pure helpers                                                       */
/* ------------------------------------------------------------------ */

export function isThreadTimerRunning(
  status: ThreadRunStatus | undefined,
): boolean {
  return status === "running";
}

export type TimerTransitionInput = {
  isTimerRunning: boolean;
  previousElapsedSeconds: number;
  previousBaseElapsedSeconds: number;
  previousStartedAtMs: number | null;
  nowMs: number;
};

export type TimerTransition = {
  elapsedSeconds: number;
  baseElapsedSeconds: number;
  startedAtMs: number | null;
};

/**
 * Compute the next timer state.
 *
 * When `isTimerRunning` is false the elapsed value is frozen and
 * `startedAtMs` is cleared so the next running cycle resumes from the
 * frozen value.
 */
export function computeTimerTransition({
  isTimerRunning,
  previousElapsedSeconds,
  previousBaseElapsedSeconds,
  previousStartedAtMs,
  nowMs,
}: TimerTransitionInput): TimerTransition {
  if (!isTimerRunning) {
    return {
      elapsedSeconds: previousElapsedSeconds,
      baseElapsedSeconds: previousElapsedSeconds,
      startedAtMs: null,
    };
  }

  const startedAtMs = previousStartedAtMs ?? nowMs;
  const baseElapsedSeconds =
    previousStartedAtMs === null
      ? previousElapsedSeconds
      : previousBaseElapsedSeconds;
  const elapsedSeconds =
    baseElapsedSeconds + Math.floor((nowMs - startedAtMs) / 1000);

  return { elapsedSeconds, baseElapsedSeconds, startedAtMs };
}

/**
 * Format seconds into a compact human-readable string.
 *
 * Examples: `"12s"`, `"3m 45s"`, `"1h 2m"`.
 */
export function formatElapsedTime(totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  } else if (minutes > 0) {
    return `${minutes}m ${seconds}s`;
  }
  return `${seconds}s`;
}

/* ------------------------------------------------------------------ */
/*  Module-level timer slots (survive mount / unmount cycles)          */
/* ------------------------------------------------------------------ */

type ThreadTimerSlot = {
  elapsed: number;
  baseElapsed: number;
  startedAt: number | null;
  /** Whether the slot has been seeded from a backend value already. */
  seeded: boolean;
};

const timerSlots = new Map<string, ThreadTimerSlot>();

/**
 * Get (or create) the timer slot for a thread.
 *
 * On first access the slot is seeded from `backendElapsedSeconds` (the
 * cumulative active running seconds persisted in the DB) so the timer
 * survives application restarts.
 */
function getSlot(tid: string, backendElapsedSeconds: number | null | undefined): ThreadTimerSlot {
  let slot = timerSlots.get(tid);
  if (!slot) {
    const seed = (backendElapsedSeconds != null && backendElapsedSeconds > 0)
      ? backendElapsedSeconds
      : 0;
    slot = { elapsed: seed, baseElapsed: seed, startedAt: null, seeded: seed > 0 };
    timerSlots.set(tid, slot);
  } else if (!slot.seeded && backendElapsedSeconds != null && backendElapsedSeconds > 0 && slot.elapsed === 0) {
    // Late seed: the store value arrived after the slot was first created
    // with zero (e.g. hook rendered before sidebar sync finished).
    slot.elapsed = backendElapsedSeconds;
    slot.baseElapsed = backendElapsedSeconds;
    slot.seeded = true;
  }
  return slot;
}

/**
 * Drop the cached slot for a thread.
 *
 * Call when a thread is deleted so a recycled id starts from zero.
 * Safe to call multiple times or for unknown ids.
 */
export function clearTimerSlot(threadId: string): void {
  timerSlots.delete(threadId);
}

/* ------------------------------------------------------------------ */
/*  Hook                                                               */
/* ------------------------------------------------------------------ */

export interface ThreadElapsedTimerResult {
  elapsedSeconds: number;
  isRunning: boolean;
  formattedTime: string;
}

/**
 * Track cumulative running time for a thread.
 *
 * Counts while `threadRunStatus === "running"` and **freezes** on any
 * other status (waiting_approval, needs_reply, …).  When running resumes
 * the timer continues from the frozen value — waiting time is never
 * counted.
 *
 * On first mount the slot is seeded from `backendElapsedSeconds`
 * (populated via the sidebar snapshot from `thread_runs.elapsed_running_secs`)
 * so the timer survives application restarts.
 */
export function useThreadElapsedTimer(
  threadId: string | undefined,
  threadRunStatus: ThreadRunStatus | undefined,
  backendElapsedSeconds?: number | null,
): ThreadElapsedTimerResult {
  const [, setTick] = useState(0);

  const isRunning = isThreadTimerRunning(threadRunStatus);
  const slot = threadId ? getSlot(threadId, backendElapsedSeconds) : undefined;

  useEffect(() => {
    if (!slot || !threadId) return;

    const syncElapsed = (nowMs: number) => {
      const next = computeTimerTransition({
        isTimerRunning: isRunning,
        previousElapsedSeconds: slot.elapsed,
        previousBaseElapsedSeconds: slot.baseElapsed,
        previousStartedAtMs: slot.startedAt,
        nowMs,
      });
      slot.elapsed = next.elapsedSeconds;
      slot.baseElapsed = next.baseElapsedSeconds;
      slot.startedAt = next.startedAtMs;
      setTick((t) => t + 1);
    };

    // Synchronise once immediately (handles freeze / resume transitions).
    syncElapsed(Date.now());

    if (!isRunning) return;

    const interval = setInterval(() => {
      syncElapsed(Date.now());
    }, 1000);
    return () => clearInterval(interval);
  }, [isRunning, slot, threadId]);

  const elapsedSeconds = slot?.elapsed ?? 0;

  return {
    elapsedSeconds,
    isRunning,
    formattedTime: formatElapsedTime(elapsedSeconds),
  };
}
