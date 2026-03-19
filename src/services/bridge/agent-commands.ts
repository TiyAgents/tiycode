import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type {
  RunModelPlanDto,
  SubagentActivityStatus,
  SubagentProgressSnapshot,
  ThreadStreamEvent,
} from "@/shared/types/api";

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

type RawThreadStreamEvent = {
  type: ThreadStreamEvent["type"];
  [key: string]: unknown;
};

function readRequiredString(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
) {
  const value = event[camelKey] ?? event[snakeKey];
  if (typeof value !== "string") {
    throw new Error(`Malformed thread stream event '${event.type}': missing ${camelKey}`);
  }
  return value;
}

function readBoolean(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
) {
  const value = event[camelKey] ?? event[snakeKey];
  if (typeof value !== "boolean") {
    throw new Error(`Malformed thread stream event '${event.type}': missing ${camelKey}`);
  }
  return value;
}

function readOptionalString(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
) {
  const value = event[camelKey] ?? event[snakeKey];
  if (value == null) {
    return null;
  }
  if (typeof value !== "string") {
    throw new Error(`Malformed thread stream event '${event.type}': invalid ${camelKey}`);
  }
  return value;
}

function readValue(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
) {
  return event[camelKey] ?? event[snakeKey];
}

function readSnapshot(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
): SubagentProgressSnapshot {
  const value = readValue(event, camelKey, snakeKey) as Record<string, unknown> | null | undefined;
  return {
    totalToolCalls:
      typeof value?.totalToolCalls === "number"
        ? value.totalToolCalls
        : typeof value?.total_tool_calls === "number"
          ? value.total_tool_calls
          : 0,
    completedSteps:
      typeof value?.completedSteps === "number"
        ? value.completedSteps
        : typeof value?.completed_steps === "number"
          ? value.completed_steps
          : 0,
    currentAction:
      typeof value?.currentAction === "string"
        ? value.currentAction
        : typeof value?.current_action === "string"
          ? value.current_action
          : null,
    toolCounts:
      value && typeof value.toolCounts === "object" && value.toolCounts
        ? value.toolCounts as Record<string, number>
        : value && typeof value.tool_counts === "object" && value.tool_counts
          ? value.tool_counts as Record<string, number>
          : {},
    recentActions:
      Array.isArray(value?.recentActions)
        ? value.recentActions.filter((entry): entry is string => typeof entry === "string")
        : Array.isArray(value?.recent_actions)
          ? value.recent_actions.filter((entry): entry is string => typeof entry === "string")
          : [],
  };
}

function readActivity(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
): SubagentActivityStatus {
  return readValue(event, camelKey, snakeKey) as SubagentActivityStatus;
}

function normalizeThreadStreamEvent(rawEvent: RawThreadStreamEvent): ThreadStreamEvent {
  switch (rawEvent.type) {
    case "run_started":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        runMode: readRequiredString(rawEvent, "runMode", "run_mode"),
      };
    case "message_delta":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        messageId: readRequiredString(rawEvent, "messageId", "message_id"),
        delta: readRequiredString(rawEvent, "delta", "delta"),
      };
    case "message_completed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        messageId: readRequiredString(rawEvent, "messageId", "message_id"),
        content: readRequiredString(rawEvent, "content", "content"),
      };
    case "plan_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        plan: readValue(rawEvent, "plan", "plan"),
      };
    case "reasoning_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        messageId: readRequiredString(rawEvent, "messageId", "message_id"),
        reasoning: readRequiredString(rawEvent, "reasoning", "reasoning"),
      };
    case "queue_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        queue: readValue(rawEvent, "queue", "queue"),
      };
    case "subagent_started":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "subagent_progress":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        activity: readActivity(rawEvent, "activity", "activity"),
        message: readRequiredString(rawEvent, "message", "message"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "subagent_completed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        summary: readOptionalString(rawEvent, "summary", "summary"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "subagent_failed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        error: readRequiredString(rawEvent, "error", "error"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "tool_requested":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        toolName: readRequiredString(rawEvent, "toolName", "tool_name"),
        toolInput: readValue(rawEvent, "toolInput", "tool_input"),
      };
    case "approval_required":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        toolName: readRequiredString(rawEvent, "toolName", "tool_name"),
        toolInput: readValue(rawEvent, "toolInput", "tool_input"),
        reason: readRequiredString(rawEvent, "reason", "reason"),
      };
    case "approval_resolved":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        approved: readBoolean(rawEvent, "approved", "approved"),
      };
    case "tool_running":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
      };
    case "tool_completed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        result: readValue(rawEvent, "result", "result"),
      };
    case "tool_failed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        error: readRequiredString(rawEvent, "error", "error"),
      };
    case "run_completed":
    case "run_cancelled":
    case "run_interrupted":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
      };
    case "run_failed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        error: readRequiredString(rawEvent, "error", "error"),
      };
  }
}

function coerceThreadStreamEvent(rawEvent: RawThreadStreamEvent): ThreadStreamEvent {
  try {
    return normalizeThreadStreamEvent(rawEvent);
  } catch (error) {
    console.error("Failed to normalize thread stream event", rawEvent, error);
    return rawEvent as ThreadStreamEvent;
  }
}

/**
 * Start a new agent run for a thread.
 *
 * Returns the run ID. Events are delivered via the `onEvent` callback,
 * which is backed by a Tauri Channel for real-time streaming.
 */
export async function threadStartRun(
  threadId: string,
  prompt: string,
  onEvent: (event: ThreadStreamEvent) => void,
  runMode?: string,
  modelPlan?: RunModelPlanDto | null,
): Promise<string> {
  requireTauri("thread_start_run");

  const channel = new Channel<RawThreadStreamEvent>();
  channel.onmessage = (event) => {
    onEvent(coerceThreadStreamEvent(event));
  };

  return invoke<string>("thread_start_run", {
    threadId,
    prompt,
    runMode: runMode ?? null,
    modelPlan: modelPlan ?? null,
    onEvent: channel,
  });
}

export async function threadCancelRun(threadId: string): Promise<void> {
  requireTauri("thread_cancel_run");
  return invoke("thread_cancel_run", { threadId });
}

export async function toolApprovalRespond(
  toolCallId: string,
  runId: string,
  approved: boolean,
): Promise<void> {
  requireTauri("tool_approval_respond");
  return invoke("tool_approval_respond", { toolCallId, runId, approved });
}
