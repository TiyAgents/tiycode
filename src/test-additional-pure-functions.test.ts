import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));

// ── Pure functions from various modules ────────────────────────

import { messagesToMarkdown } from "@/components/ai-elements/conversation";
import { isOnboardingCompleted } from "@/modules/onboarding/model/use-onboarding";
import { computeProgress, isBoardCompleted, hasFailedTasks, getActiveTask, getNextPendingTask, getStageIconVariant } from "@/modules/workbench-shell/model/task-board";
import { countDiffLineChanges } from "@/modules/workbench-shell/model/file-mutation-presentation";
import { buildThreadTitle } from "@/modules/workbench-shell/model/helpers";
import { streamdownLinkSafety } from "@/shared/lib/streamdown-link-safety";
import { ONBOARDING_COMPLETED_KEY, ONBOARDING_STEPS } from "@/modules/onboarding/model/types";

// ── messagesToMarkdown ──────────────────────────────────────

describe("messagesToMarkdown", () => {
  it("returns empty string for empty messages", () => {
    expect(messagesToMarkdown([])).toBe("");
  });

  it("converts user message to text", () => {
    const result = messagesToMarkdown([
      { id: "1", role: "user", content: "Hello AI" },
    ] as any);
    expect(result).toContain("Hello AI");
  });

  it("converts assistant message to text", () => {
    const result = messagesToMarkdown([
      { id: "1", role: "assistant", content: "Hi there!" },
    ] as any);
    expect(result).toContain("Hi there!");
  });
});

// ── isOnboardingCompleted ──────────────────────────────────

describe("isOnboardingCompleted", () => {
  it("returns false when key not set", () => {
    // localStorage mock returns null for missing keys
    expect(isOnboardingCompleted()).toBe(false);
  });

  it("returns true when key is set", () => {
    localStorage.setItem(ONBOARDING_COMPLETED_KEY, "true");
    expect(isOnboardingCompleted()).toBe(true);
    localStorage.removeItem(ONBOARDING_COMPLETED_KEY);
  });

  it("returns false for non-true values", () => {
    localStorage.setItem(ONBOARDING_COMPLETED_KEY, "false");
    expect(isOnboardingCompleted()).toBe(false);
    localStorage.removeItem(ONBOARDING_COMPLETED_KEY);
  });
});

// ── ONBOARDING_STEPS constants ───────────────────────────────

describe("ONBOARDING_STEPS", () => {
  it("has at least one step", () => {
    expect(ONBOARDING_STEPS.length).toBeGreaterThan(0);
  });

  it("each step has an id and label", () => {
    for (const step of ONBOARDING_STEPS) {
      expect(step.id).toBeTruthy();
      expect(step.label).toBeTruthy();
    }
  });
});

// ── TaskBoard pure functions ────────────────────────────────

function makeTaskItem(overrides: Record<string, unknown> = {}): any {
  return {
    id: "task-1",
    title: "Test task",
    status: "pending" as const,
    stage: "todo" as const,
    ...overrides,
  };
}

function makeBoard(tasks: any[] = []): any {
  return {
    id: "board-1",
    title: "Test board",
    items: tasks,
    stages: [
      { id: "todo", title: "Todo", itemIds: [] },
      { id: "doing", title: "Doing", itemIds: [] },
      { id: "done", title: "Done", itemIds: [] },
    ],
  };
}

describe("computeProgress", () => {
  it("returns 0 for empty board", () => {
    expect(computeProgress(makeBoard())).toBe(0);
  });

  it("returns 100 when all tasks done", () => {
    const board = makeBoard([
      makeTaskItem({ status: "completed", stage: "done" }),
      makeTaskItem({ status: "completed", stage: "done" }),
    ]);
    // Need to put tasks in done stage
    board.stages[2].itemIds = ["task-1", "task-2"];
    expect(computeProgress(board)).toBe(100);
  });

  it("returns 50 when half done", () => {
    const board = makeBoard([
      makeTaskItem({ id: "t1", status: "completed", stage: "done" }),
      makeTaskItem({ id: "t2", status: "pending", stage: "todo" }),
    ]);
    board.stages[0].itemIds = ["t2"];
    board.stages[2].itemIds = ["t1"];
    const progress = computeProgress(board);
    expect(progress).toBeGreaterThanOrEqual(40);
    expect(progress).toBeLessThanOrEqual(60);
  });
});

