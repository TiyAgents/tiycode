import { describe, expect, it } from "vitest";
import { resolveGoalStatusKey } from "./goal-status-bar";

const source = await import("./goal-status-bar?raw").then((module) => module.default as string);

describe("GoalStatusBar layout contract", () => {
  it("keeps the objective as the flexible truncated area and reserves metric space", () => {
    expect(source).toContain("flex items-center gap-3 px-6 py-1.5");
    expect(source).toContain("relative overflow-hidden");
    expect(source).toContain("flex min-w-0 flex-1 items-center gap-2");
    expect(source).toContain("min-w-0 flex-1 truncate text-foreground/80");
    expect(source).toContain("flex shrink-0 items-center gap-3 whitespace-nowrap");
    expect(source).toContain("shrink-0 whitespace-nowrap tabular-nums text-muted-foreground");
  });

  it("does not render a per-goal elapsed timer (thread header owns that surface)", () => {
    // The previous refactor deliberately removed the cumulative timer from
    // this bar; the workbench header is now the single owner of per-thread
    // elapsed time. Guard against accidentally re-introducing it.
    expect(source).not.toContain("useThreadElapsedTimer");
    expect(source).not.toContain("goal.time.elapsed");
    expect(source).not.toContain("goal.time.hoursMinutes");
  });
});

describe("resolveGoalStatusKey", () => {
  it("maps non-complete statuses to their own keys", () => {
    expect(resolveGoalStatusKey("active", undefined)).toBe("goal.status.active");
    expect(resolveGoalStatusKey("paused", undefined)).toBe("goal.status.paused");
    expect(resolveGoalStatusKey("budget_limited", undefined)).toBe("goal.status.budgetLimited");
  });

  it("shows the verified label only when a complete goal passed Judge acceptance", () => {
    expect(resolveGoalStatusKey("complete", true)).toBe("goal.status.verified");
  });

  it("falls back to the plain complete label when judge has not passed", () => {
    expect(resolveGoalStatusKey("complete", false)).toBe("goal.status.complete");
  });

  it("treats an undefined judgePassed as not verified", () => {
    expect(resolveGoalStatusKey("complete", undefined)).toBe("goal.status.complete");
  });
});
