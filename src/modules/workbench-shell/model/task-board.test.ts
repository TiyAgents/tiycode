import { describe, it, expect } from "vitest";
import {
  computeProgress,
  isBoardCompleted,
  hasFailedTasks,
  getActiveTask,
  getNextPendingTask,
  getStageIconVariant,
  applyTaskBoardUpdate,
  taskBoardsFromSnapshot,
  initialTaskBoardState,
} from "@/modules/workbench-shell/model/task-board";
import type { TaskBoardDto, TaskItemDto } from "@/shared/types/api";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeTask(overrides: Partial<TaskItemDto> = {}): TaskItemDto {
  return {
    id: "task-1",
    taskBoardId: "board-1",
    description: "Test task",
    stage: "pending",
    sortOrder: 0,
    errorDetail: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function makeBoard(overrides: Partial<TaskBoardDto> = {}): TaskBoardDto {
  return {
    id: "board-1",
    threadId: "thread-1",
    title: "Test Board",
    status: "active",
    activeTaskId: null,
    tasks: [],
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// computeProgress
// ---------------------------------------------------------------------------

describe("computeProgress", () => {
  it("returns 0 for empty tasks", () => {
    expect(computeProgress(makeBoard({ tasks: [] }))).toBe(0);
  });

  it("returns 0 when no tasks are completed", () => {
    const board = makeBoard({
      tasks: [makeTask({ stage: "pending" }), makeTask({ id: "t2", stage: "in_progress" })],
    });
    expect(computeProgress(board)).toBe(0);
  });

  it("returns 100 when all tasks are completed", () => {
    const board = makeBoard({
      tasks: [makeTask({ stage: "completed" }), makeTask({ id: "t2", stage: "completed" })],
    });
    expect(computeProgress(board)).toBe(100);
  });

  it("returns rounded percentage for partial completion", () => {
    const board = makeBoard({
      tasks: [
        makeTask({ stage: "completed" }),
        makeTask({ id: "t2", stage: "in_progress" }),
        makeTask({ id: "t3", stage: "pending" }),
      ],
    });
    expect(computeProgress(board)).toBe(33);
  });
});

// ---------------------------------------------------------------------------
// isBoardCompleted
// ---------------------------------------------------------------------------

describe("isBoardCompleted", () => {
  it("returns true when status is completed", () => {
    const board = makeBoard({ status: "completed", tasks: [makeTask({ stage: "pending" })] });
    expect(isBoardCompleted(board)).toBe(true);
  });

  it("returns true when all tasks are completed", () => {
    const board = makeBoard({
      status: "active",
      tasks: [makeTask({ stage: "completed" }), makeTask({ id: "t2", stage: "completed" })],
    });
    expect(isBoardCompleted(board)).toBe(true);
  });

  it("returns false when some tasks are not completed", () => {
    const board = makeBoard({
      status: "active",
      tasks: [makeTask({ stage: "completed" }), makeTask({ id: "t2", stage: "pending" })],
    });
    expect(isBoardCompleted(board)).toBe(false);
  });

  it("returns true for empty tasks", () => {
    expect(isBoardCompleted(makeBoard({ tasks: [] }))).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// hasFailedTasks
// ---------------------------------------------------------------------------

describe("hasFailedTasks", () => {
  it("returns true when a task has failed", () => {
    const board = makeBoard({ tasks: [makeTask({ stage: "failed" })] });
    expect(hasFailedTasks(board)).toBe(true);
  });

  it("returns false when no tasks have failed", () => {
    const board = makeBoard({ tasks: [makeTask({ stage: "completed" })] });
    expect(hasFailedTasks(board)).toBe(false);
  });

  it("returns false for empty tasks", () => {
    expect(hasFailedTasks(makeBoard())).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// getActiveTask
// ---------------------------------------------------------------------------

describe("getActiveTask", () => {
  it("returns the in-progress task", () => {
    const active = makeTask({ id: "active", stage: "in_progress" });
    const board = makeBoard({ tasks: [makeTask({ stage: "completed" }), active] });
    expect(getActiveTask(board)).toEqual(active);
  });

  it("returns undefined when no in-progress task", () => {
    const board = makeBoard({ tasks: [makeTask({ stage: "pending" })] });
    expect(getActiveTask(board)).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// getNextPendingTask
// ---------------------------------------------------------------------------

describe("getNextPendingTask", () => {
  it("returns the first pending task", () => {
    const pending = makeTask({ id: "p1", stage: "pending" });
    const board = makeBoard({ tasks: [makeTask({ stage: "completed" }), pending] });
    expect(getNextPendingTask(board)).toEqual(pending);
  });

  it("returns undefined when no pending tasks", () => {
    const board = makeBoard({ tasks: [makeTask({ stage: "completed" })] });
    expect(getNextPendingTask(board)).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// getStageIconVariant
// ---------------------------------------------------------------------------

describe("getStageIconVariant", () => {
  it("maps pending to pending", () => {
    expect(getStageIconVariant("pending")).toBe("pending");
  });

  it("maps in_progress to running", () => {
    expect(getStageIconVariant("in_progress")).toBe("running");
  });

  it("maps completed to success", () => {
    expect(getStageIconVariant("completed")).toBe("success");
  });

  it("maps failed to error", () => {
    expect(getStageIconVariant("failed")).toBe("error");
  });
});

// ---------------------------------------------------------------------------
// initialTaskBoardState
// ---------------------------------------------------------------------------

describe("initialTaskBoardState", () => {
  it("has null activeBoard and empty boards", () => {
    expect(initialTaskBoardState.activeBoard).toBeNull();
    expect(initialTaskBoardState.boards).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// applyTaskBoardUpdate
// ---------------------------------------------------------------------------

describe("applyTaskBoardUpdate", () => {
  it("adds a new board to empty state", () => {
    const board = makeBoard({ id: "b1", status: "active" });
    const result = applyTaskBoardUpdate(initialTaskBoardState, board);
    expect(result.boards).toHaveLength(1);
    expect(result.activeBoard).toEqual(board);
  });

  it("updates an existing board", () => {
    const state = { activeBoard: null, boards: [makeBoard({ id: "b1", status: "active" })] };
    const updatedBoard = makeBoard({ id: "b1", status: "completed" });
    const result = applyTaskBoardUpdate(state, updatedBoard);
    expect(result.boards).toHaveLength(1);
    expect(result.boards[0].status).toBe("completed");
    expect(result.activeBoard).toBeNull();
  });

  it("sorts boards by createdAt ascending", () => {
    const older = makeBoard({ id: "b1", createdAt: "2026-01-01T00:00:00Z" });
    const newer = makeBoard({ id: "b2", createdAt: "2026-01-02T00:00:00Z" });
    const state = { activeBoard: null, boards: [newer] };
    const result = applyTaskBoardUpdate(state, older);
    expect(result.boards[0].id).toBe("b1");
    expect(result.boards[1].id).toBe("b2");
  });

  it("derives activeBoard from updated list", () => {
    const completed = makeBoard({ id: "b1", status: "completed" });
    const active = makeBoard({ id: "b2", status: "active", createdAt: "2026-01-02T00:00:00Z" });
    const state = { activeBoard: null, boards: [completed] };
    const result = applyTaskBoardUpdate(state, active);
    expect(result.activeBoard?.id).toBe("b2");
  });
});

// ---------------------------------------------------------------------------
// taskBoardsFromSnapshot
// ---------------------------------------------------------------------------

describe("taskBoardsFromSnapshot", () => {
  it("initializes with active board by ID", () => {
    const boards = [makeBoard({ id: "b1" }), makeBoard({ id: "b2", createdAt: "2026-01-02T00:00:00Z" })];
    const result = taskBoardsFromSnapshot(boards, "b2");
    expect(result.activeBoard?.id).toBe("b2");
    expect(result.boards).toHaveLength(2);
  });

  it("sets activeBoard to null when ID not found", () => {
    const boards = [makeBoard({ id: "b1" })];
    const result = taskBoardsFromSnapshot(boards, "nonexistent");
    expect(result.activeBoard).toBeNull();
  });

  it("sets activeBoard to null when ID is null", () => {
    const boards = [makeBoard({ id: "b1" })];
    const result = taskBoardsFromSnapshot(boards, null);
    expect(result.activeBoard).toBeNull();
  });

  it("sorts boards by createdAt", () => {
    const newer = makeBoard({ id: "b1", createdAt: "2026-06-01T00:00:00Z" });
    const older = makeBoard({ id: "b2", createdAt: "2026-01-01T00:00:00Z" });
    const result = taskBoardsFromSnapshot([newer, older], null);
    expect(result.boards[0].id).toBe("b2");
    expect(result.boards[1].id).toBe("b1");
  });
});