describe("isBoardCompleted", () => {
  it("returns true when all tasks completed", () => {
    const board = makeBoard([
      makeTaskItem({ status: "completed" }),
    ]);
    board.stages[2].itemIds = ["task-1"];
    expect(isBoardCompleted(board)).toBe(true);
  });

  it("returns false when pending tasks exist", () => {
    const board = makeBoard([makeTaskItem()]);
    board.stages[0].itemIds = ["task-1"];
    expect(isBoardCompleted(board)).toBe(false);
  });
});

describe("hasFailedTasks", () => {
  it("returns true when a task failed", () => {
    const board = makeBoard([makeTaskItem({ status: "failed" })]);
    expect(hasFailedTasks(board)).toBe(true);
  });

  it("returns false when no failures", () => {
    const board = makeBoard([makeTaskItem({ status: "completed" })]);
    expect(hasFailedTasks(board)).toBe(false);
  });
});

describe("getActiveTask", () => {
  it("returns running task if exists", () => {
    const board = makeBoard([
      makeTaskItem({ id: "t1", status: "running" }),
      makeTaskItem({ id: "t2", status: "pending" }),
    ]);
    const active = getActiveTask(board);
    expect(active?.id).toBe("t1");
  });

  it("returns undefined when no running task", () => {
    const board = makeBoard([makeTaskItem()]);
    expect(getActiveTask(board)).toBeUndefined();
  });
});

describe("getNextPendingTask", () => {
  it("returns first pending task", () => {
    const board = makeBoard([
      makeTaskItem({ id: "t1", status: "completed" }),
      makeTaskItem({ id: "t2", status: "pending" }),
      makeTaskItem({ id: "t3", status: "pending" }),
    ]);
    const next = getNextPendingTask(board);
    expect(next?.id).toBe("t2");
  });

  it("returns undefined when no pending tasks", () => {
    const board = makeBoard([makeTaskItem({ status: "completed" })]);
    expect(getNextPendingTask(board)).toBeUndefined();
  });
});

describe("getStageIconVariant", () => {
  it("returns pending for pending stage", () => {
    expect(getStageIconVariant("pending")).toBe("pending");
  });

  it("returns running for in_progress stage", () => {
    expect(getStageIconVariant("in_progress")).toBe("running");
  });

  it("returns success for completed stage", () => {
    expect(getStageIconVariant("completed")).toBe("success");
  });

  it("returns error for failed stage", () => {
    expect(getStageIconVariant("failed")).toBe("error");
  });

  it("returns undefined for unknown stage", () => {
    const result = getStageIconVariant("todo" as any);
    expect(result).toBeUndefined();
  });
});

// ── countDiffLineChanges ────────────────────────────────────

describe("countDiffLineChanges", () => {
  it("counts added lines starting with +", () => {
    const diff = "+ line 1\n+ line 2\n- line 3\n  context";
    const result = countDiffLineChanges(diff);
    expect(result.linesAdded).toBe(2);
    expect(result.linesRemoved).toBe(1);
  });

  it("handles empty diff", () => {
    const result = countDiffLineChanges("");
    expect(result.linesAdded).toBe(0);
    expect(result.linesRemoved).toBe(0);
  });
});

// ── buildThreadTitle ─────────────────────────────────────────

describe("buildThreadTitle", () => {
  it("uses prompt directly as title", () => {
    expect(buildThreadTitle("Hello world")).toBe("Hello world");
  });

  it("truncates long prompts", () => {
    const long = "a".repeat(200);
    const title = buildThreadTitle(long);
    expect(title.length).toBeLessThan(200);
  });

  it("handles empty string", () => {
    expect(buildThreadTitle("")).toBe("");
  });
});

// ── streamdownLinkSafety ────────────────────────────────────

describe("streamdownLinkSafety", () => {
  it("has expected structure with boolean flags", () => {
    expect(typeof streamdownLinkSafety.enabled).toBe("boolean");
    expect(typeof streamdownLinkSafety.renderModal).toBe("function");
  });
});
