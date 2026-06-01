import { describe, expect, it } from "vitest";

import {
  computeGoalTimerTransition,
  isGoalTimerRunning,
} from "./goal-status-bar";

const source = await import("./goal-status-bar?raw").then((module) => module.default as string);

describe("GoalStatusBar timer helpers", () => {
  it("runs only while the thread is running and the goal is active", () => {
    expect(isGoalTimerRunning("running", "active")).toBe(true);
    expect(isGoalTimerRunning("waiting_approval", "active")).toBe(false);
    expect(isGoalTimerRunning("needs_reply", "active")).toBe(false);
    expect(isGoalTimerRunning("running", "paused")).toBe(false);
  });

  it("freezes while waiting for user action and resumes from the frozen elapsed value", () => {
    const started = computeGoalTimerTransition({
      isTimerRunning: true,
      previousElapsedSeconds: 0,
      previousBaseElapsedSeconds: 0,
      previousStartedAtMs: null,
      nowMs: 1_000,
    });
    expect(started).toEqual({
      elapsedSeconds: 0,
      baseElapsedSeconds: 0,
      startedAtMs: 1_000,
    });

    const advanced = computeGoalTimerTransition({
      isTimerRunning: true,
      previousElapsedSeconds: started.elapsedSeconds,
      previousBaseElapsedSeconds: started.baseElapsedSeconds,
      previousStartedAtMs: started.startedAtMs,
      nowMs: 3_400,
    });
    expect(advanced.elapsedSeconds).toBe(2);

    const frozen = computeGoalTimerTransition({
      isTimerRunning: false,
      previousElapsedSeconds: advanced.elapsedSeconds,
      previousBaseElapsedSeconds: advanced.baseElapsedSeconds,
      previousStartedAtMs: advanced.startedAtMs,
      nowMs: 10_000,
    });
    expect(frozen).toEqual({
      elapsedSeconds: 2,
      baseElapsedSeconds: 2,
      startedAtMs: null,
    });

    const resumed = computeGoalTimerTransition({
      isTimerRunning: true,
      previousElapsedSeconds: frozen.elapsedSeconds,
      previousBaseElapsedSeconds: frozen.baseElapsedSeconds,
      previousStartedAtMs: frozen.startedAtMs,
      nowMs: 10_000,
    });
    expect(resumed).toEqual({
      elapsedSeconds: 2,
      baseElapsedSeconds: 2,
      startedAtMs: 10_000,
    });

    const resumedAdvanced = computeGoalTimerTransition({
      isTimerRunning: true,
      previousElapsedSeconds: resumed.elapsedSeconds,
      previousBaseElapsedSeconds: resumed.baseElapsedSeconds,
      previousStartedAtMs: resumed.startedAtMs,
      nowMs: 12_100,
    });
    expect(resumedAdvanced.elapsedSeconds).toBe(4);
  });
});

describe("GoalStatusBar layout contract", () => {
  it("keeps the objective as the flexible truncated area and reserves metric space", () => {
    expect(source).toContain("flex items-center gap-3 px-6 py-1.5");
    expect(source).toContain("relative overflow-hidden");
    expect(source).toContain("flex min-w-0 flex-1 items-center gap-2");
    expect(source).toContain("min-w-0 flex-1 truncate text-foreground/80");
    expect(source).toContain("flex shrink-0 items-center gap-3 whitespace-nowrap");
    expect(source).toContain("shrink-0 whitespace-nowrap tabular-nums text-muted-foreground");
  });
});
