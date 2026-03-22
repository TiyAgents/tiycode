import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type {
  RunModelPlanDto,
  RunUsageDto,
  SubagentActivityStatus,
  SubagentProgressSnapshot,
  ThreadStreamEvent,
} from "@/shared/types/api";

export type ThreadRunInput = {
  prompt: string;
  displayPrompt?: string | null;
  promptMetadata?: Record<string, unknown> | null;
};

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
    usage:
      value && typeof value.usage === "object" && value.usage
        ? {
            inputTokens:
              typeof (value.usage as Record<string, unknown>).inputTokens === "number"
                ? ((value.usage as Record<string, unknown>).inputTokens as number)
                : typeof (value.usage as Record<string, unknown>).input_tokens === "number"
                  ? ((value.usage as Record<string, unknown>).input_tokens as number)
                  : 0,
            outputTokens:
              typeof (value.usage as Record<string, unknown>).outputTokens === "number"
                ? ((value.usage as Record<string, unknown>).outputTokens as number)
                : typeof (value.usage as Record<string, unknown>).output_tokens === "number"
                  ? ((value.usage as Record<string, unknown>).output_tokens as number)
                  : 0,
            cacheReadTokens:
              typeof (value.usage as Record<string, unknown>).cacheReadTokens === "number"
                ? ((value.usage as Record<string, unknown>).cacheReadTokens as number)
                : typeof (value.usage as Record<string, unknown>).cache_read_tokens === "number"
                  ? ((value.usage as Record<string, unknown>).cache_read_tokens as number)
                  : 0,
            cacheWriteTokens:
              typeof (value.usage as Record<string, unknown>).cacheWriteTokens === "number"
                ? ((value.usage as Record<string, unknown>).cacheWriteTokens as number)
                : typeof (value.usage as Record<string, unknown>).cache_write_tokens === "number"
                  ? ((value.usage as Record<string, unknown>).cache_write_tokens as number)
                  : 0,
            totalTokens:
              typeof (value.usage as Record<string, unknown>).totalTokens === "number"
                ? ((value.usage as Record<string, unknown>).totalTokens as number)
                : typeof (value.usage as Record<string, unknown>).total_tokens === "number"
                  ? ((value.usage as Record<string, unknown>).total_tokens as number)
                  : 0,
          }
        : {
            inputTokens: 0,
            outputTokens: 0,
            cacheReadTokens: 0,
            cacheWriteTokens: 0,
            totalTokens: 0,
          },
  };
}

function readActivity(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
): SubagentActivityStatus {
  return readValue(event, camelKey, snakeKey) as SubagentActivityStatus;
}

function readUsage(event: RawThreadStreamEvent): RunUsageDto {
  const value = readValue(event, "usage", "usage") as Record<string, unknown> | null | undefined;
  return {
    inputTokens:
      typeof value?.inputTokens === "number"
        ? value.inputTokens
        : typeof value?.input_tokens === "number"
          ? value.input_tokens
          : 0,
    outputTokens:
      typeof value?.outputTokens === "number"
        ? value.outputTokens
        : typeof value?.output_tokens === "number"
          ? value.output_tokens
          : 0,
    cacheReadTokens:
      typeof value?.cacheReadTokens === "number"
        ? value.cacheReadTokens
        : typeof value?.cache_read_tokens === "number"
          ? value.cache_read_tokens
          : 0,
    cacheWriteTokens:
      typeof value?.cacheWriteTokens === "number"
        ? value.cacheWriteTokens
        : typeof value?.cache_write_tokens === "number"
          ? value.cache_write_tokens
          : 0,
    totalTokens:
      typeof value?.totalTokens === "number"
        ? value.totalTokens
        : typeof value?.total_tokens === "number"
          ? value.total_tokens
          : 0,
  };
}

