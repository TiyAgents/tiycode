"use client";

import { CheckCircle2Icon, ChevronRightIcon, CircleIcon, Loader2Icon, XCircleIcon } from "lucide-react";
import { ComponentProps, useCallback, useState } from "react";
import { cn } from "@/shared/lib/utils";
import type { TaskBoardDto, TaskItemDto, TaskStage } from "@/shared/types/api";
import { computeProgress, getActiveTask, hasFailedTasks, isBoardCompleted } from "../model/task-board";

// ---------------------------------------------------------------------------
// Task Stage Indicator
// ---------------------------------------------------------------------------

export type TaskStageIndicatorProps = ComponentProps<"span"> & {
  stage: TaskStage;
};

export const TaskStageIndicator = ({
  stage,
  className,
  ...props
}: TaskStageIndicatorProps) => {
  const icon =
    stage === "completed" ? (
      <CheckCircle2Icon className="size-4 text-success" />
    ) : stage === "in_progress" ? (
      <Loader2Icon className="size-4 animate-spin text-primary" />
    ) : stage === "failed" ? (
      <XCircleIcon className="size-4 text-destructive" />
    ) : (
      <CircleIcon className="size-4 text-muted-foreground/50" />
    );

  return (
    <span className={cn("flex-shrink-0", className)} {...props}>
      {icon}
    </span>
  );
};

// ---------------------------------------------------------------------------
// Task Item Row
// ---------------------------------------------------------------------------

export type TaskItemRowProps = ComponentProps<"li"> & {
  task: TaskItemDto;
  isActive?: boolean;
};

export const TaskItemRow = ({
  task,
  isActive = false,
  className,
  ...props
}: TaskItemRowProps) => {
  return (
    <li
      className={cn(
        "flex items-start gap-2 py-1.5 text-sm",
        isActive && "rounded-md bg-primary/5 px-2 -mx-2",
        className
      )}
      {...props}
    >
      <TaskStageIndicator stage={task.stage} />
      <span
        className={cn(
          "min-w-0 flex-1 break-words",
          task.stage === "completed" && "text-muted-foreground/60 line-through",
          task.stage === "failed" && "text-destructive",
          task.stage === "pending" && "text-muted-foreground"
        )}
      >
        {task.description}
        {task.errorDetail && (
          <span className="mt-0.5 block text-xs text-destructive/80">
            {task.errorDetail}
          </span>
        )}
      </span>
    </li>
  );
};

// ---------------------------------------------------------------------------
// Task Board Card
// ---------------------------------------------------------------------------

export type TaskBoardCardProps = ComponentProps<"div"> & {
  board: TaskBoardDto;
  showTitle?: boolean;
  defaultCollapsed?: boolean;
  variant?: "default" | "composer";
  compactThreshold?: number;
};

