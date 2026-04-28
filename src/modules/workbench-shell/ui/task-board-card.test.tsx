import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { TaskBoardDto, TaskItemDto, TaskStage } from "@/shared/types/api";
import { TaskBoardCard } from "./task-board-card";

function task(id: string, stage: TaskStage, sortOrder: number): TaskItemDto {
  return {
    id,
    taskBoardId: "board-1",
    description: `Task ${sortOrder + 1}`,
    stage,
    sortOrder,
    errorDetail: stage === "failed" ? "boom" : null,
    createdAt: "2026-04-25T00:00:00Z",
    updatedAt: "2026-04-25T00:00:00Z",
  };
}

function board(overrides: Partial<TaskBoardDto> = {}): TaskBoardDto {
  const tasks = [
    task("task-1", "completed", 0),
    task("task-2", "completed", 1),
    task("task-3", "in_progress", 2),
    task("task-4", "pending", 3),
    task("task-5", "pending", 4),
    task("task-6", "failed", 5),
    task("task-7", "pending", 6),
    task("task-8", "pending", 7),
  ];

  return {
    id: "board-1",
    threadId: "thread-1",
    title: "Implementation plan",
    status: "active",
    activeTaskId: "task-3",
    tasks,
    createdAt: "2026-04-25T00:00:00Z",
    updatedAt: "2026-04-25T00:00:00Z",
    ...overrides,
  };
}

describe("TaskBoardCard", () => {
  it("constrains composer task boards and keeps the list internally scrollable", () => {
    const html = renderToStaticMarkup(<TaskBoardCard board={board()} variant="composer" />);

    expect(html).toContain("max-h-[min(32vh,280px)]");
    expect(html).toContain("min-h-0");
    expect(html).toContain("overflow-y-auto");
    expect(html).toContain("overscroll-contain");
    expect(html).toContain("2/8 completed");
    expect(html).toContain("1 failed");
    expect(html).toContain("Current:");
    expect(html).toContain("Task 3");
    expect(html).toContain("Task 8");
  });

  it("shows a long-list summary without enabling composer scroll classes by default", () => {
    const html = renderToStaticMarkup(<TaskBoardCard board={board()} />);

    expect(html).toContain("2/8 completed");
    expect(html).toContain("Current:");
    expect(html).not.toContain("max-h-[min(32vh,280px)]");
    expect(html).not.toContain("overflow-y-auto");
  });

  it("keeps short default boards close to the original full-list presentation", () => {
    const shortBoard = board({
      tasks: [task("task-1", "completed", 0), task("task-2", "in_progress", 1)],
      activeTaskId: "task-2",
    });
    const html = renderToStaticMarkup(<TaskBoardCard board={shortBoard} />);

    expect(html).toContain("Task 1");
    expect(html).toContain("Task 2");
    expect(html).not.toContain("completed</span>");
    expect(html).not.toContain("overflow-y-auto");
  });
});
