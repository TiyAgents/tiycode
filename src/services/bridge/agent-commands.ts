import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type {
  MessageAttachmentDto,
  RunModelPlanDto,
  RunUsageDto,
  RuntimeQueueMessageKind,
  RuntimeQueueSnapshotDto,
  SubagentActivityStatus,
  SubagentProgressSnapshot,
  TaskBoardDto,
  ThreadStreamEvent,
} from "@/shared/types/api";

export type ThreadRunInput = {
  prompt: string;
  displayPrompt?: string | null;
  promptMetadata?: Record<string, unknown> | null;
  attachments?: MessageAttachmentDto[];
};

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

export type RawThreadStreamEvent = {
  type: ThreadStreamEvent["type"];
  [key: string]: unknown;
};

const VALID_ARTIFACT_STATUSES = new Set(["started", "delta", "completed", "failed"]);

function readArtifactStatus(rawStatus: string): "started" | "delta" | "completed" | "failed" {
  if (VALID_ARTIFACT_STATUSES.has(rawStatus)) {
    return rawStatus as "started" | "delta" | "completed" | "failed";
  }
  console.warn(`[agent-commands] Unknown artifact status "${rawStatus}", falling back to "completed"`);
  return "completed";
}

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

function readRequiredNumber(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
) {
  const value = event[camelKey] ?? event[snakeKey];
  if (typeof value !== "number") {
    throw new Error(`Malformed thread stream event '${event.type}': missing ${camelKey}`);
  }
  return value;
}

