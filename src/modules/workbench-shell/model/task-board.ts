/**
 * Task board state management for runtime thread surface.
 */

import type { TaskBoardDto, TaskItemDto, TaskStage } from "@/shared/types/api";

export type { TaskBoardDto, TaskItemDto, TaskStage };

/**
 * Compute completion percentage for a task board.
 */
export function computeProgress(board: TaskBoardDto): number {
  if (board.tasks.length === 0) return 0;
  const completed = board.tasks.filter((t) => t.stage === "completed").length;
  return Math.round((completed / board.tasks.length) * 100);
}

/**
 * Check if a task board is fully completed.
 */
export function isBoardCompleted(board: TaskBoardDto): boolean {
  return board.status === "completed" || board.tasks.every((t) => t.stage === "completed");
}

/**
 * Check if a task board has any failed tasks.
 */
export function hasFailedTasks(board: TaskBoardDto): boolean {
  return board.tasks.some((t) => t.stage === "failed");
}

/**
 * Get active (in-progress) task, if any.
 */
export function getActiveTask(board: TaskBoardDto): TaskItemDto | undefined {
  return board.tasks.find((t) => t.stage === "in_progress");
}

/**
 * Get the next pending task after the current active task.
 */
export function getNextPendingTask(board: TaskBoardDto): TaskItemDto | undefined {
  return board.tasks.find((t) => t.stage === "pending");
}

/**
 * Stage status for rendering.
 */
export type StageStatus = "pending" | "in_progress" | "completed" | "failed";

/**
 * Get status icon variant for a task stage.
 */
export function getStageIconVariant(stage: TaskStage): "pending" | "running" | "success" | "error" {
  switch (stage) {
    case "pending":
      return "pending";
    case "in_progress":
      return "running";
    case "completed":
      return "success";
    case "failed":
      return "error";
  }
}

/**
 * Task board state tracked by the runtime surface.
 */
export interface TaskBoardState {
  activeBoard: TaskBoardDto | null;
  boards: TaskBoardDto[];
}

/**
 * Initial empty task board state.
 */
export const initialTaskBoardState: TaskBoardState = {
  activeBoard: null,
  boards: [],
};

/**
 * Merge a task board update into state.
 */
export function applyTaskBoardUpdate(state: TaskBoardState, board: TaskBoardDto): TaskBoardState {
  const existingIndex = state.boards.findIndex((b) => b.id === board.id);
  let newBoards: TaskBoardDto[];

  if (existingIndex >= 0) {
    // Update existing board
    newBoards = [...state.boards];
    newBoards[existingIndex] = board;
  } else {
    // Add new board
    newBoards = [...state.boards, board];
  }

  // Sort by created_at ascending
  newBoards.sort((a, b) => new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime());

  // Derive active board from the updated list
  const activeBoard = newBoards.find((b) => b.status === "active") ?? null;

  return {
    activeBoard,
    boards: newBoards,
  };
}

/**
 * Initialize task board state from thread snapshot.
 */
export function taskBoardsFromSnapshot(
  boards: TaskBoardDto[],
  activeTaskBoardId: string | null
): TaskBoardState {
  const activeBoard = activeTaskBoardId ? boards.find((b) => b.id === activeTaskBoardId) ?? null : null;

  return {
    activeBoard,
    boards: [...boards].sort((a, b) => new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime()),
  };
}