export const TaskBoardCard = ({
  board,
  showTitle = true,
  defaultCollapsed = false,
  variant = "default",
  compactThreshold = 6,
  className,
  ...props
}: TaskBoardCardProps) => {
  const [collapsed, setCollapsed] = useState(defaultCollapsed);
  const progress = computeProgress(board);
  const completed = isBoardCompleted(board);
  const hasFailed = hasFailedTasks(board);
  const isComposerVariant = variant === "composer";
  const totalCount = board.tasks.length;
  const completedCount = board.tasks.filter((task: TaskItemDto) => task.stage === "completed").length;
  const failedCount = board.tasks.filter((task: TaskItemDto) => task.stage === "failed").length;
  const activeTask = board.tasks.find((task: TaskItemDto) => task.id === board.activeTaskId) ?? getActiveTask(board);
  const isLongList = totalCount > compactThreshold;
  const showSummary = isComposerVariant || isLongList || collapsed;
  const taskCountSummary = `${completedCount}/${totalCount} completed`;

  const toggleCollapse = useCallback(() => setCollapsed((c) => !c), []);

  return (
    <div
      className={cn(
        "rounded-xl border border-app-border/40 bg-app-surface/40 p-3",
        isComposerVariant && "flex max-h-[min(32vh,280px)] min-h-0 flex-col",
        completed && "border-success/30 bg-success/5",
        hasFailed && "border-destructive/30 bg-destructive/5",
        className
      )}
      {...props}
    >
      {showTitle && (
        <button
          type="button"
          onClick={toggleCollapse}
          className="mb-3 flex w-full shrink-0 items-center justify-between gap-2 text-left"
        >
          <div className="flex min-w-0 items-center gap-2">
            <ChevronRightIcon
              className={cn(
                "size-3.5 shrink-0 text-muted-foreground transition-transform duration-200",
                !collapsed && "rotate-90"
              )}
            />
            <h4 className="truncate text-sm font-medium">{board.title}</h4>
          </div>
          <span
            className={cn(
              "shrink-0 text-xs tabular-nums",
              completed && "text-success",
              hasFailed && "text-destructive",
              !completed && !hasFailed && "text-muted-foreground"
            )}
          >
            {progress}%
          </span>
        </button>
      )}

      {/* Progress bar */}
      <div className="mb-3 h-1.5 w-full shrink-0 overflow-hidden rounded-full bg-muted">
        <div
          className={cn(
            "h-full rounded-full transition-all duration-300",
            completed && "bg-success",
            hasFailed && "bg-destructive",
            !completed && !hasFailed && "bg-primary"
          )}
          style={{ width: `${progress}%` }}
        />
      </div>

      {showSummary && (
        <div className="mb-2 flex shrink-0 items-center gap-3 text-xs text-muted-foreground">
          {activeTask && (
            <div className="min-w-0 flex-1 truncate">
              Current: <span className="text-foreground/80">{activeTask.description}</span>
            </div>
          )}
          <div className="ml-auto flex shrink-0 items-center gap-2 whitespace-nowrap tabular-nums">
            <span>{taskCountSummary}</span>
            {failedCount > 0 && (
              <span className="text-destructive">
                {failedCount} failed
              </span>
            )}
          </div>
        </div>
      )}

      {/* Task list */}
      {!collapsed && (
        <div
          className={cn(
            "min-h-0",
            isComposerVariant && "overflow-y-auto overscroll-contain pr-1"
          )}
        >
          <ul className="space-y-0.5">
            {board.tasks.map((task: TaskItemDto) => (
              <TaskItemRow
                key={task.id}
                task={task}
                isActive={task.id === board.activeTaskId}
              />
            ))}
          </ul>
        </div>
      )}

      {/* Status badge */}
      {!collapsed && board.status !== "active" && (
        <div className="mt-2 shrink-0 text-xs text-muted-foreground">
          {board.status === "completed" ? "✓ Completed" : "⊘ Abandoned"}
        </div>
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Compact Task Board Summary
// ---------------------------------------------------------------------------

export type TaskBoardSummaryProps = ComponentProps<"div"> & {
  board: TaskBoardDto;
};

export const TaskBoardSummary = ({
  board,
  className,
  ...props
}: TaskBoardSummaryProps) => {
  const progress = computeProgress(board);
  const completed = isBoardCompleted(board);
  const hasFailed = hasFailedTasks(board);
  const activeTask = board.tasks.find((t) => t.id === board.activeTaskId);

  return (
    <div
      className={cn(
        "flex items-center gap-3 rounded-lg border border-app-border/30 bg-app-surface/30 px-3 py-2 text-sm",
        className
      )}
      {...props}
    >
      <div className="flex-1">
        <div className="flex items-center gap-2">
          <span className="font-medium">{board.title}</span>
          <span className="text-xs text-muted-foreground">
            {board.tasks.filter((t: TaskItemDto) => t.stage === "completed").length}/
            {board.tasks.length}
          </span>
        </div>
        {activeTask && (
          <div className="mt-0.5 text-xs text-muted-foreground">
            {activeTask.description}
          </div>
        )}
      </div>

      {/* Mini progress bar */}
      <div className="h-1.5 w-16 overflow-hidden rounded-full bg-muted">
        <div
          className={cn(
            "h-full rounded-full",
            completed && "bg-success",
            hasFailed && "bg-destructive",
            !completed && !hasFailed && "bg-primary"
          )}
          style={{ width: `${progress}%` }}
        />
      </div>
    </div>
  );
};
