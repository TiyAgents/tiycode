import type { RunHelperDto, SubagentProgressSnapshot, ToolCallDto } from "@/shared/types/api";
import { buildSnapshotHelperToolSummary as buildSnapshotHelperToolSummaryFromHelpers } from "@/modules/workbench-shell/model/helpers";

export type RuntimeSurfaceHelperEntry = {
  completedSteps: number;
  currentAction?: string | null;
  error?: string;
  finishedAt?: string | null;
  id: string;
  inputSummary?: string | null;
  kind: string;
  latestMessage?: string;
  recentActions: string[];
  runId: string;
  startedAt: string;
  status: "running" | "completed" | "failed";
  summary?: string | null;
  toolCounts: Record<string, number>;
  totalToolCalls: number;
};

export function mapSnapshotHelperStatus(
  helper: RunHelperDto,
): RuntimeSurfaceHelperEntry["status"] {
  switch (helper.status) {
    case "running":
    case "requested":
    case "created":
    case "dispatching":
    case "waiting_tool_result":
    case "waiting_approval":
      return "running";
    case "completed":
      return "completed";
    case "failed":
    case "interrupted":
    case "cancelled":
      return "failed";
    default:
      return "running";
  }
}

export function buildSnapshotHelperToolSummary(
  helperId: string,
  toolCalls: ReadonlyArray<ToolCallDto>,
) {
  return buildSnapshotHelperToolSummaryFromHelpers(helperId, toolCalls);
}

export function mapSnapshotHelper(
  helper: RunHelperDto,
  toolCalls: ReadonlyArray<ToolCallDto>,
): RuntimeSurfaceHelperEntry {
  const toolSummary = buildSnapshotHelperToolSummary(helper.id, toolCalls);

  return {
    completedSteps: toolSummary.completedSteps,
    currentAction: null,
    error: helper.errorSummary ?? undefined,
    finishedAt: helper.finishedAt,
    id: helper.id,
    inputSummary: helper.inputSummary,
    kind: helper.helperKind,
    latestMessage: undefined,
    recentActions: [],
    runId: helper.runId,
    startedAt: helper.startedAt,
    status: mapSnapshotHelperStatus(helper),
    summary: helper.outputSummary,
    toolCounts: toolSummary.toolCounts,
    totalToolCalls: toolSummary.totalToolCalls,
  };
}

export function applyHelperSnapshot(
  snapshot: SubagentProgressSnapshot,
): Pick<
  RuntimeSurfaceHelperEntry,
  | "completedSteps"
  | "currentAction"
  | "recentActions"
  | "toolCounts"
  | "totalToolCalls"
> {
  return {
    completedSteps: snapshot.completedSteps ?? 0,
    currentAction: snapshot.currentAction ?? null,
    recentActions: snapshot.recentActions ?? [],
    toolCounts: snapshot.toolCounts ?? {},
    totalToolCalls: snapshot.totalToolCalls ?? 0,
  };
}

export function formatHelperKind(kind: string) {
  switch (kind) {
    case "helper_explore":
      return "Explore Agent";
    case "helper_review":
      return "Review Agent";
    default:
      return kind;
  }
}

export function formatToolCallCount(count: number) {
  return `${count} tool call${count === 1 ? "" : "s"}`;
}

export function formatHelperSummary(helper: RuntimeSurfaceHelperEntry) {
  return [
    formatHelperName(helper),
    formatHelperDetailSummary(helper),
  ].filter(Boolean).join(" · ");
}

export function formatHelperName(helper: RuntimeSurfaceHelperEntry) {
  return formatHelperKind(helper.kind);
}

export function formatHelperDetailSummary(helper: RuntimeSurfaceHelperEntry) {
  return [
    helper.inputSummary,
    helper.totalToolCalls > 0 ? formatToolCallCount(helper.totalToolCalls) : null,
  ].filter(Boolean).join(" · ");
}

export function formatHelperStatusLabel(status: RuntimeSurfaceHelperEntry["status"]) {
  switch (status) {
    case "completed":
      return "done";
    case "failed":
      return "failed";
    default:
      return "running";
  }
}

export function formatHelperToolCounts(toolCounts: Record<string, number>) {
  return Object.entries(toolCounts ?? {})
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([toolName, count]) => `${toolName} ${count}`);
}

export function getHelperElapsedSeconds(
  helper: RuntimeSurfaceHelperEntry,
  now = Date.now(),
) {
  const startedAt = new Date(helper.startedAt).getTime();
  const finishedAt = helper.finishedAt ? new Date(helper.finishedAt).getTime() : now;

  if (Number.isNaN(startedAt) || Number.isNaN(finishedAt) || finishedAt < startedAt) {
    return null;
  }

  return (finishedAt - startedAt) / 1000;
}

export function formatElapsedSeconds(seconds: number | null) {
  if (seconds === null) {
    return null;
  }

  return `${seconds.toFixed(1)}s elapsed`;
}

type HelperExecutionSummaryMetrics = {
  elapsedText?: string | null;
  toolUses?: number | null;
};

export function formatExecutionSummary({
  elapsedText,
  toolUses,
}: HelperExecutionSummaryMetrics) {
  const fragments = [
    typeof toolUses === "number" && toolUses > 0
      ? `${toolUses} tool use${toolUses === 1 ? "" : "s"}`
      : null,
    elapsedText ?? null,
  ].filter(Boolean);

  return fragments.length > 0
    ? `Execution Summary: ${fragments.join(", ")}`
    : null;
}
