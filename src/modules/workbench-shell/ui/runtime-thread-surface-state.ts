import type { TranslationKey } from "@/i18n";
import { isHelperOwnedTool } from "@/modules/workbench-shell/model/helpers";
import type {
  ChartMessagePartDto,
  DataMessagePartDto,
  MessageAttachmentDto,
  MessageDto,
  MessagePartDto,
  RunMode,
  RunSummaryDto,
  ThreadSnapshotDto,
  ToolCallDto,
} from "@/shared/types/api";
import type { ThreadStreamEvent } from "@/services/thread-stream";
import type { RuntimeSurfaceHelperEntry } from "@/modules/workbench-shell/ui/runtime-thread-surface-helpers";
import type { RuntimeSurfaceToolState } from "@/modules/workbench-shell/ui/runtime-thread-surface-logic";
import {
  type PlanApprovalAction,
  parseSummaryMarkerMetadata,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-metadata";

export type SurfaceTextMessagePart = {
  type: "text";
  text: string;
};

export type SurfaceChartMessagePart = {
  type: "chart";
  artifactId: string;
  library: string;
  spec: unknown;
  source: string | null;
  title: string | null;
  caption: string | null;
  status: "ready" | "loading" | "error";
  error: string | null;
};

export type SurfaceDataMessagePart = {
  type: `data-${string}`;
  id?: string;
  data: unknown;
};

export type SurfaceUnknownMessagePart = {
  type: string;
  value: Record<string, unknown>;
};

export type SurfaceMessagePart =
  | SurfaceTextMessagePart
  | SurfaceChartMessagePart
  | SurfaceDataMessagePart
  | SurfaceUnknownMessagePart;

export type SurfaceMessage = {
  createdAt: string;
  id: string;
  messageType: MessageDto["messageType"];
  metadata?: unknown | null;
  attachments: MessageAttachmentDto[];
  role: "user" | "assistant" | "system";
  runId: string | null;
  content: string;
  parts: SurfaceMessagePart[];
  status: "streaming" | "completed" | "failed" | "discarded";
};

export type SurfaceToolState = RuntimeSurfaceToolState;

export type SurfaceApproval =
  | {
      id: string;
    }
  | {
      approved: true;
      id: string;
      reason?: string;
    }
  | {
      approved: false;
      id: string;
      reason?: string;
    };

export type SurfaceToolEntry = {
  approval?: SurfaceApproval;
  error?: string;
  finishedAt?: string | null;
  id: string;
  input?: unknown;
  name: string;
  result?: unknown;
  runId: string;
  startedAt: string;
  state: SurfaceToolState;
};

export type SurfaceHelperEntry = RuntimeSurfaceHelperEntry;

export type TimelineEntry =
  | {
      kind: "message";
      key: string;
      occurredAt: string;
      message: SurfaceMessage;
    }
  | {
      kind: "tool";
      key: string;
      occurredAt: string;
      tool: SurfaceToolEntry;
    }
  | {
      kind: "helper";
      key: string;
      occurredAt: string;
      helper: SurfaceHelperEntry;
    };

export type SurfaceRuntimeError = {
  message: string;
  runId: string;
};

export type InitialPromptRequest = {
  id: string;
  threadId: string;
  displayText: string;
  effectivePrompt: string;
  attachments: MessageAttachmentDto[];
  metadata: Record<string, unknown> | null;
  runMode?: RunMode;
};

export type ThinkingPlaceholder = {
  createdAt: string;
  id: string;
  runId?: string | null;
  label?: string;
};

function mapChartPart(part: ChartMessagePartDto): SurfaceChartMessagePart {
  return {
    artifactId: part.artifactId,
    caption: part.caption ?? null,
    error: part.error ?? null,
    library: part.library,
    source: (part as unknown as Record<string, unknown>).source as string ?? null,
    spec: part.spec,
    status: part.status ?? "ready",
    title: part.title ?? null,
    type: "chart",
  };
}

function mapDataPart(part: DataMessagePartDto): SurfaceDataMessagePart {
  return {
    data: part.data,
    id: part.id,
    type: part.type,
  };
}

function mapUnknownPart(part: MessagePartDto): SurfaceUnknownMessagePart {
  return {
    type: part.type,
    value: part as Record<string, unknown>,
  };
}

export function mapMessageParts(parts: MessageDto["parts"], contentMarkdown: string): SurfaceMessagePart[] {
  if (Array.isArray(parts) && parts.length > 0) {
    return parts.map((part): SurfaceMessagePart => {
      if (part.type === "text") {
        return {
          type: "text",
          text: typeof part.text === "string" ? part.text : String(part.text ?? ""),
        };
      }

      if (
        part.type === "chart"
        && "artifactId" in part
        && "library" in part
        && ("spec" in part || "source" in part)
      ) {
        return mapChartPart(part as ChartMessagePartDto);
      }

      if (part.type.startsWith("data-") && "data" in part) {
        return mapDataPart(part as DataMessagePartDto);
      }

      return mapUnknownPart(part);
    });
  }

  return [{ type: "text", text: contentMarkdown }];
}

export type TimelineRole = SurfaceMessage["role"];

export function mapSnapshotMessage(message: MessageDto): SurfaceMessage {
  return {
    createdAt: message.createdAt ?? new Date().toISOString(),
    id: message.id,
    messageType: message.messageType,
    metadata: message.metadata,
    attachments: message.attachments ?? [],
    role:
      message.role === "user" || message.role === "assistant" || message.role === "system"
        ? message.role
        : "assistant",
    runId: message.runId,
    content: message.contentMarkdown,
    parts: mapMessageParts(message.parts, message.contentMarkdown),
    status: message.status,
  };
}



export function deriveSelectedRunMode(snapshot: ThreadSnapshotDto, currentMode: RunMode) {
  if (
    snapshot.thread.status === "waiting_approval"
    && !snapshot.activeRun
    && snapshot.latestRun?.runMode === "plan"
  ) {
    return "plan";
  }

  if (snapshot.activeRun?.runMode === "default" || snapshot.activeRun?.runMode === "plan") {
    return snapshot.activeRun.runMode;
  }

  return currentMode;
}

export function formatApprovalPromptState(state: string, approvedAction: PlanApprovalAction | null, t: (key: TranslationKey) => string) {
  switch (state) {
    case "approved":
      return approvedAction === "apply_plan_with_context_reset"
        ? t("plan.approvedClearAndImplement")
        : approvedAction === "apply_plan"
          ? t("plan.approvedImplement")
          : t("plan.approvedToImplement")
    case "superseded":
      return t("plan.superseded");
    default:
      return t("plan.awaitingApproval");
  }
}

export function mapSnapshotToolState(tool: ToolCallDto): SurfaceToolState {
  switch (tool.status) {
    case "waiting_approval":
      return "approval-requested";
    case "waiting_clarification":
      return "clarify-requested";
    case "approved":
      return "approval-responded";
    case "running":
      return "input-available";
    case "completed":
      return "output-available";
    case "denied":
      return "output-denied";
    case "failed":
    case "cancelled":
      return "output-error";
    default:
      return "input-streaming";
  }
}

export function mapSnapshotToolApproval(tool: ToolCallDto): SurfaceApproval | undefined {
  if (tool.status === "waiting_approval") {
    return { id: tool.id };
  }
  if (tool.approvalStatus === "approved") {
    return { approved: true, id: tool.id };
  }
  if (tool.approvalStatus === "denied" || tool.status === "denied") {
    return { approved: false, id: tool.id };
  }
  return undefined;
}

export function mapSnapshotTool(tool: ToolCallDto): SurfaceToolEntry {
  return {
    approval: mapSnapshotToolApproval(tool),
    error:
      tool.status === "failed" || tool.status === "denied"
        ? typeof tool.toolOutput === "object" && tool.toolOutput && "error" in tool.toolOutput
          ? String((tool.toolOutput as Record<string, unknown>).error)
          : undefined
        : undefined,
    finishedAt: tool.finishedAt,
    id: tool.id,
    input: tool.toolInput,
    name: tool.toolName,
    result: tool.toolOutput ?? undefined,
    runId: tool.runId,
    startedAt: tool.startedAt,
    state: mapSnapshotToolState(tool),
  };
}

function getLatestContextResetMarker(snapshot: ThreadSnapshotDto) {
  for (let index = snapshot.messages.length - 1; index >= 0; index -= 1) {
    const message = snapshot.messages[index];
    const marker = parseSummaryMarkerMetadata(message.metadata);
    if (message.messageType === "summary_marker" && marker?.kind === "context_reset") {
      return message;
    }
  }

  return null;
}

function hasVisibleHistoryForRun(
  snapshot: ThreadSnapshotDto,
  runId: string,
  afterCreatedAt?: string | null,
) {
  return snapshot.messages.some(
    (message) =>
      message.runId === runId
      && message.status !== "discarded"
      && (!afterCreatedAt || message.createdAt > afterCreatedAt),
  );
}

export function getLatestVisibleRun(snapshot: ThreadSnapshotDto) {
  if (snapshot.activeRun) {
    return snapshot.activeRun;
  }

  if (!snapshot.latestRun) {
    return null;
  }

  const latestResetMarker = getLatestContextResetMarker(snapshot);
  if (!latestResetMarker) {
    return snapshot.latestRun;
  }

  if (snapshot.latestRun.startedAt > latestResetMarker.createdAt) {
    return snapshot.latestRun;
  }

  return hasVisibleHistoryForRun(
    snapshot,
    snapshot.latestRun.id,
    latestResetMarker.createdAt,
  )
    ? snapshot.latestRun
    : null;
}

export function mapRunSummaryToContextUsage(run: RunSummaryDto | null) {
  if (!run) {
    return null;
  }

  return {
    contextWindow: run.contextWindow,
    inputTokens: run.usage.inputTokens,
    outputTokens: run.usage.outputTokens,
    cacheReadTokens: run.usage.cacheReadTokens,
    cacheWriteTokens: run.usage.cacheWriteTokens,
    totalTokens: run.usage.totalTokens,
    modelDisplayName: run.modelDisplayName,
    runId: run.id,
  };
}

export function getSnapshotRuntimeError(snapshot: ThreadSnapshotDto): SurfaceRuntimeError | null {
  const run = getLatestVisibleRun(snapshot);
  if (!run) {
    return null;
  }

  if (
    run.status !== "failed"
    && run.status !== "denied"
    && run.status !== "interrupted"
    && run.status !== "limit_reached"
  ) {
    return null;
  }

  return {
    message:
      run.errorMessage
      ?? (
        run.status === "limit_reached"
          ? "The agent hit its maximum tool/turn budget before it could produce a final reply. Continue the thread to let it pick up from the latest tool results."
          : "The app closed or the run was terminated before completion. This thread was restored as interrupted."
      ),
    runId: run.id,
  };
}

/**
 * Message status progression order.  A live-stream message whose status is
 * further along in this list should NOT be overwritten by a stale snapshot
 * that still shows an earlier status.
 */
const MESSAGE_STATUS_ORDER: Record<string, number> = {
  streaming: 0,
  completed: 1,
  discarded: 2,
  failed: 3,
};

export function isMoreAdvancedMessageStatus(localStatus: string, snapshotStatus: string): boolean {
  const localRank = MESSAGE_STATUS_ORDER[localStatus] ?? -1;
  const snapshotRank = MESSAGE_STATUS_ORDER[snapshotStatus] ?? -1;
  return localRank > snapshotRank;
}

export function appendOrReplaceMessage(
  messages: Array<SurfaceMessage>,
  nextMessage: SurfaceMessage,
) {
  const existingIndex = messages.findIndex((entry) => entry.id === nextMessage.id);
  if (existingIndex === -1) {
    return [...messages, nextMessage];
  }

  const nextMessages = [...messages];
  nextMessages[existingIndex] = nextMessage;
  return nextMessages;
}

export type ArtifactEvent = {
  artifactId: string;
  artifactType: string;
  payload?: unknown;
  error?: string;
  kind: "started" | "delta" | "completed" | "failed";
};

/**
 * Merge an artifact event into a message's parts array.
 * - If the artifact type is not "chart", returns the message unchanged.
 * - If an existing chart part with the same artifactId exists, it is updated in place.
 * - Otherwise the new chart part is appended to the parts array.
 */
export function mergeArtifactPartIntoMessage(
  message: SurfaceMessage,
  event: ArtifactEvent,
): SurfaceMessage {
  if (event.artifactType !== "chart") {
    return message;
  }

  const payload = event.payload && typeof event.payload === "object"
    ? event.payload as Record<string, unknown>
    : null;
  const nextChartPart: SurfaceChartMessagePart = {
    type: "chart",
    artifactId: event.artifactId,
    library: typeof payload?.library === "string" ? payload.library : "vega-lite",
    spec: payload?.spec ?? {},
    source: typeof payload?.source === "string" ? payload.source : null,
    title: typeof payload?.title === "string" ? payload.title : null,
    caption: typeof payload?.caption === "string" ? payload.caption : null,
    status: event.kind === "failed" ? "error" : event.kind === "started" ? "loading" : "ready",
    error: event.error ?? (typeof payload?.error === "string" ? payload.error : null),
  };

  const existingIndex = message.parts.findIndex(
    (part) => part.type === "chart" && "artifactId" in part && part.artifactId === event.artifactId,
  );

  if (existingIndex === -1) {
    return {
      ...message,
      parts: [...message.parts, nextChartPart],
    };
  }

  const nextParts = message.parts.slice();
  nextParts[existingIndex] = {
    ...nextParts[existingIndex],
    ...nextChartPart,
  } as SurfaceMessagePart;

  return {
    ...message,
    parts: nextParts,
  };
}

export function prependOlderMessages(
  currentMessages: Array<SurfaceMessage>,
  olderMessages: Array<SurfaceMessage>,
) {
  if (olderMessages.length === 0) {
    return currentMessages;
  }

  const existingIds = new Set(currentMessages.map((message) => message.id));
  const nextOlderMessages = olderMessages.filter((message) => !existingIds.has(message.id));
  if (nextOlderMessages.length === 0) {
    return currentMessages;
  }

  return [...nextOlderMessages, ...currentMessages];
}

export function isRenderableTimelineMessage(message: SurfaceMessage) {
  return message.messageType !== "reasoning" || message.content.trim().length > 0;
}

export function updateTool(
  tools: Array<SurfaceToolEntry>,
  toolId: string,
  updater: (current: SurfaceToolEntry | null) => SurfaceToolEntry,
) {
  const existingIndex = tools.findIndex((entry) => entry.id === toolId);
  const current = existingIndex === -1 ? null : tools[existingIndex];
  const nextTool = updater(current);

  if (existingIndex === -1) {
    return [...tools, nextTool];
  }

  const nextTools = [...tools];
  nextTools[existingIndex] = nextTool;
  return nextTools;
}

export function updateHelper(
  helpers: Array<SurfaceHelperEntry>,
  helperId: string,
  updater: (current: SurfaceHelperEntry | null) => SurfaceHelperEntry,
) {
  const existingIndex = helpers.findIndex((entry) => entry.id === helperId);
  const current = existingIndex === -1 ? null : helpers[existingIndex];
  const nextHelper = updater(current);

  if (existingIndex === -1) {
    return [...helpers, nextHelper];
  }

  const nextHelpers = [...helpers];
  nextHelpers[existingIndex] = nextHelper;
  return nextHelpers;
}

export function getApprovalReason(approval?: SurfaceApproval) {
  return approval && "reason" in approval ? approval.reason : undefined;
}

export function isApprovalDenied(approval?: SurfaceApproval) {
  return Boolean(approval && "approved" in approval && approval.approved === false);
}

export function formatToolStatusLabel(
  state: SurfaceToolState,
  t: (key: TranslationKey) => string,
) {
  switch (state) {
    case "approval-requested":
      return t("tool.runtimeStatus.approval");
    case "approval-responded":
      return t("tool.runtimeStatus.approved");
    case "clarify-requested":
      return t("tool.runtimeStatus.reply");
    case "input-available":
      return t("tool.runtimeStatus.running");
    case "input-streaming":
      return t("tool.runtimeStatus.pending");
    case "output-available":
      return t("tool.runtimeStatus.done");
    case "output-denied":
      return t("tool.runtimeStatus.denied");
    case "output-error":
      return t("tool.runtimeStatus.error");
  }
}

export function getToolStatusClass(state: SurfaceToolState) {
  switch (state) {
    case "approval-requested":
    case "clarify-requested":
      return "text-app-warning";
    case "approval-responded":
      return "text-app-info";
    case "input-available":
    case "input-streaming":
      return "text-app-info";
    case "output-denied":
    case "output-error":
      return "text-app-danger";
    case "output-available":
    default:
      return "text-app-subtle";
  }
}

/**
 * Defines a rough ordering of tool states through their lifecycle.
 * Higher numbers mean the tool is further along.
 */
const TOOL_STATE_ORDER: Record<SurfaceToolState, number> = {
  "input-streaming": 0,
  "input-available": 1,
  "clarify-requested": 2,
  "approval-requested": 3,
  "approval-responded": 4,
  "output-available": 5,
  "output-denied": 5,
  "output-error": 5,
};

/**
 * Returns true if `liveState` is further along in the tool lifecycle
 * than `snapshotState`. Used during snapshot merging to avoid regressing
 * a tool's state when a stale snapshot resolves after live stream events
 * have already advanced the tool.
 */
function isMoreAdvancedToolState(
  liveState: SurfaceToolState,
  snapshotState: SurfaceToolState,
): boolean {
  return TOOL_STATE_ORDER[liveState] > TOOL_STATE_ORDER[snapshotState];
}

/**
 * Merge snapshot tools with live tools.  Uses the snapshot as the base but:
 * 1. Preserves any live tool whose state is more advanced than its snapshot
 *    counterpart (avoids regressing state from a stale snapshot).
 * 2. Appends live-only tools that are not yet present in the snapshot (e.g.
 *    a tool_requested / approval_required event arrived via the stream while
 *    the async snapshot fetch was in flight).
 */
export function mergeSnapshotTools(
  snapshotTools: Array<SurfaceToolEntry>,
  liveTools: Array<SurfaceToolEntry>,
): Array<SurfaceToolEntry> {
  if (liveTools.length === 0) {
    return snapshotTools;
  }

  const snapshotIds = new Set(snapshotTools.map((t) => t.id));

  const merged = snapshotTools.map((snapshotTool) => {
    const liveTool = liveTools.find((t) => t.id === snapshotTool.id);

    if (!liveTool) {
      return snapshotTool;
    }

    if (isMoreAdvancedToolState(liveTool.state, snapshotTool.state)) {
      return liveTool;
    }

    return snapshotTool;
  });

  // Append tools that exist in the live state but not in the snapshot.
  for (const liveTool of liveTools) {
    if (!snapshotIds.has(liveTool.id)) {
      merged.push(liveTool);
    }
  }

  return merged;
}

export function stringifyToolValue(value: unknown) {
  if (typeof value === "string") {
    return value;
  }

  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}


export function getApprovalTagLabel(
  tool: SurfaceToolEntry,
  t: (key: TranslationKey) => string,
) {
  if (tool.state === "approval-requested") {
    return t("tool.tag.approval");
  }

  if (isApprovalDenied(tool.approval)) {
    return t("tool.tag.denied");
  }

  if (tool.approval && "approved" in tool.approval && tool.approval.approved) {
    return t("tool.tag.approved");
  }

  return null;
}

export function getApprovalTagClass(tool: SurfaceToolEntry) {
  if (tool.state === "approval-requested") {
    return "border-app-warning/24 bg-app-warning/10 text-app-warning";
  }

  if (isApprovalDenied(tool.approval)) {
    return "border-app-danger/24 bg-app-danger/10 text-app-danger";
  }

  return "border-app-success/24 bg-app-success/10 text-app-success";
}

function isRuntimeOrchestrationTool(toolName: string) {
  return (
    toolName === "agent_explore"
    || toolName === "agent_review"
  );
}

export function isVisibleTimelineTool(
  tool: SurfaceToolEntry,
  helperIds: ReadonlySet<string>,
) {
  if (tool.name === "clarify") {
    return tool.state === "clarify-requested";
  }

  return !isHelperOwnedTool(tool.id, helperIds) && !isRuntimeOrchestrationTool(tool.name);
}

export function compareTimelineEntries(left: TimelineEntry, right: TimelineEntry) {
  const timestampOrder = left.occurredAt.localeCompare(right.occurredAt);
  if (timestampOrder !== 0) {
    return timestampOrder;
  }

  const kindOrder = getTimelineEntryKindOrder(left) - getTimelineEntryKindOrder(right);
  if (kindOrder !== 0) {
    return kindOrder;
  }

  return left.key.localeCompare(right.key);
}

function getTimelineEntryKindOrder(entry: TimelineEntry) {
  switch (entry.kind) {
    case "message":
      if (entry.message.role === "user") {
        return 0;
      }

      if (entry.message.messageType === "reasoning") {
        return 2;
      }

      return 5;
    case "helper":
      return 3;
    case "tool":
      return 4;
  }
}

export function shouldCompleteThinkingPhase(event: ThreadStreamEvent) {
  switch (event.type) {
    // Visible content arriving — replaces the thinking placeholder
    case "message_delta":
    case "message_discarded":
    // Approval / clarify change run state — placeholder not needed
    case "approval_required":
    case "clarify_required":
    // Terminal run states
    case "run_completed":
    case "run_failed":
    case "run_cancelled":
    case "run_interrupted":
    case "run_limit_reached":
    case "run_checkpointed":
      return true;
    // Note: `context_compressing` is intentionally NOT handled here.
    // It's a placeholder-relabel event, not a phase-completion event — the
    // placeholder is kept on screen with an updated "Compressing…" label via
    // `stream.onContextCompressing`. Treating it as a completion would race
    // with the re-show and can produce a one-frame flash of empty state.
    default:
      return false;
  }
}

/**
 * Events that should finalize in-progress reasoning messages and cancel any
 * scheduled thinking timer, but should NOT clear the thinking placeholder.
 * This prevents the placeholder from vanishing before the replacement UI
 * (tool card / helper card / plan) actually renders — especially when React 18
 * batches the placeholder-clear and replacement into a single frame.
 */
export function shouldFinalizeReasoningOnly(event: ThreadStreamEvent) {
  switch (event.type) {
    case "tool_requested":
    case "tool_running":
    case "tool_completed":
    case "tool_failed":
    case "subagent_started":
    case "subagent_progress":
    case "subagent_completed":
    case "subagent_failed":
    case "plan_updated":
      return true;
    default:
      return false;
  }
}

export function getPresentationEntryRole(entry: TimelineEntry): TimelineRole {
  if (entry.kind === "message") {
    return entry.message.role;
  }

  return "assistant";
}

export function getRoleSpacingClass(
  previousRole: TimelineRole | null,
  currentRole: TimelineRole,
) {
  if (!previousRole) {
    return undefined;
  }

  return previousRole === currentRole ? "pt-3" : "pt-6";
}
