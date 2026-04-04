"use client";

import { CheckCircle2Icon, CircleIcon, Loader2Icon, XCircleIcon } from "lucide-react";
import type { ComponentProps } from "react";
import { cn } from "@/shared/lib/utils";
import type { TaskBoardDto, TaskItemDto, TaskStage } from "@/shared/types/api";
import { computeProgress, hasFailedTasks, isBoardCompleted } from "../model/task-board";

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
          "flex-1",
          task.stage === "completed" && "text-muted-foreground/60 line-through",
          task.stage === "failed" && "text-destructive",
          task.stage === "pending" && "text-muted-foreground"
        )}
      >
        {task.description}
      </span>
      {task.errorDetail && (
        <span className="text-xs text-destructive/80">{task.errorDetail}</span>
      )}
    </li>
  );
};

// ---------------------------------------------------------------------------
// Task Board Card
// ---------------------------------------------------------------------------

export type TaskBoardCardProps = ComponentProps<"div"> & {
  board: TaskBoardDto;
  showTitle?: boolean;
};

export const TaskBoardCard = ({
  board,
  showTitle = true,
  className,
  ...props
}: TaskBoardCardProps) => {
  const progress = computeProgress(board);
  const completed = isBoardCompleted(board);
  const hasFailed = hasFailedTasks(board);

  return (
    <div
      className={cn(
        "rounded-xl border border-app-border/40 bg-app-surface/40 p-3",
        completed && "border-success/30 bg-success/5",
        hasFailed && "border-destructive/30 bg-destructive/5",
        className
      )}
      {...props}
    >
      {showTitle && (
        <div className="mb-3 flex items-center justify-between">
          <h4 className="text-sm font-medium">{board.title}</h4>
          <span
            className={cn(
              "text-xs tabular-nums",
              completed && "text-success",
              hasFailed && "text-destructive",
              !completed && !hasFailed && "text-muted-foreground"
            )}
          >
            {progress}%
          </span>
        </div>
      )}

      {/* Progress bar */}
      <div className="mb-3 h-1.5 w-full overflow-hidden rounded-full bg-muted">
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

      {/* Task list */}
      <ul className="space-y-0.5">
        {board.tasks.map((task: TaskItemDto) => (
          <TaskItemRow
            key={task.id}
            task={task}
            isActive={task.id === board.activeTaskId}
          />
        ))}
      </ul>

      {/* Status badge */}
      {board.status !== "active" && (
        <div className="mt-2 text-xs text-muted-foreground">
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
