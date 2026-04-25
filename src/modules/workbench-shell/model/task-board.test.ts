import { describe, expect, it } from "vitest";
import type { TaskBoardDto, TaskItemDto, TaskStage } from "@/shared/types/api";
import {
  applyTaskBoardUpdate,
  computeProgress,
  getActiveTask,
  getNextPendingTask,
  getStageIconVariant,
  hasFailedTasks,
  initialTaskBoardState,
  isBoardCompleted,
  taskBoardsFromSnapshot,
} from "./task-board";

function task(id: string, stage: TaskStage, sortOrder = 0): TaskItemDto {
  return {
    id,
    taskBoardId: "board-1",
    description: `Task ${id}`,
    stage,
    sortOrder,
    errorDetail: stage === "failed" ? "boom" : null,
    createdAt: "2026-04-25T00:00:00Z",
    updatedAt: "2026-04-25T00:00:00Z",
  };
}

function board(overrides: Partial<TaskBoardDto> = {}): TaskBoardDto {
  return {
    id: "board-1",
    threadId: "thread-1",
    title: "Plan",
    status: "active",
    activeTaskId: "task-2",
    tasks: [task("task-1", "completed"), task("task-2", "in_progress", 1)],
    createdAt: "2026-04-25T00:00:00Z",
    updatedAt: "2026-04-25T00:00:00Z",
    ...overrides,
  };
}

describe("task board derived state", () => {
  it("computes progress and completion status", () => {
    expect(computeProgress(board())).toBe(50);
    expect(computeProgress(board({ tasks: [] }))).toBe(0);
    expect(isBoardCompleted(board({ status: "completed" }))).toBe(true);
    expect(isBoardCompleted(board({ tasks: [task("a", "completed"), task("b", "completed")] }))).toBe(true);
    expect(isBoardCompleted(board())).toBe(false);
  });

  it("finds failed, active, and next pending tasks", () => {
    const sample = board({
      tasks: [task("a", "completed"), task("b", "failed"), task("c", "pending"), task("d", "in_progress")],
    });

    expect(hasFailedTasks(sample)).toBe(true);
    expect(getActiveTask(sample)?.id).toBe("d");
    expect(getNextPendingTask(sample)?.id).toBe("c");
  });

  it("maps stages to icon variants", () => {
    expect(getStageIconVariant("pending")).toBe("pending");
    expect(getStageIconVariant("in_progress")).toBe("running");
    expect(getStageIconVariant("completed")).toBe("success");
    expect(getStageIconVariant("failed")).toBe("error");
  });

  it("adds and replaces boards while sorting by creation time", () => {
    const later = board({ id: "later", createdAt: "2026-04-25T02:00:00Z" });
    const earlier = board({ id: "earlier", createdAt: "2026-04-25T01:00:00Z", status: "completed" });
    const withLater = applyTaskBoardUpdate(initialTaskBoardState, later);
    const withBoth = applyTaskBoardUpdate(withLater, earlier);

    expect(withBoth.boards.map((entry) => entry.id)).toEqual(["earlier", "later"]);
    expect(withBoth.activeBoard?.id).toBe("later");

    const completedLater = { ...later, status: "completed" as const };
    expect(applyTaskBoardUpdate(withBoth, completedLater).activeBoard).toBeNull();
  });

  it("builds sorted state from a snapshot and selects active board", () => {
    const first = board({ id: "first", createdAt: "2026-04-25T01:00:00Z" });
    const second = board({ id: "second", createdAt: "2026-04-25T02:00:00Z" });

    expect(taskBoardsFromSnapshot([second, first], "second")).toEqual({
      activeBoard: second,
      boards: [first, second],
    });
    expect(taskBoardsFromSnapshot([first], "missing").activeBoard).toBeNull();
  });
});
