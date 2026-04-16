"use client";

import { CheckCircle2Icon, ChevronDownIcon, ListChecksIcon, XCircleIcon } from "lucide-react";
import type { ComponentProps } from "react";
import { cn } from "@/shared/lib/utils";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/shared/ui/collapsible";
import type { TaskBoardDto } from "../model/task-board";
import { computeProgress, hasFailedTasks, isBoardCompleted } from "../model/task-board";

// ---------------------------------------------------------------------------
// Task Stage History Card
// ---------------------------------------------------------------------------

export type TaskStageHistoryCardProps = ComponentProps<"div"> & {
  board: TaskBoardDto;
};

export const TaskStageHistoryCard = ({
  board,
  className,
  ...props
}: TaskStageHistoryCardProps) => {
  const progress = computeProgress(board);
  const completed = isBoardCompleted(board);
  const hasFailed = hasFailedTasks(board);

  // Don't show history card for active boards
  if (board.status === "active") {
    return null;
  }

  return (
    <div
      className={cn(
        "rounded-lg border border-app-border/30 bg-app-surface/30 p-3",
        completed && "border-success/20 bg-success/5",
        hasFailed && "border-destructive/20 bg-destructive/5",
        className
      )}
      {...props}
    >
      <div className="flex items-start gap-2">
        <div className="flex-shrink-0 mt-0.5">
          {completed ? (
            <CheckCircle2Icon className="size-4 text-success" />
          ) : (
            <XCircleIcon className="size-4 text-destructive" />
          )}
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between gap-2">
            <h4 className="text-sm font-medium truncate">{board.title}</h4>
            <span
              className={cn(
                "text-xs tabular-nums flex-shrink-0",
                completed && "text-success",
                hasFailed && "text-destructive"
              )}
            >
              {progress}%
            </span>
          </div>
          <div className="mt-1 text-xs text-muted-foreground">
            {board.tasks.filter((t) => t.stage === "completed").length} of{" "}
            {board.tasks.length} steps completed
          </div>
          {hasFailed && (
            <div className="mt-1 text-xs text-destructive/80">
              {board.tasks.filter((t) => t.stage === "failed").length} step(s) failed
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Task History Timeline
// ---------------------------------------------------------------------------

export type TaskHistoryTimelineProps = ComponentProps<"div"> & {
  boards: TaskBoardDto[];
};

export const TaskHistoryTimeline = ({
  boards,
  className,
  ...props
}: TaskHistoryTimelineProps) => {
  // Filter to only show completed/abandoned boards
  const historyBoards = boards.filter((b) => b.status !== "active");

  if (historyBoards.length === 0) {
    return null;
  }

  return (
    <div className={cn("space-y-2", className)} {...props}>
      <Collapsible defaultOpen={false}>
        <CollapsibleTrigger className="group flex w-full items-center justify-between gap-2 rounded-md text-xs text-muted-foreground transition-colors hover:text-foreground">
          <div className="flex items-center gap-2">
            <ListChecksIcon className="size-3" />
            <span>Task History</span>
            <span className="rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
              {historyBoards.length}
            </span>
          </div>
          <ChevronDownIcon className="size-4 transition-transform group-data-[state=open]:rotate-180" />
        </CollapsibleTrigger>
        <CollapsibleContent className="space-y-2 pt-2 outline-none data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 data-[state=closed]:animate-out data-[state=open]:animate-in">
          {historyBoards.map((board) => (
            <TaskStageHistoryCard key={board.id} board={board} />
          ))}
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
};