function readRequiredObject(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
) {
  const value = event[camelKey] ?? event[snakeKey];
  if (value === undefined || value === null) {
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

function readObjectMetadata(event: RawThreadStreamEvent): Record<string, unknown> | null {
  const value = readValue(event, "metadata", "metadata");
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function readNumberOrNull(
  event: RawThreadStreamEvent,
  camelKey: string,
  snakeKey: string,
) {
  const value = event[camelKey] ?? event[snakeKey];
  if (value == null) {
    return null;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
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

function readUsage(event: RawThreadStreamEvent): RunUsageDto {
  const value = readValue(event, "usage", "usage") as Record<string, unknown> | null | undefined;
  const inputTokens = readUsageField(value, "inputTokens", "input_tokens");
  const outputTokens = readUsageField(value, "outputTokens", "output_tokens");
  const cacheReadTokens = readUsageField(value, "cacheReadTokens", "cache_read_tokens");
  const cacheWriteTokens = readUsageField(value, "cacheWriteTokens", "cache_write_tokens");
  const totalTokens = readUsageField(value, "totalTokens", "total_tokens");
  // Prefer the unified `context_size` field sent by tiycore >= 0.2.10-rc.2
  // (it sums input + output + cache_read + cache_write consistently across
  // OpenAI / Anthropic / Google). Fall back to that sum when the field is
  // missing — e.g. older persisted snapshots or partial hand-crafted events.
  const explicitContextSize = readUsageField(value, "contextSize", "context_size");
  const contextSize =
    explicitContextSize > 0
      ? explicitContextSize
      : inputTokens + outputTokens + cacheReadTokens + cacheWriteTokens;
  return {
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheWriteTokens,
    totalTokens,
    contextSize,
  };
}

function readUsageField(
  value: Record<string, unknown> | null | undefined,
  camelKey: string,
  snakeKey: string,
): number {
  if (!value) return 0;
  if (typeof value[camelKey] === "number") return value[camelKey];
  if (typeof value[snakeKey] === "number") return value[snakeKey];
  return 0;
}

function readRuntimeQueueSnapshot(value: unknown): RuntimeQueueSnapshotDto {
  const fallbackId = () => Math.random().toString(36).slice(2);
  const raw = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const messages = Array.isArray(raw.messages) ? raw.messages : [];
  const events = Array.isArray(raw.events) ? raw.events : [];

  return {
    steeringDepth: Number(raw.steeringDepth ?? raw.steering_depth ?? 0),
    followUpDepth: Number(raw.followUpDepth ?? raw.follow_up_depth ?? 0),
    isDeferringSteering: Boolean(raw.isDeferringSteering ?? raw.is_deferring_steering ?? false),
    messages: messages
      .filter((entry): entry is Record<string, unknown> => Boolean(entry) && typeof entry === "object")
      .map((entry) => ({
        id: typeof entry.id === "string" ? entry.id : fallbackId(),
        kind: entry.kind === "follow_up" ? "follow_up" : "steer",
        content: typeof entry.content === "string" ? entry.content : "",
        metadata: entry.metadata && typeof entry.metadata === "object" && !Array.isArray(entry.metadata)
          ? entry.metadata as Record<string, unknown>
          : null,
        status:
          entry.status === "consumed"
          || entry.status === "cleared"
          || entry.status === "cancelled"
            ? entry.status
            : "pending",
        createdAt: typeof entry.createdAt === "string"
          ? entry.createdAt
          : typeof entry.created_at === "string"
            ? entry.created_at
            : new Date().toISOString(),
        updatedAt: typeof entry.updatedAt === "string"
          ? entry.updatedAt
          : typeof entry.updated_at === "string"
            ? entry.updated_at
            : new Date().toISOString(),
      })),
    events: events
      .filter((entry): entry is Record<string, unknown> => Boolean(entry) && typeof entry === "object")
      .map((entry) => ({
        id: typeof entry.id === "string" ? entry.id : fallbackId(),
        kind: entry.kind === "follow_up" ? "follow_up" : "steer",
        action:
          entry.action === "consumed"
          || entry.action === "cleared"
          || entry.action === "removed"
          || entry.action === "transferred"
            ? entry.action
            : "enqueued",
        count: Number(entry.count ?? 0),
        queueDepth: entry.queueDepth !== undefined || entry.queue_depth !== undefined
          ? Number(entry.queueDepth ?? entry.queue_depth)
          : undefined,
        remaining: entry.remaining !== undefined ? Number(entry.remaining) : undefined,
        countDropped: entry.countDropped !== undefined || entry.count_dropped !== undefined
          ? Number(entry.countDropped ?? entry.count_dropped)
          : undefined,
        createdAt: typeof entry.createdAt === "string"
          ? entry.createdAt
          : typeof entry.created_at === "string"
            ? entry.created_at
            : new Date().toISOString(),
      })),
  };
}

export function normalizeThreadStreamEvent(rawEvent: RawThreadStreamEvent): ThreadStreamEvent {
  switch (rawEvent.type) {
    case "run_started":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        runMode: readRequiredString(rawEvent, "runMode", "run_mode"),
        startedAtMs: readRequiredNumber(rawEvent, "startedAtMs", "started_at_ms"),
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
    case "request_retrying":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        attempt: Number(readValue(rawEvent, "attempt", "attempt") ?? 0),
        maxRetries: Number(readValue(rawEvent, "maxRetries", "max_retries") ?? 0),
        delayMs: Number(readValue(rawEvent, "delayMs", "delay_ms") ?? 0),
        reason: readRequiredString(rawEvent, "reason", "reason"),
        status: readNumberOrNull(rawEvent, "status", "status"),
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
    case "artifact_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        messageId: readRequiredString(rawEvent, "messageId", "message_id"),
        artifactId: readRequiredString(rawEvent, "artifactId", "artifact_id"),
        artifactType: readRequiredString(rawEvent, "artifactType", "artifact_type"),
        status: readArtifactStatus(readRequiredString(rawEvent, "status", "status")),
        payload: readValue(rawEvent, "payload", "payload"),
        error: readOptionalString(rawEvent, "error", "error") ?? undefined,
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
        queue: readRuntimeQueueSnapshot(readValue(rawEvent, "queue", "queue")),
      };
    case "user_message_recorded":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        messageId: readRequiredString(rawEvent, "messageId", "message_id"),
        content: readRequiredString(rawEvent, "content", "content"),
        createdAt: readRequiredString(rawEvent, "createdAt", "created_at"),
        metadata: readObjectMetadata(rawEvent),
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
    case "context_compressing":
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
    case "task_board_updated":
      return {
        type: rawEvent.type,
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        taskBoard: readRequiredObject(rawEvent, "taskBoard", "task_board") as TaskBoardDto,
      };
    case "goal_state_updated":
      return {
        type: rawEvent.type,
        threadId: readRequiredString(rawEvent, "threadId", "thread_id"),
        goal: (readValue(rawEvent, "goal", "goal") ?? null) as GoalPayload | null,
      };
    case "goal_continuation":
      return {
        type: rawEvent.type,
        threadId: readRequiredString(rawEvent, "threadId", "thread_id"),
        runId: readRequiredString(rawEvent, "runId", "run_id"),
        prompt: readRequiredString(rawEvent, "prompt", "prompt"),
        turnsUsed: Number(readValue(rawEvent, "turnsUsed", "turns_used") ?? 0),
        maxTurns: Number(readValue(rawEvent, "maxTurns", "max_turns") ?? 0),
      };
    case "goal_paused":
      return {
        type: rawEvent.type,
        threadId: readRequiredString(rawEvent, "threadId", "thread_id"),
        reason: readRequiredString(rawEvent, "reason", "reason"),
        detail: readOptionalString(rawEvent, "detail", "detail") ?? undefined,
      };
    case "goal_completed":
      return {
        type: rawEvent.type,
        threadId: readRequiredString(rawEvent, "threadId", "thread_id"),
        evidence: readRequiredString(rawEvent, "evidence", "evidence"),
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
    attachments: input.attachments ?? [],
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

export async function threadEnqueueQueueMessage(
  threadId: string,
  kind: RuntimeQueueMessageKind,
  message: string,
  metadata?: Record<string, unknown> | null,
): Promise<RuntimeQueueSnapshotDto> {
  requireTauri("thread_enqueue_queue_message");
  return readRuntimeQueueSnapshot(await invoke("thread_enqueue_queue_message", {
    threadId,
    kind,
    message,
    metadata: metadata ?? null,
  }));
}

export async function threadClearRuntimeQueue(
  threadId: string,
  kind?: RuntimeQueueMessageKind | null,
): Promise<RuntimeQueueSnapshotDto> {
  requireTauri("thread_clear_runtime_queue");
  return readRuntimeQueueSnapshot(await invoke("thread_clear_runtime_queue", {
    threadId,
    kind: kind ?? null,
  }));
}

export async function threadCancelRuntimeQueueMessage(
  threadId: string,
  messageId: string,
): Promise<RuntimeQueueSnapshotDto> {
  requireTauri("thread_cancel_runtime_queue_message");
  return readRuntimeQueueSnapshot(await invoke("thread_cancel_runtime_queue_message", {
    threadId,
    messageId,
  }));
}

export async function threadPromoteRuntimeQueueMessage(
  threadId: string,
  messageId: string,
): Promise<RuntimeQueueSnapshotDto> {
  requireTauri("thread_promote_runtime_queue_message");
  return readRuntimeQueueSnapshot(await invoke("thread_promote_runtime_queue_message", {
    threadId,
    messageId,
  }));
}

export async function threadClearContext(threadId: string): Promise<void> {
  requireTauri("thread_clear_context");
  return invoke("thread_clear_context", { threadId });
}

export async function threadCompactContext(
  threadId: string,
  instructions: string | null | undefined,
  modelPlan: RunModelPlanDto | null | undefined,
  onEvent: (event: ThreadStreamEvent) => void,
): Promise<string> {
  requireTauri("thread_compact_context");

  const channel = new Channel<RawThreadStreamEvent>();
  channel.onmessage = (event) => {
    onEvent(coerceThreadStreamEvent(event));
  };

  return invoke<string>("thread_compact_context", {
    threadId,
    instructions: instructions ?? null,
    modelPlan: modelPlan ?? null,
    onEvent: channel,
  });
}

export async function threadCancelRun(threadId: string): Promise<boolean> {
  requireTauri("thread_cancel_run");
  return invoke<boolean>("thread_cancel_run", { threadId });
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

// ── Goal bridge ──

export type GoalPayload = {
  id: string;
  threadId: string;
  objective: string;
  status: "active" | "paused" | "budget_limited" | "complete";
  tokensUsed: number;
  turnsUsed: number;
  maxTurns: number;
  tokenBudget?: number | null;
  pauseReason?: string | null;
  pauseDetail?: string | null;
  evidence?: string | null;
  lastEvaluatedRunId?: string | null;
  judgePassed?: boolean;
  judgeCompleteness?: number | null;
  judgeFindings?: string | null;
  judgeSummary?: string | null;
  judgeEvaluatedRunId?: string | null;
};

export async function goalGetState(threadId: string): Promise<GoalPayload | null> {
  requireTauri("goal_get_state");
  return invoke("goal_get_state", { threadId });
}

export async function goalSet(
  threadId: string,
  objective: string,
  tokenBudget?: number | null,
): Promise<GoalPayload> {
  requireTauri("goal_set");
  return invoke("goal_set", { threadId, objective, tokenBudget });
}

export async function goalPause(threadId: string): Promise<GoalPayload | null> {
  requireTauri("goal_pause");
  return invoke("goal_pause", { threadId });
}

export async function goalResume(threadId: string): Promise<GoalPayload | null> {
  requireTauri("goal_resume");
  return invoke("goal_resume", { threadId });
}

export async function goalClear(threadId: string): Promise<boolean> {
  requireTauri("goal_clear");
  return invoke("goal_clear", { threadId });
}

export type GoalEvaluateResult = {
  goal: GoalPayload;
  verdict: "continue" | "challenge_evidence" | "paused" | "budget_limited" | "skipped";
  continuationPrompt?: string | null;
};

export async function goalEvaluate(
  threadId: string,
  response?: string | null,
): Promise<GoalEvaluateResult | null> {
  requireTauri("goal_evaluate");
  return invoke("goal_evaluate", { threadId, response });
}
