"use client";

import type { ChatStatus } from "ai";
import { AlertCircleIcon, BotIcon, Info, RefreshCcwIcon, SparklesIcon, WrenchIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useT, type TranslationKey } from "@/i18n";
import { CodeBlock } from "@/components/ai-elements/code-block";
import {
  CompactCollapsible,
  CompactCollapsibleContent,
  CompactCollapsibleFootnote,
  CompactCollapsibleHeader,
} from "@/components/ai-elements/compact-collapsible";
import { Conversation, ConversationContent, ConversationEmptyState, ConversationScrollButton } from "@/components/ai-elements/conversation";
import type { StickToBottomContext } from "use-stick-to-bottom";
import { Message, MessageContent, MessageResponse } from "@/components/ai-elements/message";
import { Plan, PlanContent, PlanDescription, PlanHeader, PlanTitle, PlanTrigger } from "@/components/ai-elements/plan";
import { Queue } from "@/components/ai-elements/queue";
import { Reasoning, ReasoningContent, ReasoningTrigger } from "@/components/ai-elements/reasoning";
import { Shimmer } from "@/components/ai-elements/shimmer";
import { ToolInput, ToolOutput } from "@/components/ai-elements/tool";
import { Confirmation, ConfirmationAccepted, ConfirmationAction, ConfirmationActions, ConfirmationRejected, ConfirmationRequest, ConfirmationTitle } from "@/components/ai-elements/confirmation";
import { buildRunModelPlanFromSelection } from "@/modules/settings-center/model/run-model-plan";
import type { AgentProfile, CommandEntry, ProviderEntry } from "@/modules/settings-center/model/types";
import { threadClearContext, threadCompactContext, threadLoad } from "@/services/bridge";
import {
  ThreadStream,
  type HelperEvent,
  type QueueEvent,
  type RunState,
  type ThreadStreamEvent,
  type ThreadTitleEvent,
  type UsageEvent,
} from "@/services/thread-stream";
import type {
  MessageAttachmentDto,
  MessageDto,
  RunMode,
  RunSummaryDto,
  RunHelperDto,
  SubagentProgressSnapshot,
  ThreadSnapshotDto,
  ToolCallDto,
  TaskBoardDto,
} from "@/shared/types/api";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import type { ComposerSubmission } from "@/modules/workbench-shell/model/composer-commands";
import type { SkillRecord } from "@/shared/types/extensions";
import {
  getFileMutationPresentation,
} from "@/modules/workbench-shell/model/file-mutation-presentation";
import { WorkbenchPromptComposer, ComposerMessageAttachments } from "@/modules/workbench-shell/ui/workbench-prompt-composer";
import {
  initialTaskBoardState,
  taskBoardsFromSnapshot,
  applyTaskBoardUpdate,
  type TaskBoardState,
} from "@/modules/workbench-shell/model/task-board";
import { TaskBoardCard } from "@/modules/workbench-shell/ui/task-board-card";
import { TaskHistoryTimeline } from "@/modules/workbench-shell/ui/task-stage-history-card";

type SurfaceMessage = {
  createdAt: string;
  id: string;
  messageType: MessageDto["messageType"];
  metadata?: unknown | null;
  attachments: MessageAttachmentDto[];
  role: "user" | "assistant" | "system";
  runId: string | null;
  content: string;
  status: "streaming" | "completed" | "failed" | "discarded";
};

type SurfaceToolState =
  | "approval-requested"
  | "approval-responded"
  | "clarify-requested"
  | "input-streaming"
  | "input-available"
  | "output-available"
  | "output-denied"
  | "output-error";

type SurfaceApproval =
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

type SurfaceToolEntry = {
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

type SurfaceHelperEntry = {
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

type TimelineEntry =
  | {
      kind: "message";
      key: string;
      occurredAt: string;
      message: SurfaceMessage;
    }
  | {
      kind: "thinking-placeholder";
      key: string;
      occurredAt: string;
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

type SurfaceRuntimeError = {
  message: string;
  runId: string;
};

type InitialPromptRequest = {
  id: string;
  threadId: string;
  displayText: string;
  effectivePrompt: string;
  attachments: MessageAttachmentDto[];
  metadata: Record<string, unknown> | null;
  runMode?: RunMode;
};

type ThinkingPlaceholder = {
  createdAt: string;
  id: string;
  runId?: string | null;
};

type RuntimeThreadSurfaceProps = {
  activeAgentProfileId: string;
  agentProfiles: ReadonlyArray<AgentProfile>;
  commands?: ReadonlyArray<CommandEntry>;
  composerDraft?: string;
  enabledSkills?: ReadonlyArray<Pick<SkillRecord, "id" | "name" | "description" | "scope" | "source" | "tags" | "triggers" | "contentPreview">>;
  initialPromptRequest?: InitialPromptRequest | null;
  onComposerDraftChange?: (value: string) => void;
  onConsumeInitialPrompt?: (id: string) => void;
  onContextUsageChange?: (usage: ThreadContextUsage | null) => void;
  onRunStateChange?: (state: RunState) => void;
  onSelectAgentProfile: (id: string) => void;
  onThreadTitleChange?: (threadId: string, title: string) => void;
  providers: ReadonlyArray<ProviderEntry>;
  threadId: string | null;
  threadTitle: string;
  workspaceId?: string | null;
};

export type ThreadContextUsage = {
  contextWindow: string | null;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  totalTokens: number;
  modelDisplayName: string | null;
  runId: string;
};

type PlanApprovalAction = "apply_plan" | "apply_plan_with_context_reset";

type PlanStepMetadata = {
  description?: string;
  files?: string[];
  id?: string;
  status?: string;
  title?: string;
};

type PlanMessageMetadata = {
  approvalState?: string;
  assumptions?: string[];
  context?: string;
  design?: string;
  generatedFromRunId?: string;
  kind?: string;
  keyImplementation?: string;
  needsContextResetOption?: boolean;
  planRevision?: number;
  risks?: string[];
  runModeAtCreation?: string;
  steps?: PlanStepMetadata[];
  summary?: string;
  title?: string;
  verification?: string;
};

type FormattedPlan = {
  approvalState: string | null;
  assumptions: string[];
  context: string;
  design: string;
  keyImplementation: string;
  planRevision: number | null;
  risks: string[];
  steps: string[];
  summary: string;
  title: string;
  verification: string;
};

type FormattedApprovalPrompt = {
  approvedAction: PlanApprovalAction | null;
  options: Array<{ action: PlanApprovalAction; label: string }>;
  planMessageId: string | null;
  planRevision: number | null;
  state: string;
};

type ClarifyOption = {
  description: string;
  id: string;
  label: string;
  recommended: boolean;
};

type ClarifyPrompt = {
  header: string | null;
  options: ClarifyOption[];
  question: string;
};

type TimelineRole = SurfaceMessage["role"];

function mapSnapshotMessage(message: MessageDto): SurfaceMessage {
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
    status: message.status,
  };
}

function asObjectRecord(value: unknown) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

function readStringField(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "string" ? value : null;
}

function readNumberField(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "number" ? value : null;
}

function readStringArrayField(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((entry): entry is string => typeof entry === "string" && entry.trim().length > 0);
}

function parsePlanMessageMetadata(value: unknown): PlanMessageMetadata | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  const stepEntries = Array.isArray(record.steps)
    ? record.steps
        .map<PlanStepMetadata | null>((step) => {
          if (typeof step === "string") {
            return { title: step };
          }

          const stepRecord = asObjectRecord(step);
          if (!stepRecord) {
            return null;
          }

          return {
            description: readStringField(stepRecord, "description") ?? undefined,
            files: readStringArrayField(stepRecord, "files"),
            id: readStringField(stepRecord, "id") ?? undefined,
            status: readStringField(stepRecord, "status") ?? undefined,
            title: readStringField(stepRecord, "title") ?? undefined,
          };
        })
        .filter((step): step is PlanStepMetadata => step !== null)
    : [];

  return {
    approvalState: readStringField(record, "approvalState") ?? undefined,
    assumptions: readStringArrayField(record, "assumptions"),
    context: readStringField(record, "context") ?? undefined,
    design: readStringField(record, "design") ?? undefined,
    generatedFromRunId: readStringField(record, "generatedFromRunId") ?? undefined,
    kind: readStringField(record, "kind") ?? undefined,
    keyImplementation: readStringField(record, "keyImplementation") ?? undefined,
    needsContextResetOption:
      typeof record.needsContextResetOption === "boolean" ? record.needsContextResetOption : undefined,
    planRevision: readNumberField(record, "planRevision") ?? undefined,
    risks: readStringArrayField(record, "risks"),
    runModeAtCreation: readStringField(record, "runModeAtCreation") ?? undefined,
    steps: stepEntries,
    summary: readStringField(record, "summary") ?? undefined,
    title: readStringField(record, "title") ?? undefined,
    verification: readStringField(record, "verification") ?? undefined,
  };
}

function parseApprovalPromptMetadata(value: unknown, t: (key: TranslationKey) => string): FormattedApprovalPrompt | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  const options = Array.isArray(record.options)
    ? record.options
        .map((entry) => {
          const optionRecord = asObjectRecord(entry);
          const action = readStringField(optionRecord, "action");
          const label = readStringField(optionRecord, "label");
          if (
            (action !== "apply_plan" && action !== "apply_plan_with_context_reset")
            || !label
          ) {
            return null;
          }

          return { action, label };
        })
        .filter((option): option is { action: PlanApprovalAction; label: string } => Boolean(option))
    : [];

  return {
    approvedAction:
      readStringField(record, "approvedAction") === "apply_plan"
      || readStringField(record, "approvedAction") === "apply_plan_with_context_reset"
        ? (readStringField(record, "approvedAction") as PlanApprovalAction)
        : null,
    options: options.length > 0
      ? options
      : [
          { action: "apply_plan", label: t("plan.implementAsPlan") },
          { action: "apply_plan_with_context_reset", label: t("plan.clearAndImplement") },
        ],
    planMessageId: readStringField(record, "planMessageId"),
    planRevision: readNumberField(record, "planRevision"),
    state: readStringField(record, "state") ?? "pending",
  };
}

function parseCommandComposerMetadata(value: unknown): {
  kind: "plain" | "command";
  displayText: string | null;
  effectivePrompt: string | null;
} | null {
  const record = asObjectRecord(value);
  const composer = asObjectRecord(record?.composer);
  if (!composer) {
    return null;
  }

  const kind = readStringField(composer, "kind");
  if (kind !== "command" && kind !== "plain") {
    return null;
  }

  return {
    kind,
    displayText: readStringField(composer, "displayText"),
    effectivePrompt: readStringField(composer, "effectivePrompt"),
  };
}

function parseSummaryMarkerMetadata(value: unknown): {
  kind: string | null;
  label: string | null;
  source: string | null;
} | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  return {
    kind: readStringField(record, "kind"),
    label: readStringField(record, "label"),
    source: readStringField(record, "source"),
  };
}

function parseClarifyPrompt(value: unknown): ClarifyPrompt | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  const question = readStringField(record, "question")?.trim();
  if (!question) {
    return null;
  }

  const options = Array.isArray(record.options)
    ? record.options
        .map((entry, index) => {
          const optionRecord = asObjectRecord(entry);
          const label = readStringField(optionRecord, "label")?.trim();
          const description = readStringField(optionRecord, "description")?.trim();
          if (!label || !description) {
            return null;
          }

          return {
            description,
            id: readStringField(optionRecord, "id")?.trim() || `option-${index + 1}`,
            label,
            recommended: optionRecord?.recommended === true,
          };
        })
        .filter((option): option is ClarifyOption => option !== null)
    : [];

  if (options.length < 2) {
    return null;
  }

  return {
    header: readStringField(record, "header")?.trim() || null,
    options,
    question,
  };
}

function formatPlanMetadata(
  metadata: unknown,
  fallbackContent?: string,
): FormattedPlan {
  const parsed = parsePlanMessageMetadata(metadata);
  const record = asObjectRecord(metadata);
  const title = parsed?.title?.trim() || readStringField(record, "title")?.trim() || "Execution Plan";
  const summary =
    parsed?.summary?.trim()
    || readStringField(record, "description")?.trim()
    || readStringField(record, "overview")?.trim()
    || fallbackContent?.trim()
    || "Review the proposed implementation plan before coding.";
  const stepsSource = parsed?.steps ?? [];
  const steps = stepsSource
    .map((step) => {
      const stepTitle = step.title?.trim();
      const stepDescription = step.description?.trim();
      const files = step.files?.filter((file) => file.trim().length > 0) ?? [];
      if (!stepTitle && !stepDescription && files.length === 0) {
        return null;
      }

      const fragments = [stepTitle ?? null, stepDescription ?? null, files.length > 0 ? `(${files.join(", ")})` : null]
        .filter((fragment): fragment is string => Boolean(fragment));
      return fragments.join(" — ").replace(" — (", " (");
    })
    .filter((step): step is string => Boolean(step));

  return {
    approvalState: parsed?.approvalState ?? null,
    assumptions: parsed?.assumptions ?? [],
    context: parsed?.context?.trim() ?? "",
    design: parsed?.design?.trim() ?? "",
    keyImplementation: parsed?.keyImplementation?.trim() ?? "",
    planRevision: parsed?.planRevision ?? null,
    risks: parsed?.risks ?? [],
    steps,
    summary,
    title,
    verification: parsed?.verification?.trim() ?? "",
  };
}

function renderPlanListSection(
  messageId: string,
  title: string,
  items: string[],
  ordered = false,
) {
  if (items.length === 0) {
    return null;
  }

  const ListTag = ordered ? "ol" : "ul";

  return (
    <div className="space-y-2">
      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-app-subtle">
        {title}
      </div>
      <ListTag className="space-y-1 text-sm leading-6 text-app-muted">
        {items.map((item, index) => (
          <li
            className={ordered ? "flex items-start gap-3" : undefined}
            key={`${messageId}-${title}-${index}`}
          >
            {ordered ? (
              <span className="mt-0.5 inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-app-surface-muted text-[11px] font-semibold text-app-foreground ring-1 ring-app-border/45">
                {index + 1}
              </span>
            ) : null}
            <span className="whitespace-pre-wrap">
              {ordered ? item : `- ${item}`}
            </span>
          </li>
        ))}
      </ListTag>
    </div>
  );
}

function renderPlanProseSection(title: string, content: string) {
  if (!content.trim()) {
    return null;
  }

  return (
    <div className="space-y-2">
      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-app-subtle">
        {title}
      </div>
      <div className="text-sm leading-6 text-app-muted">
        <MessageResponse>{content}</MessageResponse>
      </div>
    </div>
  );
}