function normalizeThreadStreamEvent(rawEvent: RawThreadStreamEvent): ThreadStreamEvent {
  switch (rawEvent.type) {
    case "run_started":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        runMode: readRequiredString(rawEvent, "runMode", "run_mode"),
      };
    case "stream_resync_required":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        droppedEvents: Number(readValue(rawEvent, "droppedEvents", "dropped_events") ?? 0),
      };
    case "run_retrying":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        attempt: Number(readValue(rawEvent, "attempt", "attempt") ?? 0),
        maxAttempts: Number(readValue(rawEvent, "maxAttempts", "max_attempts") ?? 0),
        delayMs: Number(readValue(rawEvent, "delayMs", "delay_ms") ?? 0),
        reason: readRequiredString(rawEvent, "reason", "reason"),
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
    case "message_discarded":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        messageId: readRequiredString(rawEvent, "messageId", "message_id"),
        reason: readRequiredString(rawEvent, "reason", "reason"),
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
        startedAt: readRequiredString(rawEvent, "startedAt", "started_at"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "subagent_progress":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        startedAt: readRequiredString(rawEvent, "startedAt", "started_at"),
        activity: readActivity(rawEvent, "activity", "activity"),
        message: readRequiredString(rawEvent, "message", "message"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "subagent_usage_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        startedAt: readRequiredString(rawEvent, "startedAt", "started_at"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "subagent_completed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        startedAt: readRequiredString(rawEvent, "startedAt", "started_at"),
        summary: readOptionalString(rawEvent, "summary", "summary"),
        snapshot: readSnapshot(rawEvent, "snapshot", "snapshot"),
      };
    case "subagent_failed":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        subtaskId: readRequiredString(rawEvent, "subtaskId", "subtask_id"),
        helperKind: readRequiredString(rawEvent, "helperKind", "helper_kind"),
        startedAt: readRequiredString(rawEvent, "startedAt", "started_at"),
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
    case "clarify_required":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        toolName: readRequiredString(rawEvent, "toolName", "tool_name"),
        toolInput: readValue(rawEvent, "toolInput", "tool_input"),
      };
    case "approval_resolved":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        approved: readBoolean(rawEvent, "approved", "approved"),
      };
    case "clarify_resolved":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        toolCallId: readRequiredString(rawEvent, "toolCallId", "tool_call_id"),
        response: readValue(rawEvent, "response", "response"),
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
    case "thread_title_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        threadId: readRequiredString(rawEvent, "threadId", "thread_id"),
        title: readRequiredString(rawEvent, "title", "title"),
      };
    case "thread_usage_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        modelDisplayName: readOptionalString(rawEvent, "modelDisplayName", "model_display_name"),
        contextWindow: readOptionalString(rawEvent, "contextWindow", "context_window"),
        usage: readUsage(rawEvent),
      };
    case "run_checkpointed":
    case "run_completed":
    case "run_cancelled":
    case "run_interrupted":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
      };
    case "run_limit_reached":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        error: readRequiredString(rawEvent, "error", "error"),
        maxTurns: Number(readValue(rawEvent, "maxTurns", "max_turns") ?? 0),
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
  input: ThreadRunInput,
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
    prompt: input.prompt,
    displayPrompt: input.displayPrompt ?? null,
    promptMetadata: input.promptMetadata ?? null,
    runMode: runMode ?? null,
    modelPlan: modelPlan ?? null,
    onEvent: channel,
  });
}

export async function threadSubscribeRun(
  threadId: string,
  onEvent: (event: ThreadStreamEvent) => void,
): Promise<string | null> {
  requireTauri("thread_subscribe_run");

  const channel = new Channel<RawThreadStreamEvent>();
  channel.onmessage = (event) => {
    onEvent(coerceThreadStreamEvent(event));
  };

  return invoke<string | null>("thread_subscribe_run", {
    threadId,
    onEvent: channel,
  });
}

export async function threadExecuteApprovedPlan(
  threadId: string,
  approvalMessageId: string,
  action: "apply_plan" | "apply_plan_with_context_reset",
  onEvent: (event: ThreadStreamEvent) => void,
): Promise<string> {
  requireTauri("thread_execute_approved_plan");

  const channel = new Channel<RawThreadStreamEvent>();
  channel.onmessage = (event) => {
    onEvent(coerceThreadStreamEvent(event));
  };

  return invoke<string>("thread_execute_approved_plan", {
    threadId,
    approvalMessageId,
    action,
    onEvent: channel,
  });
}

export async function threadClearContext(threadId: string): Promise<void> {
  requireTauri("thread_clear_context");
  return invoke("thread_clear_context", { threadId });
}

export async function threadCompactContext(
  threadId: string,
  instructions?: string | null,
  modelPlan?: RunModelPlanDto | null,
): Promise<void> {
  requireTauri("thread_compact_context");
  return invoke("thread_compact_context", {
    threadId,
    instructions: instructions ?? null,
    modelPlan: modelPlan ?? null,
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

export async function toolClarifyRespond(
  toolCallId: string,
  response: unknown,
): Promise<void> {
  requireTauri("tool_clarify_respond");
  return invoke("tool_clarify_respond", { toolCallId, response });
}