function deriveSelectedRunMode(snapshot: ThreadSnapshotDto, currentMode: RunMode) {
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

function formatApprovalPromptState(state: string, approvedAction: PlanApprovalAction | null, t: (key: TranslationKey) => string) {
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

function mapSnapshotToolState(tool: ToolCallDto): SurfaceToolState {
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

function mapSnapshotToolApproval(tool: ToolCallDto): SurfaceApproval | undefined {
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

function mapSnapshotTool(tool: ToolCallDto): SurfaceToolEntry {
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

function mapSnapshotHelperStatus(
  helper: RunHelperDto,
): SurfaceHelperEntry["status"] {
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

function buildSnapshotHelperToolSummary(
  helperId: string,
  toolCalls: ReadonlyArray<ToolCallDto>,
) {
  const helperToolCalls = toolCalls.filter((tool) => tool.id.startsWith(`${helperId}:`));
  const toolCounts = helperToolCalls.reduce<Record<string, number>>((counts, tool) => {
    counts[tool.toolName] = (counts[tool.toolName] ?? 0) + 1;
    return counts;
  }, {});
  const completedSteps = helperToolCalls.filter((tool) =>
    tool.status === "completed"
    || tool.status === "failed"
    || tool.status === "denied"
    || tool.status === "cancelled",
  ).length;

  return {
    completedSteps,
    toolCounts,
    totalToolCalls: helperToolCalls.length,
  };
}

function mapSnapshotHelper(
  helper: RunHelperDto,
  toolCalls: ReadonlyArray<ToolCallDto>,
): SurfaceHelperEntry {
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

function mapSnapshotToRunState(snapshot: ThreadSnapshotDto): RunState {
  if (snapshot.activeRun) {
    switch (snapshot.activeRun.status) {
      case "waiting_approval":
        return "waiting_approval";
      case "needs_reply":
        return "needs_reply";
      case "created":
      case "dispatching":
      case "running":
      case "waiting_tool_result":
      case "cancelling":
        return "running";
      case "failed":
      case "denied":
        return "failed";
      case "cancelled":
        return "cancelled";
      case "limit_reached":
        return "limit_reached";
      case "interrupted":
        return "interrupted";
      default:
        return "completed";
    }
  }

  switch (snapshot.thread.status) {
    case "running":
      return "running";
    case "waiting_approval":
      return "waiting_approval";
    case "needs_reply":
      return snapshot.latestRun?.status === "limit_reached" ? "limit_reached" : "needs_reply";
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
    default:
      return "completed";
  }
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

function getLatestVisibleRun(snapshot: ThreadSnapshotDto) {
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

function mapRunSummaryToContextUsage(run: RunSummaryDto | null): ThreadContextUsage | null {
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

function getSnapshotRuntimeError(snapshot: ThreadSnapshotDto): SurfaceRuntimeError | null {
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

function isMoreAdvancedMessageStatus(localStatus: string, snapshotStatus: string): boolean {
  const localRank = MESSAGE_STATUS_ORDER[localStatus] ?? -1;
  const snapshotRank = MESSAGE_STATUS_ORDER[snapshotStatus] ?? -1;
  return localRank > snapshotRank;
}

function appendOrReplaceMessage(
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

function prependOlderMessages(
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

function isRenderableTimelineMessage(message: SurfaceMessage) {
  return message.messageType !== "reasoning" || message.content.trim().length > 0;
}

function updateTool(
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

function updateHelper(
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

function getApprovalReason(approval?: SurfaceApproval) {
  return approval && "reason" in approval ? approval.reason : undefined;
}

function isApprovalDenied(approval?: SurfaceApproval) {
  return Boolean(approval && "approved" in approval && approval.approved === false);
}

function applyHelperSnapshot(
  snapshot: SubagentProgressSnapshot,
): Pick<
  SurfaceHelperEntry,
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

function formatHelperKind(kind: string) {
  switch (kind) {
    case "helper_explore":
      return "Explore Agent";
    case "helper_review":
      return "Review Agent";
    default:
      return kind;
  }
}

function formatToolCallCount(count: number) {
  return `${count} tool call${count === 1 ? "" : "s"}`;
}

function formatToolStatusLabel(
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

function getToolStatusClass(state: SurfaceToolState) {
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

function isCompletedToolState(state: SurfaceToolState) {
  return (
    state === "output-available"
    || state === "output-denied"
    || state === "output-error"
  );
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
function mergeSnapshotTools(
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

function stringifyToolValue(value: unknown) {
  if (typeof value === "string") {
    return value;
  }

  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}


function asToolDataRecord(value: unknown) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

function getToolDataString(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "string" ? value : null;
}

function getToolDataNumber(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "number" ? value : null;
}

type DiffPreviewRow = {
  kind: "add" | "context" | "hunk" | "remove";
  lineNumber: number | null;
  text: string;
};

type ReadToolPresentation = {
  error: string | null;
  fileName: string;
  path: string;
  rangeLabel: string | null;
};

type QueryToolPresentation = {
  actionLabel: "Find" | "Search";
  countLabel: string | null;
  error: string | null;
  primaryLabel: string;
  scopeLabel: string | null;
};

type ListToolPresentation = {
  countLabel: string | null;
  directoryLabel: string;
  error: string | null;
  path: string;
};

type CommandOutputToolPresentation = {
  actionLabel: string;
  command: string;
  commandLanguage: "bash" | "log" | "shell";
  detailLabel: string | null;
  output: string | null;
  outputLanguage: "log";
  summaryLabel: string;
  showCommandBlock?: boolean;
  showOutputLabel?: boolean;
};

function getReadToolPresentation(tool: SurfaceToolEntry): ReadToolPresentation | null {
  if (tool.name !== "read") {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const path = getToolDataString(result, "path") ?? getToolDataString(input, "path");

  if (!path) {
    return null;
  }

  const shownLines = getToolDataNumber(result, "shownLines");
  const lineCount = getToolDataNumber(result, "lineCount");
  const startLine = getToolDataNumber(result, "offset") ?? 1;

  let rangeLabel: string | null;
  if (shownLines && shownLines > 0) {
    rangeLabel = `[${startLine}-${startLine + shownLines - 1}]`;
  } else if (lineCount && lineCount > 0) {
    rangeLabel = `[${startLine}-${lineCount}]`;
  } else {
    rangeLabel = null;
  }

  const error = tool.state === "output-error" || tool.state === "output-denied"
    ? (tool.error ?? getToolDataString(result, "error") ?? null)
    : null;

  return {
    error,
    fileName: path.split(/[\\/]/).filter(Boolean).pop() ?? path,
    path,
    rangeLabel,
  };
}

function formatToolScopeLabel(scope: string | null) {
  if (!scope) {
    return null;
  }

  const normalized = scope.replace(/\\/g, "/").replace(/\/$/, "");
  if (!normalized) {
    return null;
  }

  const leaf = normalized.split("/").filter(Boolean).pop();
  return leaf ?? normalized;
}

function normalizeSearchFilePatternLabel(pattern: string | null): string | null {
  if (!pattern) {
    return null;
  }

  const trimmed = pattern.trim();
  if (
    trimmed === "*"
    || trimmed === "**"
    || trimmed === "**/*"
    || trimmed === "./*"
    || trimmed === "./**/*"
  ) {
    return null;
  }

  return trimmed;
}

function getQueryToolPresentation(tool: SurfaceToolEntry): QueryToolPresentation | null {
  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);

  if (tool.name === "find") {
    const pattern = getToolDataString(input, "pattern") ?? getToolDataString(result, "pattern");
    if (!pattern) {
      return null;
    }

    const count = getToolDataNumber(result, "count");
    const scope = formatToolScopeLabel(
      getToolDataString(result, "directory") ?? getToolDataString(input, "path"),
    );

    return {
      actionLabel: "Find",
      countLabel: typeof count === "number" ? `${count} result${count === 1 ? "" : "s"}` : null,
      error: tool.state === "output-error" || tool.state === "output-denied"
        ? (tool.error ?? getToolDataString(result, "error") ?? null)
        : null,
      primaryLabel: pattern,
      scopeLabel: scope,
    };
  }

  if (tool.name === "search") {
    const query = getToolDataString(input, "query") ?? getToolDataString(result, "query");
    if (!query) {
      return null;
    }

    const count = getToolDataNumber(result, "count");
    const scope = formatToolScopeLabel(
      getToolDataString(result, "directory") ?? getToolDataString(input, "directory"),
    );
    const filePattern = normalizeSearchFilePatternLabel(getToolDataString(input, "filePattern"));

    return {
      actionLabel: "Search",
      countLabel: typeof count === "number" ? `${count} match${count === 1 ? "" : "es"}` : null,
      error: tool.state === "output-error" || tool.state === "output-denied"
        ? (tool.error ?? getToolDataString(result, "error") ?? null)
        : null,
      primaryLabel: filePattern ? `${query} · ${filePattern}` : query,
      scopeLabel: scope,
    };
  }

  return null;
}

function getListToolPresentation(tool: SurfaceToolEntry): ListToolPresentation | null {
  if (tool.name !== "list") {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const path = getToolDataString(result, "path") ?? getToolDataString(input, "path");

  if (!path) {
    return null;
  }

  const count = getToolDataNumber(result, "count");

  return {
    countLabel: typeof count === "number" ? `${count} item${count === 1 ? "" : "s"}` : null,
    directoryLabel: formatToolScopeLabel(path) ?? path,
    error: tool.state === "output-error" || tool.state === "output-denied"
      ? (tool.error ?? getToolDataString(result, "error") ?? null)
      : null,
    path,
  };
}

function quoteShellValue(value: string) {
  if (!value.length) {
    return "''";
  }

  if (/^[\w./:@%+=,-]+$/u.test(value)) {
    return value;
  }

  return `'${value.replace(/'/g, "'\\''")}'`;
}

function formatJoinedArgs(values: ReadonlyArray<string>) {
  return values.map((value) => quoteShellValue(value)).join(" ");
}

function getToolDataStringArray(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((entry): entry is string => typeof entry === "string" && entry.length > 0);
}

function joinTextSections(sections: Array<{ label: string; value: string | null | undefined }>) {
  const normalized = sections
    .map(({ label, value }) => {
      const trimmed = value?.trim();
      return trimmed ? `${label}:\n${trimmed}` : null;
    })
    .filter((value): value is string => Boolean(value));

  return normalized.length > 0 ? normalized.join("\n\n") : null;
}

function summarizeInlineText(value: string | null, fallback: string) {
  if (!value) {
    return fallback;
  }

  const normalized = value.replace(/\s+/g, " ").trim();
  if (!normalized) {
    return fallback;
  }

  return normalized.length > 80 ? `${normalized.slice(0, 77)}...` : normalized;
}

function formatTerminalSessionSummary(record: Record<string, unknown> | null) {
  if (!record) {
    return null;
  }

  const status = getToolDataString(record, "status");
  const shell = getToolDataString(record, "shell");
  const cwd = getToolDataString(record, "cwd");
  const cols = getToolDataNumber(record, "cols");
  const rows = getToolDataNumber(record, "rows");
  const exitCode = getToolDataNumber(record, "exitCode");

  return joinTextSections([
    { label: "status", value: status },
    { label: "shell", value: shell },
    { label: "cwd", value: cwd },
    {
      label: "size",
      value:
        typeof cols === "number" && typeof rows === "number"
          ? `${cols} x ${rows}`
          : null,
    },
    {
      label: "exit code",
      value: typeof exitCode === "number" ? String(exitCode) : null,
    },
  ]);
}

function formatTerminalDetailLabel(record: Record<string, unknown> | null) {
  if (!record) {
    return null;
  }

  const status = getToolDataString(record, "status");
  const cwd = formatToolScopeLabel(getToolDataString(record, "cwd"));

  return [status, cwd].filter(Boolean).join(" · ") || null;
}

function getShellToolPresentation(tool: SurfaceToolEntry): CommandOutputToolPresentation | null {
  if (tool.name !== "shell") {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const command = getToolDataString(result, "command") ?? getToolDataString(input, "command");
  if (!command) {
    return null;
  }

  const stdout = getToolDataString(result, "stdout");
  const stderr = getToolDataString(result, "stderr");
  const exitCode = getToolDataNumber(result, "exitCode");
  const cwd = getToolDataString(input, "cwd");
  const stdoutTruncated = result?.stdoutTruncated === true;
  const stderrTruncated = result?.stderrTruncated === true;

  const output = joinTextSections([
    { label: "stdout", value: stdout },
    { label: "stderr", value: stderr ?? tool.error },
    {
      label: "exit code",
      value:
        typeof exitCode === "number" && (exitCode !== 0 || (!stdout && !stderr))
          ? String(exitCode)
          : null,
    },
    {
      label: "note",
      value:
        stdoutTruncated || stderrTruncated
          ? "Output was truncated to keep the latest lines visible."
          : null,
    },
  ]);

  return {
    actionLabel: "Shell",
    command,
    commandLanguage: "bash",
    detailLabel: cwd ? `cwd · ${cwd}` : null,
    output,
    outputLanguage: "log",
    summaryLabel: summarizeInlineText(command, "shell"),
  };
}

function buildGitCommand(toolName: string, input: Record<string, unknown> | null) {
  const paths = getToolDataStringArray(input, "paths");

  switch (toolName) {
    case "git_add":
    case "git_stage":
      return paths.length > 0
        ? `git add -- ${formatJoinedArgs(paths)}`
        : "git add";
    case "git_unstage":
      return paths.length > 0
        ? `git restore --staged -- ${formatJoinedArgs(paths)}`
        : "git restore --staged";
    case "git_commit": {
      const message = getToolDataString(input, "message");
      return message ? `git commit -m ${quoteShellValue(message)}` : "git commit";
    }
    case "git_fetch":
      return "git fetch --prune";
    case "git_pull":
      return "git pull --ff-only";
    case "git_push":
      return "git push";
    case "git_status":
      return "git status --short";
    case "git_diff":
      return "git diff";
    case "git_log":
      return "git log --oneline";
    default:
      return toolName;
  }
}

function buildGitFallbackOutput(
  toolName: string,
  input: Record<string, unknown> | null,
  result: Record<string, unknown> | null,
) {
  const paths = getToolDataStringArray(result, "paths");
  const resolvedPaths = paths.length > 0 ? paths : getToolDataStringArray(input, "paths");

  switch (toolName) {
    case "git_add":
    case "git_stage":
      return resolvedPaths.length > 0
        ? `staged paths:\n${resolvedPaths.join("\n")}`
        : "Staged changes.";
    case "git_unstage":
      return resolvedPaths.length > 0
        ? `unstaged paths:\n${resolvedPaths.join("\n")}`
        : "Unstaged changes.";
    default:
      return null;
  }
}

function getGitToolPresentation(tool: SurfaceToolEntry): CommandOutputToolPresentation | null {
  if (!tool.name.startsWith("git_")) {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const command = buildGitCommand(tool.name, input);
  const summary = getToolDataString(result, "summary");
  const stdout = getToolDataString(result, "stdout");
  const stderr = getToolDataString(result, "stderr");

  const output =
    joinTextSections([
      { label: "summary", value: summary },
      { label: "stdout", value: stdout },
      { label: "stderr", value: stderr ?? tool.error },
    ])
    ?? buildGitFallbackOutput(tool.name, input, result)
    ?? tool.error
    ?? null;

  return {
    actionLabel: "Git",
    command,
    commandLanguage: "bash",
    detailLabel: summary,
    output,
    outputLanguage: "log",
    summaryLabel: summarizeInlineText(command, tool.name),
  };
}

function buildTerminalCommand(tool: SurfaceToolEntry, input: Record<string, unknown> | null) {
  switch (tool.name) {
    case "term_write": {
      const data = getToolDataString(input, "data") ?? getToolDataString(input, "input");
      return data ?? "term_write";
    }
    case "term_restart": {
      const cols = getToolDataNumber(input, "cols");
      const rows = getToolDataNumber(input, "rows");
      const sizeArgs = [
        typeof cols === "number" ? `--cols ${cols}` : null,
        typeof rows === "number" ? `--rows ${rows}` : null,
      ].filter(Boolean);
      return sizeArgs.length > 0 ? `term_restart ${sizeArgs.join(" ")}` : "term_restart";
    }
    default:
      return tool.name;
  }
}

function buildTerminalSummaryLabel(tool: SurfaceToolEntry, command: string) {
  switch (tool.name) {
    case "term_write":
      return summarizeInlineText(command, "terminal input");
    case "term_output":
      return "Recent terminal output";
    case "term_status":
      return "Terminal status";
    case "term_restart":
      return "Restart terminal";
    case "term_close":
      return "Close terminal";
    default:
      return summarizeInlineText(command, tool.name);
  }
}

function buildTerminalOutput(tool: SurfaceToolEntry, result: Record<string, unknown> | null) {
  if (tool.name === "term_output") {
    return getToolDataString(result, "output") ?? tool.error ?? null;
  }

  return formatTerminalSessionSummary(result) ?? tool.error ?? null;
}

function getTerminalToolPresentation(tool: SurfaceToolEntry): CommandOutputToolPresentation | null {
  if (!tool.name.startsWith("term_")) {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const command = buildTerminalCommand(tool, input);

  return {
    actionLabel: "Terminal",
    command,
    commandLanguage: tool.name === "term_write" ? "bash" : "shell",
    detailLabel:
      formatTerminalDetailLabel(result)
      ?? formatToolScopeLabel(getToolDataString(input, "cwd")),
    output: buildTerminalOutput(tool, result),
    outputLanguage: "log",
    summaryLabel: buildTerminalSummaryLabel(tool, command),
    showCommandBlock:
      tool.name !== "term_status" && tool.name !== "term_output" && tool.name !== "term_close",
    showOutputLabel:
      tool.name !== "term_status" && tool.name !== "term_output" && tool.name !== "term_close",
  };
}

function getCommandOutputToolPresentation(tool: SurfaceToolEntry) {
  return (
    getShellToolPresentation(tool)
    ?? getGitToolPresentation(tool)
    ?? getTerminalToolPresentation(tool)
  );
}

const TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS =
  "max-h-[min(50vh,28rem)] overscroll-contain";

function ToolCommandOutputBlocks({
  presentation,
}: {
  presentation: CommandOutputToolPresentation;
}) {
  return (
    <div className="space-y-3">
      {presentation.showCommandBlock !== false ? (
        <div className="space-y-1.5">
          <h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
            Command
          </h4>
          <CodeBlock
            code={presentation.command}
            contentClassName={TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS}
            language={presentation.commandLanguage}
          />
        </div>
      ) : null}
      {presentation.output ? (
        <div className="space-y-1.5">
          {presentation.showOutputLabel !== false ? (
            <h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
              Output
            </h4>
          ) : null}
          <CodeBlock
            code={presentation.output}
            contentClassName={TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS}
            language={presentation.outputLanguage}
          />
        </div>
      ) : null}
    </div>
  );
}

function parseDiffStart(value: string | undefined) {
  if (!value) {
    return 0;
  }

  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : 0;
}

function buildDiffPreviewRows(diff: string): Array<DiffPreviewRow> {
  const rows: Array<DiffPreviewRow> = [];
  let oldLine = 0;
  let newLine = 0;

  for (const line of diff.split("\n")) {
    if (!line) {
      continue;
    }

    if (line.startsWith("--- ") || line.startsWith("+++ ")) {
      continue;
    }

    if (line.startsWith("@@")) {
      const match = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      oldLine = parseDiffStart(match?.[1]);
      newLine = parseDiffStart(match?.[2]);
      rows.push({
        kind: "hunk",
        lineNumber: null,
        text: line,
      });
      continue;
    }

    if (line.startsWith("+")) {
      rows.push({
        kind: "add",
        lineNumber: newLine || null,
        text: line.slice(1),
      });
      newLine += 1;
      continue;
    }

    if (line.startsWith("-")) {
      rows.push({
        kind: "remove",
        lineNumber: oldLine || null,
        text: line.slice(1),
      });
      oldLine += 1;
      continue;
    }

    if (line.startsWith(" ")) {
      rows.push({
        kind: "context",
        lineNumber: newLine || null,
        text: line.slice(1),
      });
      oldLine += 1;
      newLine += 1;
    }
  }

  return rows;
}

function buildPlainPreviewRows(content: string): Array<DiffPreviewRow> {
  return content.split("\n").map((line, index) => ({
    kind: "context",
    lineNumber: index + 1,
    text: line,
  }));
}

function getApprovalTagLabel(
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

function getApprovalTagClass(tool: SurfaceToolEntry) {
  if (tool.state === "approval-requested") {
    return "border-app-warning/24 bg-app-warning/10 text-app-warning";
  }

  if (isApprovalDenied(tool.approval)) {
    return "border-app-danger/24 bg-app-danger/10 text-app-danger";
  }

  return "border-app-success/24 bg-app-success/10 text-app-success";
}

function FileMutationDiffPreview({
  contentPreview,
  diff,
}: {
  contentPreview: string | null;
  diff: string | null;
}) {
  const rows = useMemo(
    () => (diff ? buildDiffPreviewRows(diff) : buildPlainPreviewRows(contentPreview ?? "")),
    [contentPreview, diff],
  );

  if (rows.length === 0) {
    return null;
  }

  return (
    <div className="max-h-[22rem] overflow-auto overscroll-contain bg-app-drawer font-mono text-[12px] leading-6 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {rows.map((row, index) => (
        <div
          className={cn(
            "grid grid-cols-[56px_1fr] border-b border-app-border/55",
            row.kind === "add"
              ? "bg-app-success/10"
              : row.kind === "remove"
                ? "bg-app-danger/10"
                : row.kind === "hunk"
                  ? "bg-app-surface-muted/55"
                  : "bg-transparent",
          )}
          key={`${row.kind}-${row.lineNumber ?? "h"}-${index}`}
        >
          <span className="select-none border-r border-app-border/60 px-3 text-right text-app-subtle">
            {row.lineNumber ?? ""}
          </span>
          <span
            className={cn(
              "overflow-x-auto whitespace-pre px-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
              row.kind === "add"
                ? "text-app-success"
                : row.kind === "remove"
                  ? "text-app-danger"
                  : row.kind === "hunk"
                    ? "text-app-subtle"
                    : "text-app-foreground",
            )}
          >
            {row.text || " "}
          </span>
        </div>
      ))}
    </div>
  );
}

function formatHelperSummary(helper: SurfaceHelperEntry) {
  return [
    formatHelperName(helper),
    formatHelperDetailSummary(helper),
  ].filter(Boolean).join(" · ");
}

function formatHelperName(helper: SurfaceHelperEntry) {
  return formatHelperKind(helper.kind);
}

function formatHelperDetailSummary(helper: SurfaceHelperEntry) {
  return [
    helper.inputSummary,
    helper.totalToolCalls > 0 ? formatToolCallCount(helper.totalToolCalls) : null,
  ].filter(Boolean).join(" · ");
}

function formatHelperStatusLabel(status: SurfaceHelperEntry["status"]) {
  switch (status) {
    case "completed":
      return "done";
    case "failed":
      return "failed";
    default:
      return "running";
  }
}

function formatHelperToolCounts(toolCounts: Record<string, number>) {
  return Object.entries(toolCounts ?? {})
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([toolName, count]) => `${toolName} ${count}`);
}

function getHelperElapsedSeconds(
  helper: SurfaceHelperEntry,
  now = Date.now(),
) {
  const startedAt = new Date(helper.startedAt).getTime();
  const finishedAt = helper.finishedAt ? new Date(helper.finishedAt).getTime() : now;

  if (Number.isNaN(startedAt) || Number.isNaN(finishedAt) || finishedAt < startedAt) {
    return null;
  }

  return (finishedAt - startedAt) / 1000;
}

function formatElapsedSeconds(seconds: number | null) {
  if (seconds === null) {
    return null;
  }

  return `${seconds.toFixed(1)}s elapsed`;
}

type HelperExecutionSummaryMetrics = {
  elapsedText?: string | null;
  toolUses?: number | null;
};

function formatExecutionSummary({
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

function isHelperOwnedTool(
  toolId: string,
  helperIds: ReadonlySet<string>,
) {
  for (const helperId of helperIds) {
    if (toolId.startsWith(`${helperId}:`)) {
      return true;
    }
  }

  return false;
}

function isRuntimeOrchestrationTool(toolName: string) {
  return (
    toolName === "agent_explore"
    || toolName === "agent_review"
  );
}

function isVisibleTimelineTool(
  tool: SurfaceToolEntry,
  helperIds: ReadonlySet<string>,
) {
  if (tool.name === "clarify") {
    return tool.state === "clarify-requested";
  }

  return !isHelperOwnedTool(tool.id, helperIds) && !isRuntimeOrchestrationTool(tool.name);
}

function compareTimelineEntries(left: TimelineEntry, right: TimelineEntry) {
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
    case "thinking-placeholder":
      return 1;
    case "helper":
      return 3;
    case "tool":
      return 4;
  }
}

function shouldCompleteThinkingPhase(event: ThreadStreamEvent) {
  switch (event.type) {
    // Content arriving — replaces the thinking placeholder
    case "message_delta":
    case "message_completed":
    case "message_discarded":
    // Tool lifecycle — tool UI replaces the placeholder
    case "tool_requested":
    case "tool_running":
    case "tool_completed":
    case "tool_failed":
    case "approval_required":
    case "clarify_required":
    // Helper lifecycle — helper UI replaces the placeholder
    case "subagent_started":
    case "subagent_progress":
    case "subagent_completed":
    case "subagent_failed":
    // Terminal run states
    case "run_completed":
    case "run_failed":
    case "run_cancelled":
    case "run_interrupted":
    case "run_limit_reached":
    case "run_checkpointed":
    case "plan_updated":
      return true;
    default:
      return false;
  }
}

function getPresentationEntryRole(entry: TimelineEntry): TimelineRole {
  if (entry.kind === "message") {
    return entry.message.role;
  }

  return "assistant";
}

function getRoleSpacingClass(
  previousRole: TimelineRole | null,
  currentRole: TimelineRole,
) {
  if (!previousRole) {
    return undefined;
  }

  return previousRole === currentRole ? "pt-3" : "pt-6";
}

export function RuntimeThreadSurface({
  activeAgentProfileId,
  agentProfiles,
  commands = [],
  composerDraft = "",
  enabledSkills = [],
  initialPromptRequest = null,
  onComposerDraftChange,
  onConsumeInitialPrompt,
  onContextUsageChange,
  onRunStateChange,
  onSelectAgentProfile,
  onThreadTitleChange,
  providers,
  threadId,
  threadTitle,
  workspaceId,
}: RuntimeThreadSurfaceProps) {
  const t = useT();
  const activeProfile = useMemo(
    () => agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0] ?? null,
    [activeAgentProfileId, agentProfiles],
  );
  const [composerError, setComposerError] = useState<string | null>(null);
  const [localComposerValue, setLocalComposerValue] = useState("");
  const composerValue = onComposerDraftChange ? composerDraft : localComposerValue;
  const setComposerValue = onComposerDraftChange ? onComposerDraftChange : setLocalComposerValue;
  const [approvingPlanMessageId, setApprovingPlanMessageId] = useState<string | null>(null);
  const [helpers, setHelpers] = useState<Array<SurfaceHelperEntry>>([]);
  const [helperOpen, setHelperOpen] = useState<Record<string, boolean>>({});
  const [hasMoreMessages, setHasMoreMessages] = useState(false);
  const [historyLoadError, setHistoryLoadError] = useState<string | null>(null);
  const [isLoading, setLoading] = useState(false);
  const [isLoadingMoreMessages, setIsLoadingMoreMessages] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Array<SurfaceMessage>>([]);
  const [queueArtifact, setQueueArtifact] = useState<unknown>(null);
  const [runtimeError, setRuntimeError] = useState<SurfaceRuntimeError | null>(null);
  const [runState, setRunState] = useState<RunState>("idle");
  const [selectedRunMode, setSelectedRunMode] = useState<RunMode>("default");
  const [snapshotReady, setSnapshotReady] = useState(false);
  const [snapshotThreadId, setSnapshotThreadId] = useState<string | null>(null);

  // Reset run mode (plan toggle) when switching to a different thread so it
  // doesn't leak from one thread to another.
  const prevThreadIdRef = useRef(threadId);
  useEffect(() => {
    if (prevThreadIdRef.current !== threadId) {
      prevThreadIdRef.current = threadId;
      setSelectedRunMode("default");
    }
  }, [threadId]);
  const [thinkingPlaceholder, setThinkingPlaceholder] = useState<ThinkingPlaceholder | null>(null);
  const [tools, setTools] = useState<Array<SurfaceToolEntry>>([]);
  const [completedToolOpen, setCompletedToolOpen] = useState<Record<string, boolean>>({});
  const [taskBoards, setTaskBoards] = useState<TaskBoardState>(initialTaskBoardState);
  const previousHelperStatusesRef = useRef<Record<string, SurfaceHelperEntry["status"]>>({});
  const previousToolStatesRef = useRef<Record<string, SurfaceToolState>>({});
  const snapshotLoadRequestRef = useRef(0);
  const completedMessageResyncRequestRef = useRef(0);
  const streamRef = useRef<ThreadStream | null>(null);
  const submittingRef = useRef(false);
  const subscribingRef = useRef(false);
  const handledInitialPromptRequestIdRef = useRef<string | null>(null);
  const thinkingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const preserveContextUsageOnNextEmptySnapshotRef = useRef(false);
  const conversationContextRef = useRef<StickToBottomContext | null>(null);

  const clearScheduledThinkingPhase = useCallback(() => {
    if (thinkingTimerRef.current !== null) {
      clearTimeout(thinkingTimerRef.current);
      thinkingTimerRef.current = null;
    }
  }, []);

  const showThinkingPlaceholder = useCallback((runId?: string | null, createdAt?: string) => {
    setThinkingPlaceholder((current) => {
      if (current && current.runId === (runId ?? null)) {
        return current;
      }

      return {
        createdAt: createdAt ?? new Date().toISOString(),
        id:
          typeof crypto !== "undefined" && "randomUUID" in crypto
            ? crypto.randomUUID()
            : `thinking-${Date.now()}`,
        runId: runId ?? null,
      };
    });
  }, []);

  const scheduleThinkingPhase = useCallback((runId?: string | null, delayMs = 160) => {
    clearScheduledThinkingPhase();
    thinkingTimerRef.current = setTimeout(() => {
      thinkingTimerRef.current = null;
      showThinkingPlaceholder(runId);
    }, delayMs);
  }, [clearScheduledThinkingPhase, showThinkingPlaceholder]);

  const finalizeReasoningForRun = useCallback((runId?: string | null) => {
    setMessages((current) => {
      let changed = false;

      const next: Array<SurfaceMessage> = current.map((message) => {
        if (
          message.messageType !== "reasoning"
          || message.status !== "streaming"
          || (runId && message.runId !== runId)
        ) {
          return message;
        }

        changed = true;
        return {
          ...message,
          status: "completed",
        };
      });

      return changed ? next : current;
    });
  }, []);

  const completeThinkingPhase = useCallback((runId?: string | null) => {
    clearScheduledThinkingPhase();
    setThinkingPlaceholder((current) => {
      if (runId && current?.runId && current.runId !== runId) {
        return current;
      }

      return null;
    });
    finalizeReasoningForRun(runId);
  }, [clearScheduledThinkingPhase, finalizeReasoningForRun]);

  const appendOptimisticUserMessage = useCallback((
    content: string,
    metadata?: unknown | null,
    attachments: MessageAttachmentDto[] = [],
    showThinking = true,
  ) => {
    const userCreatedAt = new Date().toISOString();
    const localUserMessageId = `local-user-${Date.now()}`;

    setMessages((current) => {
      const withoutStaleLocal = current.filter(
        (entry) => !(entry.role === "user" && entry.id.startsWith("local-user-")),
      );

      return [
        ...withoutStaleLocal,
        {
          createdAt: userCreatedAt,
          id: localUserMessageId,
          messageType: "plain_message",
          metadata: metadata ?? null,
          attachments,
          role: "user",
          runId: null,
          content,
          status: "completed",
        },
      ];
    });

    if (showThinking) {
      showThinkingPlaceholder(null, userCreatedAt);
    }
  }, [showThinkingPlaceholder]);

  const loadSnapshot = useCallback(async () => {
    const requestId = snapshotLoadRequestRef.current + 1;
    snapshotLoadRequestRef.current = requestId;

    if (!threadId) {
      preserveContextUsageOnNextEmptySnapshotRef.current = false;
      subscribingRef.current = false;
      clearScheduledThinkingPhase();
      setHasMoreMessages(false);
      setHistoryLoadError(null);
      setMessages([]);
      setLoadError(null);
      setLoading(false);
      setIsLoadingMoreMessages(false);
      onContextUsageChange?.(null);
      setApprovingPlanMessageId(null);
      setRuntimeError(null);
      setRunState("idle");
      setSnapshotReady(true);
      setSnapshotThreadId(null);
      setThinkingPlaceholder(null);
      onRunStateChange?.("idle");
      return;
    }

    setLoading(true);
    setHistoryLoadError(null);
    setLoadError(null);

    try {
      const snapshot = await threadLoad(threadId);
      if (snapshotLoadRequestRef.current !== requestId) {
        return;
      }

      const nextState = mapSnapshotToRunState(snapshot);
      const snapshotMessages = snapshot.messages.map(mapSnapshotMessage);
      const latestVisibleRun = getLatestVisibleRun(snapshot);
      const nextContextUsage = mapRunSummaryToContextUsage(latestVisibleRun);
      const shouldPreserveContextUsage =
        preserveContextUsageOnNextEmptySnapshotRef.current
        && (nextContextUsage === null || nextContextUsage.totalTokens === 0);
      if (!shouldPreserveContextUsage) {
        // Clear the flag only when we have valid usage or it was never set.
        // This prevents premature clearing when stream_resync_required triggers
        // loadSnapshot multiple times before a run has usage info, and also
        // avoids a brief "0" flash when a new run exists but hasn't received
        // its first API response yet.
        preserveContextUsageOnNextEmptySnapshotRef.current = false;
      }
      // Use snapshot as the base but preserve any live-streamed message that
      // the snapshot hasn't caught up with yet.  This prevents a stale snapshot
      // (loaded while the DB write is still in-flight) from overwriting a
      // message that the user already saw streaming.
      setMessages((currentMessages) => {
        if (currentMessages.length === 0) {
          return snapshotMessages;
        }

        // Start from the snapshot list, then merge any local messages that
        // are more "advanced" than the snapshot version or completely absent.
        const merged = snapshotMessages.slice();
        for (const localMsg of currentMessages) {
          const snapshotIdx = merged.findIndex((m) => m.id === localMsg.id);
          if (snapshotIdx === -1) {
            // Message exists locally but not in snapshot — keep it when it
            // has meaningful content (streaming or completed assistant reply).
            if (
              localMsg.role === "assistant"
              && (localMsg.status === "streaming" || localMsg.status === "completed")
              && localMsg.content.length > 0
            ) {
              merged.push(localMsg);
            }
          } else if (
            isMoreAdvancedMessageStatus(localMsg.status, merged[snapshotIdx].status)
          ) {
            merged[snapshotIdx] = localMsg;
          } else if (
            merged[snapshotIdx].status === localMsg.status
            && merged[snapshotIdx].role === "assistant"
            && merged[snapshotIdx].content.length === 0
            && localMsg.content.length > 0
          ) {
            // Snapshot has same status but empty content while local has
            // content — keep the local version (DB write not yet committed).
            merged[snapshotIdx] = localMsg;
          }
        }
        return merged;
      });
      setHasMoreMessages(snapshot.hasMoreMessages);
      setApprovingPlanMessageId(null);
      setTools((currentTools) => {
        const snapshotTools = (snapshot.toolCalls ?? []).map(mapSnapshotTool);
        return mergeSnapshotTools(snapshotTools, currentTools);
      });
      setHelpers((snapshot.helpers ?? []).map((helper) => mapSnapshotHelper(helper, snapshot.toolCalls ?? [])));
      setTaskBoards(taskBoardsFromSnapshot(snapshot.taskBoards ?? [], snapshot.activeTaskBoardId ?? null));
      setRuntimeError(getSnapshotRuntimeError(snapshot));
      setRunState(nextState);
      setSelectedRunMode((current) => deriveSelectedRunMode(snapshot, current));
      if (!shouldPreserveContextUsage) {
        onContextUsageChange?.(nextContextUsage);
      }
      setSnapshotReady(true);
      setSnapshotThreadId(threadId);
      if (nextState === "running") {
        // Preserve (or restore) the thinking placeholder while the run is
        // still active — the LLM may be mid-generation and we don't want the
        // placeholder to vanish just because loadSnapshot was triggered (e.g.
        // by stream_resync_required or plan approval).
        showThinkingPlaceholder(latestVisibleRun?.id ?? null);
      } else {
        setThinkingPlaceholder(null);
      }
      if (
        (nextState === "running" || nextState === "waiting_approval" || nextState === "needs_reply")
        && streamRef.current
        && !streamRef.current.runId
        && !subscribingRef.current
      ) {
        subscribingRef.current = true;
        void streamRef.current.subscribe(threadId)
          .finally(() => {
            subscribingRef.current = false;
          });
      }
      if (snapshot.thread.title.trim()) {
        onThreadTitleChange?.(snapshot.thread.id, snapshot.thread.title.trim());
      }
      onRunStateChange?.(nextState);
    } catch (error) {
      if (snapshotLoadRequestRef.current !== requestId) {
        return;
      }

      preserveContextUsageOnNextEmptySnapshotRef.current = false;
      const message = error instanceof Error ? error.message : String(error);
      setLoadError(message);
      onContextUsageChange?.(null);
      setSnapshotReady(true);
      setSnapshotThreadId(threadId);
    } finally {
      if (snapshotLoadRequestRef.current === requestId) {
        setLoading(false);
      }
    }
  }, [clearScheduledThinkingPhase, onContextUsageChange, onRunStateChange, onThreadTitleChange, showThinkingPlaceholder, threadId]);

  const loadOlderMessages = useCallback(async () => {
    if (!threadId || isLoadingMoreMessages || messages.length === 0 || !hasMoreMessages) {
      return;
    }

    const oldestMessageId = messages[0]?.id;
    if (!oldestMessageId) {
      return;
    }

    setHistoryLoadError(null);
    setIsLoadingMoreMessages(true);

    try {
      const snapshot = await threadLoad(threadId, oldestMessageId);
      const olderMessages = snapshot.messages.map(mapSnapshotMessage);
      setMessages((current) => prependOlderMessages(current, olderMessages));
      setHasMoreMessages(snapshot.hasMoreMessages);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setHistoryLoadError(message);
    } finally {
      setIsLoadingMoreMessages(false);
    }
  }, [hasMoreMessages, isLoadingMoreMessages, messages, threadId]);

  const resyncCompletedMessage = useCallback(async (messageId: string, runId: string) => {
    if (!threadId) {
      return;
    }

    const requestId = completedMessageResyncRequestRef.current + 1;
    completedMessageResyncRequestRef.current = requestId;

    try {
      const snapshot = await threadLoad(threadId);
      if (
        completedMessageResyncRequestRef.current !== requestId
        || snapshot.thread.id !== threadId
      ) {
        return;
      }

      const persistedMessage = snapshot.messages.find((message) => message.id === messageId);
      if (!persistedMessage) {
        return;
      }

      const mappedMessage = mapSnapshotMessage(persistedMessage);
      setMessages((current) => appendOrReplaceMessage(current, mappedMessage));

      const nextState = mapSnapshotToRunState(snapshot);
      setTools((currentTools) => {
        const snapshotTools = (snapshot.toolCalls ?? []).map(mapSnapshotTool);
        return mergeSnapshotTools(snapshotTools, currentTools);
      });
      setHelpers((snapshot.helpers ?? []).map((helper) => mapSnapshotHelper(helper, snapshot.toolCalls ?? [])));
      setTaskBoards(taskBoardsFromSnapshot(snapshot.taskBoards ?? [], snapshot.activeTaskBoardId ?? null));
      setRuntimeError(getSnapshotRuntimeError(snapshot));
      setRunState(nextState);
      setSelectedRunMode((current) => deriveSelectedRunMode(snapshot, current));

      const latestVisibleRun = getLatestVisibleRun(snapshot);
      if (latestVisibleRun?.id === runId) {
        const nextContextUsage = mapRunSummaryToContextUsage(latestVisibleRun);
        if (nextContextUsage) {
          onContextUsageChange?.(nextContextUsage);
        }
      }

      if (snapshot.thread.title.trim()) {
        onThreadTitleChange?.(snapshot.thread.id, snapshot.thread.title.trim());
      }
      onRunStateChange?.(nextState);
    } catch {
      // Keep the local completed fallback message if snapshot resync is not ready yet.
    }
  }, [onContextUsageChange, onRunStateChange, onThreadTitleChange, threadId]);

  useEffect(() => {
    subscribingRef.current = false;
    setComposerError(null);
    if (!onComposerDraftChange) {
      setLocalComposerValue("");
    }
    setHelpers([]);
    setHasMoreMessages(false);
    setHistoryLoadError(null);
    setLoadError(null);
    setMessages([]);
    setIsLoadingMoreMessages(false);
    setApprovingPlanMessageId(null);
    setQueueArtifact(null);
    setRuntimeError(null);
    setRunState("idle");
    setSnapshotReady(false);
    setSnapshotThreadId(null);
    clearScheduledThinkingPhase();
    setThinkingPlaceholder(null);
    setTools([]);
    void loadSnapshot();
  }, [clearScheduledThinkingPhase, loadSnapshot, threadId]);

  useEffect(() => {
    if (!threadId) {
      streamRef.current = null;
      return;
    }

    const stream = new ThreadStream();
    const withActiveStream = <Args extends unknown[]>(
      handler: (...args: Args) => void,
    ) => (...args: Args) => {
      if (streamRef.current !== stream) {
        return;
      }
      handler(...args);
    };

    stream.onRawEvent = withActiveStream((event) => {
      if (shouldCompleteThinkingPhase(event)) {
        completeThinkingPhase(event.runId);
      }

      if (event.type === "run_started") {
        setApprovingPlanMessageId(null);
        if (event.runMode === "default" || event.runMode === "plan") {
          setSelectedRunMode(event.runMode);
        }
      }

      if (event.type === "stream_resync_required") {
        void loadSnapshot();
      }

      if (event.type === "message_discarded") {
        setMessages((current) =>
          current.map((message) => (
            message.id === event.messageId
              ? { ...message, status: "discarded" }
              : message
          )),
        );
      }
    });

    stream.onMessage = withActiveStream((event) => {
      if (event.kind === "delta") {
        setMessages((current) =>
          appendOrReplaceMessage(current, {
            createdAt:
              current.find((entry) => entry.id === event.messageId)?.createdAt
              ?? new Date().toISOString(),
            id: event.messageId,
            messageType: "plain_message",
            attachments: [],
            role: "assistant",
            runId: event.runId,
            content:
              current.find((entry) => entry.id === event.messageId)?.content.concat(event.delta ?? "")
              ?? (event.delta ?? ""),
            status: "streaming",
          }),
        );
        return;
      }

      setMessages((current) =>
        appendOrReplaceMessage(current, {
          createdAt:
            current.find((entry) => entry.id === event.messageId)?.createdAt
            ?? new Date().toISOString(),
          id: event.messageId,
          messageType: "plain_message",
          attachments: [],
          role: "assistant",
          runId: event.runId,
          content: event.content ?? "",
          status: "completed",
        }),
      );

      void resyncCompletedMessage(event.messageId, event.runId);
      showThinkingPlaceholder(event.runId);
    });

    stream.onPlan = withActiveStream((event) => {
      scheduleThinkingPhase(event.runId);
    });

    stream.onReasoning = withActiveStream((event) => {
      setThinkingPlaceholder(null);
      scheduleThinkingPhase(event.runId);
      const reasoningMessageId = event.messageId ?? `reasoning-${event.runId}`;
      setMessages((current) =>
        appendOrReplaceMessage(
          current.map((message) => {
            if (
              message.id === reasoningMessageId
              || message.messageType !== "reasoning"
              || message.status !== "streaming"
              || message.runId !== event.runId
            ) {
              return message;
            }

            return {
              ...message,
              status: "completed",
            };
          }),
          {
            createdAt:
              current.find((entry) => entry.id === reasoningMessageId)?.createdAt
              ?? new Date().toISOString(),
            id: reasoningMessageId,
            messageType: "reasoning",
            attachments: [],
            role: "assistant",
            runId: event.runId,
            content: event.reasoning,
            status: "streaming",
          },
        ),
      );
    });

    stream.onQueue = withActiveStream((event: QueueEvent) => {
      setQueueArtifact(event.queue);
    });

    stream.onTaskBoard = withActiveStream((event: { taskBoard: TaskBoardDto }) => {
      setTaskBoards((current) => applyTaskBoardUpdate(current, event.taskBoard));
    });

    stream.onThreadTitle = withActiveStream((event: ThreadTitleEvent) => {
      onThreadTitleChange?.(event.threadId, event.title);
    });

    stream.onUsage = withActiveStream((event: UsageEvent) => {
      onContextUsageChange?.({
        contextWindow: event.contextWindow,
        inputTokens: event.usage.inputTokens,
        outputTokens: event.usage.outputTokens,
        cacheReadTokens: event.usage.cacheReadTokens,
        cacheWriteTokens: event.usage.cacheWriteTokens,
        totalTokens: event.usage.totalTokens,
        modelDisplayName: event.modelDisplayName,
        runId: event.runId,
      });
    });

    stream.onHelperEvent = withActiveStream((event: HelperEvent) => {
      if (event.kind === "completed" || event.kind === "failed") {
        scheduleThinkingPhase(event.runId);
      }

      setHelpers((current) => {
        switch (event.kind) {
          case "started":
            return updateHelper(current, event.subtaskId, (entry) => ({
              ...applyHelperSnapshot(event.snapshot),
              error: undefined,
              finishedAt: entry?.finishedAt ?? null,
              id: event.subtaskId,
              inputSummary: entry?.inputSummary,
              kind: event.helperKind,
              latestMessage: undefined,
              runId: event.runId,
              startedAt: entry?.startedAt ?? event.startedAt,
              status: "running",
              summary: entry?.summary,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
          case "progress":
            return updateHelper(current, event.subtaskId, (entry) => ({
              ...applyHelperSnapshot(event.snapshot),
              error: entry?.error,
              finishedAt: entry?.finishedAt ?? null,
              id: event.subtaskId,
              inputSummary: entry?.inputSummary,
              kind: event.helperKind,
              latestMessage: event.message,
              runId: event.runId,
              startedAt: entry?.startedAt ?? event.startedAt,
              status: entry?.status ?? "running",
              summary: entry?.summary,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
          case "completed":
            return updateHelper(current, event.subtaskId, (_entry) => ({
              ...applyHelperSnapshot(event.snapshot),
              error: undefined,
              finishedAt: new Date().toISOString(),
              id: event.subtaskId,
              inputSummary: _entry?.inputSummary,
              kind: event.helperKind,
              latestMessage: undefined,
              runId: event.runId,
              startedAt: _entry?.startedAt ?? event.startedAt,
              status: "completed",
              summary: event.summary,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
          case "failed":
            return updateHelper(current, event.subtaskId, (_entry) => ({
              ...applyHelperSnapshot(event.snapshot),
              error: event.error,
              finishedAt: new Date().toISOString(),
              id: event.subtaskId,
              inputSummary: _entry?.inputSummary,
              kind: event.helperKind,
              latestMessage: undefined,
              runId: event.runId,
              startedAt: _entry?.startedAt ?? event.startedAt,
              status: "failed",
              summary: undefined,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
        }
      });
    });

    stream.onToolEvent = withActiveStream((event) => {
      if (event.kind === "completed" || event.kind === "failed") {
        scheduleThinkingPhase(event.runId);
      }

      setTools((current) => {
        switch (event.kind) {
          case "requested":
            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: undefined,
              finishedAt: entry?.finishedAt ?? null,
              id: event.toolCallId,
              input: event.toolInput,
              name: event.toolName ?? entry?.name ?? "tool",
              result: entry?.result,
              runId: event.runId,
              startedAt: entry?.startedAt ?? new Date().toISOString(),
              state: entry?.state === "approval-requested" ? "approval-requested" : "input-streaming",
            }));
          case "clarify-required":
            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: undefined,
              finishedAt: null,
              id: event.toolCallId,
              input: event.toolInput ?? entry?.input,
              name: event.toolName ?? entry?.name ?? "tool",
              result: undefined,
              runId: event.runId,
              startedAt: entry?.startedAt ?? new Date().toISOString(),
              state: "clarify-requested",
            }));
          case "clarify-resolved":
            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: undefined,
              finishedAt: new Date().toISOString(),
              id: event.toolCallId,
              input: entry?.input,
              name: entry?.name ?? "tool",
              result: event.response,
              runId: event.runId,
              startedAt: entry?.startedAt ?? new Date().toISOString(),
              state: "output-available",
            }));
          case "running":
            return updateTool(current, event.toolCallId, (entry) => {
              if (entry && isCompletedToolState(entry.state)) {
                return entry;
              }

              // Preserve approval-requested state — the tool_running event
              // can arrive after approval_required has already set the state,
              // so we must not regress it to input-available.
              if (entry?.state === "approval-requested") {
                return entry;
              }

              return {
                approval: entry?.approval,
                error: undefined,
                finishedAt: entry?.finishedAt ?? null,
                id: event.toolCallId,
                input: entry?.input,
                name: entry?.name ?? "tool",
                result: undefined,
                runId: event.runId,
                startedAt: entry?.startedAt ?? new Date().toISOString(),
                state: "input-available",
              };
            });
          case "completed":
            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: undefined,
              finishedAt: new Date().toISOString(),
              id: event.toolCallId,
              input: entry?.input,
              name: entry?.name ?? "tool",
              result: event.result,
              runId: event.runId,
              startedAt: entry?.startedAt ?? new Date().toISOString(),
              state: "output-available",
            }));
          case "failed": {
            const denied =
              isApprovalDenied(current.find((entry) => entry.id === event.toolCallId)?.approval)
              || event.error?.toLowerCase().includes("denied");

            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: event.error,
              finishedAt: new Date().toISOString(),
              id: event.toolCallId,
              input: entry?.input,
              name: entry?.name ?? "tool",
              result: undefined,
              runId: event.runId,
              startedAt: entry?.startedAt ?? new Date().toISOString(),
              state: denied ? "output-denied" : "output-error",
            }));
          }
        }
      });
    });

    stream.onApproval = withActiveStream((event) => {
      if (event.kind === "resolved" && event.approved) {
        scheduleThinkingPhase(event.runId);
      }
      setTools((current) =>
        updateTool(current, event.toolCallId, (entry) => ({
          approval:
            event.kind === "required"
              ? {
                  id: event.toolCallId,
                }
              : event.approved
                ? {
                    approved: true,
                    id: event.toolCallId,
                    reason: event.reason ?? getApprovalReason(entry?.approval),
                  }
                : {
                    approved: false,
                    id: event.toolCallId,
                    reason: event.reason ?? getApprovalReason(entry?.approval),
                  },
          error: entry?.error,
          finishedAt: entry?.finishedAt ?? null,
          id: event.toolCallId,
          input: event.toolInput ?? entry?.input,
          name: event.toolName ?? entry?.name ?? "tool",
          result: entry?.result,
          runId: event.runId,
          startedAt: entry?.startedAt ?? new Date().toISOString(),
          state: event.kind === "required" ? "approval-requested" : "approval-responded",
        })),
      );
    });

    stream.onRunStateChange = withActiveStream((state, runId) => {
      setRunState(state);
      onRunStateChange?.(state);

      if (state === "running" || state === "waiting_approval" || state === "needs_reply") {
        setRuntimeError(null);
      }

      if (
        state === "completed"
        || state === "failed"
        || state === "cancelled"
        || state === "interrupted"
        || state === "limit_reached"
      ) {
        completeThinkingPhase(runId);
      }

      if (state === "running") {
        return;
      }

      if ((state === "waiting_approval" || state === "needs_reply") && !stream.runId) {
        void loadSnapshot();
        return;
      }

      if (
        state === "completed"
        || state === "failed"
        || state === "cancelled"
        || state === "interrupted"
        || state === "limit_reached"
      ) {
        void loadSnapshot();
      }
    });

    stream.onError = withActiveStream((message, runId) => {
      setApprovingPlanMessageId(null);
      if (runId) {
        setRuntimeError({
          message,
          runId,
        });
        return;
      }

      setComposerError(message);
    });

    streamRef.current = stream;
    return () => {
      streamRef.current = null;
      subscribingRef.current = false;
      clearScheduledThinkingPhase();
      stream.dispose();
    };
  }, [
    clearScheduledThinkingPhase,
    completeThinkingPhase,
    loadSnapshot,
    onRunStateChange,
    onContextUsageChange,
    onThreadTitleChange,
    resyncCompletedMessage,
    scheduleThinkingPhase,
    threadId,
  ]);

  // Global "thread-run-finished" listener acts as a safety net.
  // If the per-stream `onRunStateChange` callback misses a terminal event
  // (e.g. the stream was disposed during an effect re-run or the broadcast
  // channel lagged), this listener will still fire because it is emitted as
  // a Tauri app-wide event, independent of the broadcast channel.
  useEffect(() => {
    if (!threadId) {
      return;
    }

    const setup = listen<{ threadId: string; runId: string; status: string }>(
      "thread-run-finished",
      (event) => {
        if (event.payload.threadId !== threadId) {
          return;
        }

        // Only reload if we still think the run is active — avoids
        // unnecessary snapshot loads when the stream already handled it.
        setRunState((current) => {
          if (current === "running" || current === "waiting_approval" || current === "needs_reply") {
            void loadSnapshot();
          }

          return current;
        });
      },
    );

    return () => {
      setup.then((fn) => fn());
    };
  }, [loadSnapshot, threadId]);

  const submitPrompt = useCallback(async (
    submissionOrPrompt: ComposerSubmission | string,
    runModeOverride?: RunMode,
  ) => {
    if (!threadId) {
      setComposerError("This thread is still preparing. Try again in a moment.");
      return;
    }

    const submission = typeof submissionOrPrompt === "string"
      ? {
          kind: "plain" as const,
          displayText: submissionOrPrompt,
          effectivePrompt: submissionOrPrompt,
          rawMessage: { text: submissionOrPrompt, files: [] },
          attachments: [],
          metadata: null,
          runMode: runModeOverride,
        }
      : submissionOrPrompt;
    const prompt = submission.effectivePrompt.trim();

    if (!prompt) {
      setComposerError("Type a prompt before starting a run.");
      return;
    }

    if (!activeProfile) {
      setComposerError("Select an agent profile with an enabled model before starting a run.");
      return;
    }

    const activeRunId = streamRef.current?.runId ?? null;
    if (runState === "running" || (runState === "waiting_approval" && activeRunId)) {
      setComposerError("This thread already has an active run.");
      return;
    }

    if (runState === "needs_reply" && activeRunId) {
      setComposerError("Reply to the pending question before starting a new run.");
      return;
    }

    // Guard against concurrent invocations. The `initialPromptRequest` effect
    // may re-fire while an `await startRun()` is still in flight because
    // `runState` hasn't transitioned to "running" yet (it only changes when
    // the Rust backend sends back a `run_started` event). Without this ref
    // guard, a second `startRun` invoke reaches Rust where the first run is
    // already registered in `active_runs`, producing `thread.run.already_active`.
    if (submittingRef.current) {
      return;
    }
    submittingRef.current = true;

    const modelPlan = buildRunModelPlanFromSelection(
      activeAgentProfileId,
      agentProfiles,
      providers,
    );

    if (!modelPlan) {
      submittingRef.current = false;
      setComposerError("Select an enabled primary model for the current profile before starting a run.");
      return;
    }

    setComposerError(null);
    setRuntimeError(null);
    setQueueArtifact(null);

    if (submission.kind === "command" && submission.command?.behavior === "clear") {
      appendOptimisticUserMessage(submission.displayText, submission.metadata ?? null, [], false);
      conversationContextRef.current?.scrollToBottom();
      try {
        preserveContextUsageOnNextEmptySnapshotRef.current = false;
        onContextUsageChange?.(null);
        await threadClearContext(threadId);
        await loadSnapshot();
      } finally {
        submittingRef.current = false;
      }
      return;
    }

    if (submission.kind === "command" && submission.command?.behavior === "compact") {
      appendOptimisticUserMessage(submission.displayText, submission.metadata ?? null, [], false);
      conversationContextRef.current?.scrollToBottom();
      try {
        preserveContextUsageOnNextEmptySnapshotRef.current = false;
        onContextUsageChange?.(null);
        await threadCompactContext(
          threadId,
          submission.command.argumentsText || null,
          modelPlan,
        );
        await loadSnapshot();
      } finally {
        submittingRef.current = false;
      }
      return;
    }

    appendOptimisticUserMessage(
      submission.displayText,
      submission.metadata ?? null,
      submission.attachments,
    );

    // Scroll to bottom when sending a new message to ensure the conversation
    // follows the new content even if the user had scrolled up previously.
    conversationContextRef.current?.scrollToBottom();

    try {
      await streamRef.current?.startRun(
        threadId,
        {
          prompt,
          displayPrompt: submission.displayText,
          promptMetadata: submission.metadata ?? null,
          attachments: submission.attachments,
        },
        runModeOverride ?? submission.runMode ?? selectedRunMode,
        modelPlan,
      );
    } catch (error) {
      setThinkingPlaceholder(null);
      throw error;
    } finally {
      submittingRef.current = false;
    }
  }, [activeAgentProfileId, activeProfile, agentProfiles, appendOptimisticUserMessage, loadSnapshot, onContextUsageChange, providers, runState, selectedRunMode, threadId]);

  const respondToClarify = useCallback(async (
    tool: SurfaceToolEntry,
    response: Record<string, unknown>,
    displayText: string,
  ) => {
    if (!streamRef.current) {
      return;
    }

    setComposerError(null);
    setRuntimeError(null);
    setQueueArtifact(null);
    appendOptimisticUserMessage(displayText, null, []);
    conversationContextRef.current?.scrollToBottom();

    try {
      await streamRef.current.respondToClarify(tool.id, response);
    } catch {
      setThinkingPlaceholder(null);
    }
  }, [appendOptimisticUserMessage]);

  useEffect(() => {
    const isCurrentThreadSnapshotReady =
      snapshotReady && snapshotThreadId === threadId;
    const initialPromptRequestId = initialPromptRequest?.id ?? null;
    const hasBlockingRun =
      runState === "running"
      || ((runState === "waiting_approval" || runState === "needs_reply") && Boolean(streamRef.current?.runId));

    if (
      !initialPromptRequest
      || initialPromptRequest.threadId !== threadId
      || !isCurrentThreadSnapshotReady
      || hasBlockingRun
      || handledInitialPromptRequestIdRef.current === initialPromptRequestId
    ) {
      return;
    }

    // Parent state clears this request asynchronously, so mark it handled
    // before awaiting to keep effect re-runs from starting the same run twice.
    handledInitialPromptRequestIdRef.current = initialPromptRequestId;
    if (initialPromptRequest.runMode) {
      setSelectedRunMode(initialPromptRequest.runMode);
    }
    void submitPrompt({
      kind: "plain",
      displayText: initialPromptRequest.displayText,
      effectivePrompt: initialPromptRequest.effectivePrompt,
      rawMessage: { text: initialPromptRequest.displayText, files: [] },
      attachments: initialPromptRequest.attachments,
      metadata: initialPromptRequest.metadata,
      runMode: initialPromptRequest.runMode,
    }, initialPromptRequest.runMode)
      .finally(() => {
        onConsumeInitialPrompt?.(initialPromptRequest.id);
      });
  }, [initialPromptRequest, onConsumeInitialPrompt, runState, snapshotReady, snapshotThreadId, submitPrompt, threadId]);

  const hasLiveRun =
    runState === "running"
    || (runState === "waiting_approval" && Boolean(streamRef.current?.runId));
  const composerStatus: ChatStatus = hasLiveRun ? "streaming" : "ready";
  const helperIds = useMemo(
    () => new Set(helpers.map((helper) => helper.id)),
    [helpers],
  );
  const visibleTools = useMemo(
    () => tools.filter((tool) => isVisibleTimelineTool(tool, helperIds)),
    [helperIds, tools],
  );
  const pendingClarifyTool = useMemo(
    () =>
      tools.find(
        (tool) => tool.name === "clarify" && tool.state === "clarify-requested",
      ) ?? null,
    [tools],
  );
  const hasRuntimeArtifacts =
    Boolean(runtimeError)
    || Boolean(queueArtifact)
    || helpers.length > 0
    || visibleTools.length > 0
    || Boolean(taskBoards.activeBoard);
  const timelineEntries = useMemo<Array<TimelineEntry>>(
    () =>
      [
        ...messages.filter(isRenderableTimelineMessage).map((message) => ({
          kind: "message" as const,
          key: `message:${message.id}`,
          occurredAt: message.createdAt,
          message,
        })),
        ...(thinkingPlaceholder
          ? [{
              kind: "thinking-placeholder" as const,
              key: `thinking-placeholder:${thinkingPlaceholder.id}`,
              occurredAt: thinkingPlaceholder.createdAt,
            }]
          : []),
        ...helpers.map((helper) => ({
          kind: "helper" as const,
          key: `helper:${helper.id}`,
          occurredAt: helper.startedAt,
          helper,
        })),
        ...visibleTools.map((tool) => ({
          kind: "tool" as const,
          key: `tool:${tool.id}`,
          occurredAt: tool.startedAt,
          tool,
        })),
      ].sort(compareTimelineEntries),
    [helpers, messages, thinkingPlaceholder, visibleTools],
  );
  const presentationEntries = timelineEntries;
  const lastPresentationRole = presentationEntries.length > 0
    ? getPresentationEntryRole(presentationEntries[presentationEntries.length - 1])
    : null;
  const queuePreviousRole: TimelineRole | null = lastPresentationRole;
  const runtimeErrorPreviousRole: TimelineRole | null = queueArtifact ? "assistant" : lastPresentationRole;

  useEffect(() => {
    const previousToolStates = previousToolStatesRef.current;
    const nextToolStates = Object.fromEntries(visibleTools.map((tool) => [tool.id, tool.state]));

    setCompletedToolOpen((current) => {
      const next: Record<string, boolean> = {};

      for (const tool of visibleTools) {
        const previousState = previousToolStates[tool.id];

        if (previousState !== tool.state) {
          next[tool.id] = !isCompletedToolState(tool.state);
          continue;
        }

        if (tool.id in current) {
          next[tool.id] = current[tool.id];
          continue;
        }

        next[tool.id] = !isCompletedToolState(tool.state);
      }

      const currentKeys = Object.keys(current);
      const nextKeys = Object.keys(next);
      if (currentKeys.length !== nextKeys.length) {
        return next;
      }

      for (const key of nextKeys) {
        if (current[key] !== next[key]) {
          return next;
        }
      }

      return current;
    });

    previousToolStatesRef.current = nextToolStates;
  }, [visibleTools]);

  useEffect(() => {
    const previousHelperStatuses = previousHelperStatusesRef.current;
    const nextHelperStatuses = Object.fromEntries(
      helpers.map((helper) => [helper.id, helper.status]),
    );

    setHelperOpen((current) => {
      const next: Record<string, boolean> = {};

      for (const helper of helpers) {
        const previousStatus = previousHelperStatuses[helper.id];
        const preferredOpen = helper.status !== "completed";

        if (previousStatus !== helper.status) {
          next[helper.id] = preferredOpen;
          continue;
        }

        if (helper.id in current) {
          next[helper.id] = current[helper.id];
          continue;
        }

        next[helper.id] = preferredOpen;
      }

      const currentKeys = Object.keys(current);
      const nextKeys = Object.keys(next);
      if (currentKeys.length !== nextKeys.length) {
        return next;
      }

      for (const key of nextKeys) {
        if (current[key] !== next[key]) {
          return next;
        }
      }

      return current;
    });

    previousHelperStatusesRef.current = nextHelperStatuses;
  }, [helpers]);

  const handleSubmit = useCallback(async (submission: ComposerSubmission) => {
    const prompt = submission.effectivePrompt?.trim() ?? "";
    if (!prompt) {
      return;
    }

    setComposerValue("");
    if (pendingClarifyTool) {
      await respondToClarify(
        pendingClarifyTool,
        {
          kind: "freeform",
          text: prompt,
        },
        submission.displayText || prompt,
      );
      return;
    }

    await submitPrompt(submission);
  }, [pendingClarifyTool, respondToClarify, submitPrompt]);

  const handleCompletedToolOpenChange = useCallback((toolId: string, open: boolean) => {
    setCompletedToolOpen((current) => (current[toolId] === open ? current : { ...current, [toolId]: open }));
  }, []);

  const handleHelperOpenChange = useCallback((helperId: string, open: boolean) => {
    setHelperOpen((current) => (current[helperId] === open ? current : { ...current, [helperId]: open }));
  }, []);

  const handlePlanApproval = useCallback(async (
    messageId: string,
    action: PlanApprovalAction,
  ) => {
    if (!threadId || !streamRef.current) {
      return;
    }

    preserveContextUsageOnNextEmptySnapshotRef.current = action === "apply_plan";
    if (action === "apply_plan_with_context_reset") {
      onContextUsageChange?.(null);
    }
    setApprovingPlanMessageId(messageId);
    setComposerError(null);
    setRuntimeError(null);
    showThinkingPlaceholder(null, new Date().toISOString());

    try {
      await streamRef.current.executeApprovedPlan(threadId, messageId, action);
      await loadSnapshot();
      setMessages((current) => {
        const approvalPrompt = parseApprovalPromptMetadata(
          current.find((message) => message.id === messageId)?.metadata, t,
        );

        return current.map((message) => {
          if (message.id === messageId) {
            const metadata = asObjectRecord(message.metadata);
            return {
              ...message,
              metadata: {
                ...(metadata ?? {}),
                approvedAction: action,
                state: "approved",
              },
            };
          }

          if (approvalPrompt?.planMessageId && message.id === approvalPrompt.planMessageId) {
            const metadata = asObjectRecord(message.metadata);
            return {
              ...message,
              metadata: {
                ...(metadata ?? {}),
                approvalState: "approved",
              },
            };
          }

          return message;
        });
      });
    } catch {
      preserveContextUsageOnNextEmptySnapshotRef.current = false;
      setThinkingPlaceholder(null);
    } finally {
      setApprovingPlanMessageId((current) => (current === messageId ? null : current));
    }
  }, [loadSnapshot, showThinkingPlaceholder, threadId]);

  const renderToolEntry = useCallback((tool: SurfaceToolEntry, key: string, inset = false) => {
    const clarifyPrompt = parseClarifyPrompt(tool.input);
    const fileMutation = getFileMutationPresentation(tool);
    const readTool = getReadToolPresentation(tool);
    const queryTool = getQueryToolPresentation(tool);
    const listTool = getListToolPresentation(tool);
    const commandOutputTool = getCommandOutputToolPresentation(tool);
    const approvalTagLabel = getApprovalTagLabel(tool, t);
    const showStatusLabel = !fileMutation || tool.state !== "output-available";
    const showGenericInput = !fileMutation && !commandOutputTool && tool.input !== undefined;
    const showGenericOutput =
      !fileMutation
      && !commandOutputTool
      && (tool.state === "output-available" || tool.state === "output-denied" || tool.state === "output-error")
      && (tool.result !== undefined || tool.error);

    if (tool.name === "clarify" && clarifyPrompt && tool.state === "clarify-requested") {
      return (
        <Message className="max-w-full" from="assistant" key={key}>
          <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
            <div
              className={cn(
                "rounded-2xl border border-app-warning/22 bg-app-warning/8 p-4",
                inset ? "ml-0" : undefined,
              )}
            >
              <div className="space-y-2">
                <div className="flex flex-wrap items-center gap-2 text-xs text-app-warning">
                  {clarifyPrompt.header ? (
                    <span className="rounded-full border border-app-warning/22 bg-app-warning/12 px-2 py-0.5 font-medium">
                      {clarifyPrompt.header}
                    </span>
                  ) : null}
                  <span>Need your input</span>
                </div>
                <p className="text-sm font-medium leading-6 text-app-foreground">
                  {clarifyPrompt.question}
                </p>
              </div>

              <div className="mt-4 grid gap-2">
                {clarifyPrompt.options.map((option, index) => (
                  <button
                    className={cn(
                      "rounded-xl border px-3 py-3 text-left transition",
                      option.recommended
                        ? "border-app-info/28 bg-app-info/8 hover:bg-app-info/12"
                        : "border-app-border/28 bg-app-surface/18 hover:bg-app-surface/28",
                    )}
                    key={`${tool.id}-${option.id}`}
                    onClick={() => {
                      void respondToClarify(
                        tool,
                        {
                          kind: "option",
                          optionId: option.id,
                          text: option.label,
                        },
                        option.label,
                      );
                    }}
                    type="button"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0 space-y-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="inline-flex size-5 items-center justify-center rounded-full border border-app-border/40 text-[11px] font-semibold text-app-subtle">
                            {index + 1}
                          </span>
                          <span className="text-sm font-medium text-app-foreground">
                            {option.label}
                          </span>
                          {option.recommended ? (
                            <span className="rounded-full border border-app-info/22 bg-app-info/10 px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em] text-app-info">
                              Recommended
                            </span>
                          ) : null}
                        </div>
                        <p className="text-xs leading-5 text-app-subtle">{option.description}</p>
                      </div>
                    </div>
                  </button>
                ))}
              </div>
              <p className="mt-3 text-xs text-app-subtle">
                Or type your own reply in the composer below.
              </p>
            </div>
          </MessageContent>
        </Message>
      );
    }

    if (readTool) {
      return (
        <Message className="max-w-full" from="assistant" key={key}>
          <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
            <div
              className={cn(
                "flex w-full items-start justify-between gap-3 text-left",
                inset ? "pl-0" : undefined,
              )}
            >
              <div className="min-w-0 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                <span className="text-app-muted">Read</span>
                <span className="truncate font-medium text-app-info" title={readTool.path}>
                  {readTool.fileName}
                </span>
                {readTool.rangeLabel && (
                  <span className="shrink-0 font-mono text-[12px] text-app-subtle">
                    {readTool.rangeLabel}
                  </span>
                )}
              </div>
              {readTool.error ? (
                <span className="shrink-0 truncate text-xs text-app-danger" title={readTool.error}>
                  {readTool.error}
                </span>
              ) : (
                <span className={cn("shrink-0 pt-0.5 text-xs", getToolStatusClass(tool.state))}>
                  {formatToolStatusLabel(tool.state, t)}
                </span>
              )}
            </div>
          </MessageContent>
        </Message>
      );
    }

    if (queryTool) {
      return (
        <Message className="max-w-full" from="assistant" key={key}>
          <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
            <div
              className={cn(
                "flex w-full items-start justify-between gap-3 text-left",
                inset ? "pl-0" : undefined,
              )}
            >
              <div className="min-w-0 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                <span className="text-app-muted">{queryTool.actionLabel}</span>
                <span className="truncate font-medium text-app-info" title={queryTool.primaryLabel}>
                  {queryTool.primaryLabel}
                </span>
                {queryTool.scopeLabel ? (
                  <span className="shrink-0 text-app-subtle">{`in ${queryTool.scopeLabel}`}</span>
                ) : null}
                {queryTool.countLabel ? (
                  <span className="shrink-0 font-mono text-[12px] text-app-subtle">
                    {queryTool.countLabel}
                  </span>
                ) : null}
              </div>
              {queryTool.error ? (
                <span className="shrink-0 truncate text-xs text-app-danger" title={queryTool.error}>
                  {queryTool.error}
                </span>
              ) : (
                <span className={cn("shrink-0 pt-0.5 text-xs", getToolStatusClass(tool.state))}>
                  {formatToolStatusLabel(tool.state, t)}
                </span>
              )}
            </div>
          </MessageContent>
        </Message>
      );
    }

    if (listTool) {
      return (
        <Message className="max-w-full" from="assistant" key={key}>
          <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
            <div
              className={cn(
                "flex w-full items-start justify-between gap-3 text-left",
                inset ? "pl-0" : undefined,
              )}
            >
              <div className="min-w-0 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                <span className="text-app-muted">List</span>
                <span className="truncate font-medium text-app-info" title={listTool.path}>
                  {listTool.directoryLabel}
                </span>
                {listTool.countLabel ? (
                  <span className="shrink-0 font-mono text-[12px] text-app-subtle">
                    {listTool.countLabel}
                  </span>
                ) : null}
              </div>
              {listTool.error ? (
                <span className="shrink-0 truncate text-xs text-app-danger" title={listTool.error}>
                  {listTool.error}
                </span>
              ) : (
                <span className={cn("shrink-0 pt-0.5 text-xs", getToolStatusClass(tool.state))}>
                  {formatToolStatusLabel(tool.state, t)}
                </span>
              )}
            </div>
          </MessageContent>
        </Message>
      );
    }

    return (
      <Message className="max-w-full" from="assistant" key={key}>
        <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
          <CompactCollapsible
            onOpenChange={(open) => {
              if (!isCompletedToolState(tool.state)) {
                return;
              }

              handleCompletedToolOpenChange(tool.id, open);
            }}
            open={!isCompletedToolState(tool.state) ? true : (completedToolOpen[tool.id] ?? false)}
          >
            <CompactCollapsibleHeader
              className={cn(
                "items-start gap-3 text-left text-app-subtle hover:text-app-foreground",
                inset ? "pl-0" : undefined,
              )}
              trailing={showStatusLabel ? (
                <span className={cn("shrink-0 text-xs", getToolStatusClass(tool.state))}>
                  {formatToolStatusLabel(tool.state, t)}
                </span>
              ) : null}
            >
              {fileMutation ? (
                <div className="min-w-0 space-y-1">
                  <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                    <span className="text-app-muted">{fileMutation.actionLabel}</span>
                    <span className="truncate font-medium text-app-info" title={fileMutation.path}>
                      {fileMutation.fileName}
                    </span>
                    {typeof fileMutation.linesAdded === "number" && fileMutation.linesAdded > 0 ? (
                      <span className="shrink-0 font-medium text-app-success">{`+${fileMutation.linesAdded}`}</span>
                    ) : null}
                    {typeof fileMutation.linesRemoved === "number" && fileMutation.linesRemoved > 0 ? (
                      <span className="shrink-0 font-medium text-app-danger">{`-${fileMutation.linesRemoved}`}</span>
                    ) : null}
                  </div>
                  <p className="truncate text-xs text-app-subtle">{fileMutation.path}</p>
                </div>
              ) : commandOutputTool ? (
                <div className="min-w-0 space-y-1">
                  <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                    <span className="text-app-muted">{commandOutputTool.actionLabel}</span>
                    <span
                      className="truncate font-medium text-app-info"
                      title={commandOutputTool.command}
                    >
                      {commandOutputTool.summaryLabel}
                    </span>
                    {approvalTagLabel ? (
                      <span
                        className={cn(
                          "inline-flex items-center rounded-full border px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em]",
                          getApprovalTagClass(tool),
                        )}
                        title={getApprovalReason(tool.approval) ?? undefined}
                      >
                        {approvalTagLabel}
                      </span>
                    ) : null}
                  </div>
                  {commandOutputTool.detailLabel ? (
                    <p className="truncate text-xs text-app-subtle">{commandOutputTool.detailLabel}</p>
                  ) : null}
                </div>
              ) : (
                <div className="flex min-w-0 items-start gap-3">
                  <WrenchIcon className={cn("mt-0.5 size-4 shrink-0", getToolStatusClass(tool.state))} />
                  <span className="truncate text-app-foreground text-sm" title={tool.name}>
                    {tool.name}
                  </span>
                </div>
              )}
            </CompactCollapsibleHeader>
            <CompactCollapsibleContent className="pl-0">
              <div className="space-y-3">
                {fileMutation ? (
                  <div className="space-y-3">
                    <div className="rounded-2xl border border-app-border/18 bg-app-surface/16 shadow-none">
                      <div className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-app-border/14 px-4 py-3">
                        <span className="text-[15px] font-semibold text-app-foreground">{fileMutation.fileName}</span>
                        {typeof fileMutation.linesAdded === "number" && fileMutation.linesAdded > 0 ? (
                          <span className="text-sm font-medium text-app-success">{`+${fileMutation.linesAdded}`}</span>
                        ) : null}
                        {typeof fileMutation.linesRemoved === "number" && fileMutation.linesRemoved > 0 ? (
                          <span className="text-sm font-medium text-app-danger">{`-${fileMutation.linesRemoved}`}</span>
                        ) : null}
                        {approvalTagLabel ? (
                          <span
                            className={cn(
                              "ml-auto inline-flex items-center rounded-full border px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em]",
                              getApprovalTagClass(tool),
                            )}
                            title={getApprovalReason(tool.approval) ?? undefined}
                          >
                            {approvalTagLabel}
                          </span>
                        ) : null}
                      </div>
                      <div className="overflow-hidden rounded-b-2xl bg-app-canvas/70">
                        <FileMutationDiffPreview
                          contentPreview={fileMutation.contentPreview}
                          diff={fileMutation.diff}
                        />
                      </div>
                    </div>
                  </div>
                ) : null}

                {commandOutputTool ? (
                  <ToolCommandOutputBlocks presentation={commandOutputTool} />
                ) : null}

                {showGenericInput ? (
                  <ToolInput
                    className="space-y-1.5"
                    codeBlockContentClassName={TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS}
                    input={tool.input}
                    label={t("tool.label.input")}
                  />
                ) : null}

                {!fileMutation
                && !commandOutputTool
                && tool.state !== "approval-requested"
                && tool.state !== "clarify-requested" ? (
                  <Confirmation
                    className={cn(
                      "gap-3 rounded-xl border px-3 py-3 shadow-none",
                      isApprovalDenied(tool.approval)
                        ? "border-app-danger/18 bg-app-danger/6"
                        : "border-app-border/18 bg-app-surface/14",
                    )}
                    approval={tool.approval}
                    state={tool.state as "approval-responded" | "input-streaming" | "input-available" | "output-available" | "output-denied" | "output-error"}
                  >
                    <ConfirmationTitle className="text-sm text-app-muted">
                      <ConfirmationRequest>
                        {t("tool.approval.request")}
                      </ConfirmationRequest>
                      <ConfirmationAccepted>
                        <span>{getApprovalReason(tool.approval) || t("tool.approval.granted")}</span>
                      </ConfirmationAccepted>
                      <ConfirmationRejected>
                        <span>{tool.error || getApprovalReason(tool.approval) || t("tool.approval.denied")}</span>
                      </ConfirmationRejected>
                    </ConfirmationTitle>

                    <ConfirmationActions className="justify-start self-auto pt-1">
                      <ConfirmationAction
                        className="h-7 px-2.5 text-xs"
                        onClick={() => {
                          if (!streamRef.current?.runId) {
                            return;
                          }

                          void streamRef.current.respondToApproval(tool.id, streamRef.current.runId, false);
                        }}
                        size="sm"
                        variant="ghost"
                      >
                        {t("tool.action.reject")}
                      </ConfirmationAction>
                      <ConfirmationAction
                        className="h-7 px-2.5 text-xs"
                        onClick={() => {
                          if (!streamRef.current?.runId) {
                            return;
                          }

                          void streamRef.current.respondToApproval(tool.id, streamRef.current.runId, true);
                        }}
                        size="sm"
                        variant="outline"
                      >
                        {t("tool.action.approve")}
                      </ConfirmationAction>
                    </ConfirmationActions>
                  </Confirmation>
                ) : null}

                {showGenericOutput ? (
                  <ToolOutput
                    className="space-y-1.5"
                    codeBlockContentClassName={TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS}
                    errorLabel={t("tool.label.error")}
                    errorText={tool.state === "output-available" ? undefined : tool.error}
                    label={t("tool.label.output")}
                    output={stringifyToolValue(tool.state === "output-available" ? tool.result : tool.error ?? tool.result)}
                  />
                ) : null}

                {tool.state === "approval-requested" ? (
                  <div className="flex justify-end gap-2 pt-1">
                    <ConfirmationAction
                      className="h-7 px-2.5 text-xs"
                      onClick={() => {
                        if (!streamRef.current?.runId) {
                          return;
                        }

                        void streamRef.current.respondToApproval(tool.id, streamRef.current.runId, false);
                      }}
                      size="sm"
                      variant="ghost"
                    >
                      {t("tool.action.reject")}
                    </ConfirmationAction>
                    <ConfirmationAction
                      className="h-7 px-2.5 text-xs"
                      onClick={() => {
                        if (!streamRef.current?.runId) {
                          return;
                        }

                        void streamRef.current.respondToApproval(tool.id, streamRef.current.runId, true);
                      }}
                      size="sm"
                      variant="outline"
                    >
                      {t("tool.action.approve")}
                    </ConfirmationAction>
                  </div>
                ) : null}
              </div>
            </CompactCollapsibleContent>
          </CompactCollapsible>
        </MessageContent>
      </Message>
    );
  }, [completedToolOpen, handleCompletedToolOpenChange, respondToClarify, t]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-app-canvas">
      <div className="pointer-events-none absolute left-1/2 top-0 h-56 w-[72rem] -translate-x-1/2 rounded-full bg-[radial-gradient(circle,rgba(120,180,255,0.11),transparent_68%)] blur-3xl" />
      <div className="relative min-h-0 flex-1">
        <Conversation className="size-full" contextRef={conversationContextRef}>
          <ConversationContent className="mx-auto w-full max-w-4xl gap-0 px-6 pb-10 pt-8">
            {hasMoreMessages ? (
              <div className="pb-4">
                <div className="flex flex-col items-center gap-2">
                  <Button
                    disabled={isLoading || isLoadingMoreMessages}
                    onClick={() => void loadOlderMessages()}
                    size="sm"
                    variant="outline"
                  >
                    {isLoadingMoreMessages ? "Loading older messages..." : "Load older messages"}
                  </Button>
                  {historyLoadError ? (
                    <p className="text-xs text-app-danger">{historyLoadError}</p>
                  ) : null}
                </div>
              </div>
            ) : null}

            {isLoading && messages.length === 0 ? (
              <ConversationEmptyState
                description="Loading thread history and runtime state."
                icon={<SparklesIcon className="size-5" />}
                title="Loading thread"
              />
            ) : null}

            {loadError ? (
              <div className="rounded-2xl border border-app-danger/25 bg-app-danger/8 px-4 py-3 text-sm text-app-danger">
                <div className="flex items-center gap-2 font-medium">
                  <AlertCircleIcon className="size-4" />
                  Failed to load thread state
                </div>
                <p className="mt-2 leading-6 text-app-danger/90">{loadError}</p>
                <Button className="mt-3" onClick={() => void loadSnapshot()} size="sm" variant="outline">
                  <RefreshCcwIcon className="size-3.5" />
                  Retry
                </Button>
              </div>
            ) : null}

            {!isLoading && !loadError && messages.length === 0 && !hasRuntimeArtifacts ? (
              <ConversationEmptyState
                description="Ask Tiy to inspect the workspace, run tools, or plan the next task."
                icon={<BotIcon className="size-5" />}
                title={threadTitle || "No messages yet"}
              />
            ) : null}

            {presentationEntries.map((entry, index) => {
              const currentRole = getPresentationEntryRole(entry);
              const previousRole = index > 0
                ? getPresentationEntryRole(presentationEntries[index - 1])
                : null;
              const spacingClass = getRoleSpacingClass(previousRole, currentRole);

              if (entry.kind === "thinking-placeholder") {
                return (
                  <div className={spacingClass} key={entry.key}>
                    <Message className="max-w-full" from="assistant">
                      <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                        <Reasoning
                          className="mb-0 w-full bg-transparent px-0 py-0"
                          defaultOpen={false}
                          isStreaming
                        >
                          <ReasoningTrigger />
                        </Reasoning>
                      </MessageContent>
                    </Message>
                  </div>
                );
              }

              if (entry.kind === "message") {
                const { message } = entry;
                const summaryMarker = message.messageType === "summary_marker"
                  ? parseSummaryMarkerMetadata(message.metadata)
                  : null;

                if (message.messageType === "summary_marker" && summaryMarker?.kind === "context_reset") {
                  return (
                    <div className={spacingClass} key={entry.key}>
                      <div className="flex items-center gap-3 py-2">
                        <div className="h-px flex-1 bg-app-border/28" />
                        <span className="rounded-full border border-app-border/24 bg-app-surface/40 px-3 py-1 text-[11px] font-medium uppercase tracking-[0.08em] text-app-subtle">
                          {summaryMarker.label ?? "Context is now reset"}
                        </span>
                        <div className="h-px flex-1 bg-app-border/28" />
                      </div>
                    </div>
                  );
                }

                if (message.messageType === "summary_marker" && summaryMarker?.kind === "context_summary") {
                  return (
                    <div className={spacingClass} key={entry.key}>
                      <Message className="max-w-full" from="assistant">
                        <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                          <div className="rounded-2xl border border-app-border/24 bg-app-surface/18 px-4 py-3">
                            <div className="text-[11px] font-medium uppercase tracking-[0.08em] text-app-subtle">
                              {summaryMarker.label ?? "Compacted context summary"}
                            </div>
                            <div className="mt-2 whitespace-pre-wrap text-sm leading-6 text-app-muted">
                              {message.content}
                            </div>
                          </div>
                        </MessageContent>
                      </Message>
                    </div>
                  );
                }

                if (message.messageType === "reasoning") {
                  return (
                    <div className={spacingClass} key={entry.key}>
                      <Message className="max-w-full" from="assistant">
                        <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                          <Reasoning
                            className="mb-0 w-full bg-transparent px-0 py-0"
                            defaultOpen={message.status === "streaming" || runState === "running"}
                            isStreaming={message.status === "streaming"}
                          >
                            <ReasoningTrigger />
                            <ReasoningContent>{message.content}</ReasoningContent>
                          </Reasoning>
                        </MessageContent>
                      </Message>
                    </div>
                  );
                }

                if (message.messageType === "plan") {
                  const formattedPlan = formatPlanMetadata(message.metadata, message.content);
                  const approvalStateLabel = formattedPlan.approvalState
                    ? formatApprovalPromptState(formattedPlan.approvalState, null, t)
                    : null;

                  return (
                    <div className={spacingClass} key={entry.key}>
                      <Message className="max-w-full" from="assistant">
                        <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                          <Plan className="overflow-hidden rounded-2xl border border-app-border/28 bg-app-surface/28 shadow-none">
                            <PlanHeader>
                              <div className="space-y-3">
                                <div className="flex flex-wrap items-center gap-2 text-xs text-app-subtle">
                                  {formattedPlan.planRevision !== null ? (
                                    <span>{`Plan v${formattedPlan.planRevision}`}</span>
                                  ) : null}
                                  {approvalStateLabel ? (
                                    <span>{approvalStateLabel}</span>
                                  ) : null}
                                </div>
                                <PlanTitle>{formattedPlan.title}</PlanTitle>
                                <PlanDescription>{formattedPlan.summary}</PlanDescription>
                              </div>
                              <PlanTrigger />
                            </PlanHeader>
                            <PlanContent className="space-y-4">
                              {formattedPlan.context
                                ? renderPlanProseSection("Context", formattedPlan.context)
                                : null}

                              {formattedPlan.design
                                ? renderPlanProseSection("Design", formattedPlan.design)
                                : null}

                              {formattedPlan.keyImplementation
                                ? renderPlanProseSection(
                                  "Key Implementation",
                                  formattedPlan.keyImplementation,
                                )
                                : null}

                              {formattedPlan.steps.length > 0 ? (
                                renderPlanListSection(message.id, "Steps", formattedPlan.steps, true)
                              ) : (
                                <MessageResponse>{message.content}</MessageResponse>
                              )}

                              {formattedPlan.verification
                                ? renderPlanProseSection(
                                  "Verification",
                                  formattedPlan.verification,
                                )
                                : null}

                              {formattedPlan.risks.length > 0
                                ? renderPlanListSection(message.id, "Risks", formattedPlan.risks)
                                : null}

                              {formattedPlan.assumptions.length > 0
                                ? renderPlanListSection(
                                  message.id,
                                  "Assumptions",
                                  formattedPlan.assumptions,
                                )
                                : null}
                            </PlanContent>
                          </Plan>
                        </MessageContent>
                      </Message>
                    </div>
                  );
                }

                if (message.messageType === "approval_prompt") {
                  const approvalPrompt = parseApprovalPromptMetadata(message.metadata, t);
                  const approvalState = approvalPrompt?.state ?? "pending";
                  const approvalOptions = approvalPrompt?.options ?? [
                    { action: "apply_plan" as const, label: t("plan.implementAsPlan") },
                    { action: "apply_plan_with_context_reset" as const, label: t("plan.clearAndImplement") },
                  ];
                  const disabled =
                    !threadId
                    || approvalState !== "pending"
                    || hasLiveRun
                    || approvingPlanMessageId === message.id;

                  return (
                    <div className={spacingClass} key={entry.key}>
                      <Message className="max-w-full" from="assistant">
                        <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                          <div className="rounded-2xl border border-app-border/24 bg-app-surface/20 p-4">
                            <div className="space-y-2">
                              <div className="flex flex-wrap items-center gap-2 text-xs text-app-subtle">
                                {approvalPrompt?.planRevision !== null && approvalPrompt?.planRevision !== undefined ? (
                                  <span>{`Plan v${approvalPrompt.planRevision}`}</span>
                                ) : null}
                                <span>{formatApprovalPromptState(approvalState, approvalPrompt?.approvedAction ?? null, t)}</span>
                              </div>
                              <MessageResponse>{message.content}</MessageResponse>
                            </div>

                            <div className="mt-4 flex flex-wrap gap-2">
                              {approvalOptions.map((option) => (
                                <Button
                                  disabled={disabled}
                                  key={`${message.id}-${option.action}`}
                                  onClick={() => {
                                    void handlePlanApproval(message.id, option.action);
                                  }}
                                  size="sm"
                                  variant={option.action === "apply_plan" ? "default" : "outline"}
                                >
                                  {option.label}
                                </Button>
                              ))}
                            </div>
                          </div>
                        </MessageContent>
                      </Message>
                    </div>
                  );
                }

                return (
                  <div className={spacingClass} key={entry.key}>
                    <Message
                      className={message.role === "assistant" ? "max-w-full" : undefined}
                      from={message.role}
                    >
                      <MessageContent
                        className={
                          message.role === "assistant"
                            ? "w-full max-w-full bg-transparent px-0 py-0 shadow-none"
                            : "rounded-2xl bg-app-surface/62 px-4 py-3 shadow-none backdrop-blur-sm"
                        }
                      >
                        {message.role === "assistant" && message.status === "discarded" ? (
                          <div className="rounded-2xl border border-app-warning/22 bg-app-warning/10 px-4 py-3 text-app-foreground">
                            <div className="mb-2 inline-flex items-center gap-2 rounded-full bg-app-warning/12 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.08em] text-app-warning">
                              <Info className="size-3.5" />
                              {t("tool.discarded.title")}
                            </div>
                            <MessageResponse>{message.content}</MessageResponse>
                            <p className="mt-3 text-xs leading-5 text-app-warning/90">
                              {t("tool.discarded.description")}
                            </p>
                          </div>
                        ) : (
                          (() => {
                            const commandComposer = message.role === "user"
                              ? parseCommandComposerMetadata(message.metadata)
                              : null;
                            const expandedPrompt = commandComposer?.kind === "command"
                              ? (commandComposer.effectivePrompt?.trim() ?? "")
                              : "";

                            return (
                              <div className="space-y-2">
                                <ComposerMessageAttachments
                                  attachments={message.attachments.map((attachment) => ({
                                    id: attachment.id,
                                    mediaType: attachment.mediaType ?? undefined,
                                    name: attachment.name,
                                    url: attachment.url ?? undefined,
                                  }))}
                                />
                                <MessageResponse>{message.content || (message.status === "streaming" ? "…" : "")}</MessageResponse>
                                {expandedPrompt && expandedPrompt !== (message.content ?? "").trim() ? (
                                  <CompactCollapsible defaultOpen={false}>
                                    <CompactCollapsibleHeader className="items-start gap-3 text-left text-app-subtle hover:text-app-foreground">
                                      <div className="min-w-0">
                                        <div className="text-[11px] font-medium uppercase tracking-[0.08em] text-app-subtle">
                                          Expanded prompt
                                        </div>
                                        <div className="truncate text-xs text-app-muted">
                                          {expandedPrompt}
                                        </div>
                                      </div>
                                    </CompactCollapsibleHeader>
                                    <CompactCollapsibleContent className="pl-0">
                                      <div className="whitespace-pre-wrap rounded-xl border border-app-border/25 bg-app-surface/35 px-3 py-2 text-xs leading-5 text-app-muted">
                                        {expandedPrompt}
                                      </div>
                                    </CompactCollapsibleContent>
                                  </CompactCollapsible>
                                ) : null}
                              </div>
                            );
                          })()
                        )}
                      </MessageContent>
                    </Message>
                  </div>
                );
              }

              if (entry.kind === "helper") {
                const { helper } = entry;
                const helperName = formatHelperName(helper);
                const helperDetailSummary = formatHelperDetailSummary(helper);
                const helperSummary = formatHelperSummary(helper);
                const helperToolCounts = formatHelperToolCounts(helper.toolCounts);
                const executionSummary = formatExecutionSummary({
                  elapsedText: formatElapsedSeconds(getHelperElapsedSeconds(helper)),
                  toolUses: helper.totalToolCalls,
                });
                return (
                  <div className={spacingClass} key={entry.key}>
                    <Message className="max-w-full" from="assistant">
                      <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                        <CompactCollapsible
                          onOpenChange={(open) => handleHelperOpenChange(helper.id, open)}
                          open={helperOpen[helper.id] ?? helper.status !== "completed"}
                        >
                          <CompactCollapsibleHeader
                            className="items-start gap-3 text-left text-app-subtle hover:text-app-foreground"
                            trailing={
                              <span
                                className={cn(
                                  "shrink-0 text-xs",
                                  helper.status === "failed"
                                    ? "text-app-danger"
                                    : helper.status === "completed"
                                      ? "text-app-subtle"
                                      : "text-app-info",
                                )}
                              >
                                {formatHelperStatusLabel(helper.status)}
                              </span>
                            }
                          >
                            <div className="flex min-w-0 items-start gap-3">
                              <BotIcon
                                className={cn(
                                  "mt-0.5 size-4 shrink-0",
                                  helper.status === "failed"
                                    ? "text-app-danger"
                                    : helper.status === "completed"
                                      ? "text-app-subtle"
                                      : "text-app-info",
                                )}
                              />
                              <span
                                className="block truncate text-app-foreground text-sm"
                                title={helperSummary}
                              >
                                {helper.status === "running" ? (
                                  <Shimmer as="span" className="align-baseline" duration={1}>
                                    {helperName}
                                  </Shimmer>
                                ) : (
                                  helperName
                                )}
                                {helperDetailSummary ? (
                                  <span className="text-app-subtle">
                                    {" · "}
                                    {helperDetailSummary}
                                  </span>
                                ) : null}
                              </span>
                            </div>
                          </CompactCollapsibleHeader>
                          <CompactCollapsibleContent className="pl-0">
                            <div className="max-h-40 space-y-2 overflow-y-auto pr-3">
                              {helperToolCounts.length > 0 ? (
                                <p className="whitespace-pre-wrap break-words text-xs text-app-subtle">
                                  {helperToolCounts.join(" · ")}
                                </p>
                              ) : null}
                              {helper.totalToolCalls > 0 && helper.status !== "completed" ? (
                                <p className="text-xs text-app-subtle">
                                  {`${helper.completedSteps} of ${formatToolCallCount(helper.totalToolCalls)} finished`}
                                </p>
                              ) : null}
                              {helper.currentAction ? (
                                <p className="whitespace-pre-wrap break-words text-xs text-app-subtle">
                                  {`Current: ${helper.currentAction}`}
                                </p>
                              ) : null}
                              {helper.latestMessage ? (
                                <p className="whitespace-pre-wrap break-words text-sm text-app-muted">
                                  {helper.latestMessage}
                                </p>
                              ) : null}
                              {helper.recentActions.length > 0 ? (
                                <div className="space-y-1">
                                  {helper.recentActions.slice(-3).map((action, index) => (
                                    <p
                                      className="whitespace-pre-wrap break-words text-sm text-app-muted"
                                      key={`${helper.id}-action-${index}`}
                                    >
                                      {action}
                                    </p>
                                  ))}
                                </div>
                              ) : null}
                              {helper.summary ? (
                                <p className="whitespace-pre-wrap break-words text-sm text-app-muted">
                                  {helper.summary}
                                </p>
                              ) : null}
                              {helper.error ? (
                                <p className="whitespace-pre-wrap break-words text-sm text-app-danger">
                                  {helper.error}
                                </p>
                              ) : null}
                            </div>
                          </CompactCollapsibleContent>
                          {executionSummary ? (
                            <CompactCollapsibleFootnote className="pl-0">
                              {executionSummary}
                            </CompactCollapsibleFootnote>
                          ) : null}
                        </CompactCollapsible>
                      </MessageContent>
                    </Message>
                  </div>
                );
              }

              const { tool } = entry;
              return (
                <div className={spacingClass} key={entry.key}>
                  {renderToolEntry(tool, entry.key)}
                </div>
              );
            })}

            {queueArtifact ? (
              <div className={getRoleSpacingClass(queuePreviousRole, "assistant")}>
                <Message className="max-w-full" from="assistant">
                  <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                    <Queue className="rounded-2xl border border-app-border/24 bg-app-surface/16 p-2 shadow-none">
                      <div>
                        <div className="px-3 py-2 text-sm font-medium text-app-foreground">Runtime Queue</div>
                        <div className="rounded-xl bg-app-surface/45 px-3 py-3 text-sm text-app-muted">
                          <MessageResponse>{JSON.stringify(queueArtifact, null, 2)}</MessageResponse>
                        </div>
                      </div>
                    </Queue>
                  </MessageContent>
                </Message>
              </div>
            ) : null}

            {taskBoards.activeBoard ? (
              <div className={getRoleSpacingClass(queueArtifact ? "assistant" : lastPresentationRole, "assistant")}>
                <Message className="max-w-full" from="assistant">
                  <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                    <TaskBoardCard board={taskBoards.activeBoard} />
                  </MessageContent>
                </Message>
              </div>
            ) : null}

            {taskBoards.boards.length > 1 ? (
              <div className={getRoleSpacingClass(taskBoards.activeBoard ? "assistant" : lastPresentationRole, "assistant")}>
                <Message className="max-w-full" from="assistant">
                  <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                    <TaskHistoryTimeline boards={taskBoards.boards} />
                  </MessageContent>
                </Message>
              </div>
            ) : null}

            {runtimeError ? (
              <div className={getRoleSpacingClass(runtimeErrorPreviousRole, "assistant")}>
                <Message className="max-w-full" from="assistant">
                  <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                      <div className="rounded-2xl border border-app-danger/25 bg-app-danger/8 px-4 py-3 text-sm text-app-danger">
                        <div className="flex items-center gap-2 font-medium">
                          <AlertCircleIcon className="size-4" />
                        {runState === "interrupted"
                          ? "Last run interrupted"
                          : runState === "limit_reached"
                            ? "Run paused at turn limit"
                            : "Last run failed"}
                        </div>
                      <p className="mt-2 whitespace-pre-wrap leading-6 text-app-danger/90">{runtimeError.message}</p>
                    </div>
                  </MessageContent>
                </Message>
              </div>
            ) : null}

            {!messages.length && !hasRuntimeArtifacts && !isLoading && !loadError ? (
              <div className="rounded-2xl border border-dashed border-app-border bg-app-surface/20 px-4 py-3 text-sm text-app-muted">
                Runtime events, helper summaries, tool approvals, and reasoning traces will appear here once the thread starts running.
              </div>
            ) : null}
          </ConversationContent>
          <ConversationScrollButton className="bottom-4" />
        </Conversation>
      </div>

      <div className="shrink-0 px-6 pb-6 pt-4">
        <WorkbenchPromptComposer
          activeAgentProfileId={activeAgentProfileId}
          agentProfiles={agentProfiles}
          canSubmitWhenAttachmentsOnly={false}
          commands={commands}
          enabledSkills={enabledSkills}
          error={composerError}
          onErrorMessageChange={setComposerError}
          onRunModeChange={setSelectedRunMode}
          onSelectAgentProfile={onSelectAgentProfile}
          onStop={() => {
            if (!threadId) {
              return;
            }

            void streamRef.current?.cancelRun(threadId).then(() => {
              // Optimistic UI update: immediately reflect the cancellation in
              // the UI so the user sees instant feedback. The backend has
              // accepted the cancel request but `RunCancelled` may arrive late
              // if the agent loop is blocked on a long-running HTTP call.
              completeThinkingPhase();
              setRunState("cancelled");
              onRunStateChange?.("cancelled");

              // Safety net: if the backend event (`run_cancelled`) hasn't
              // arrived within 5 seconds, force a snapshot reload to reconcile
              // the UI with the actual backend state.
              const timer = setTimeout(() => {
                void loadSnapshot();
              }, 5_000);

              // If the stream delivers a terminal event before the timeout,
              // the next `onRunStateChange` + `loadSnapshot` will render the
              // correct state and this timer becomes a harmless no-op.
              return () => clearTimeout(timer);
            }).catch(() => {
              // The cancel request failed — most likely the run already
              // finished on the backend but the terminal event was lost or
              // hasn't arrived yet.  Reload the snapshot to reconcile the
              // UI with the actual backend state.
              void loadSnapshot();
            });
          }}
          onSubmit={handleSubmit}
          placeholder="Ask Tiy anything, @ to add files, / for commands, $ for skills"
          providers={providers}
          runMode={selectedRunMode}
          runModeDisabled={runState === "running" || runState === "waiting_approval"}
          showRunModeToggle
          status={composerStatus}
          value={composerValue}
          workspaceId={workspaceId}
          onValueChange={setComposerValue}
        />
      </div>
    </div>
  );
}
