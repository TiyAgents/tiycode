"use client";

import type { ChatStatus } from "ai";
import { AlertCircleIcon, BotIcon, RefreshCcwIcon, SparklesIcon, WrenchIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CompactCollapsible,
  CompactCollapsibleContent,
  CompactCollapsibleFootnote,
  CompactCollapsibleHeader,
} from "@/components/ai-elements/compact-collapsible";
import { CodeBlock } from "@/components/ai-elements/code-block";
import { Conversation, ConversationContent, ConversationEmptyState, ConversationScrollButton } from "@/components/ai-elements/conversation";
import { Message, MessageContent, MessageResponse } from "@/components/ai-elements/message";
import { Plan, PlanContent, PlanDescription, PlanHeader, PlanTitle, PlanTrigger } from "@/components/ai-elements/plan";
import { Queue } from "@/components/ai-elements/queue";
import { Reasoning, ReasoningContent, ReasoningTrigger } from "@/components/ai-elements/reasoning";
import { Confirmation, ConfirmationAccepted, ConfirmationAction, ConfirmationActions, ConfirmationRejected, ConfirmationRequest, ConfirmationTitle } from "@/components/ai-elements/confirmation";
import { buildRunModelPlanFromSelection } from "@/modules/settings-center/model/run-model-plan";
import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import { threadLoad } from "@/services/bridge";
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
  MessageDto,
  RunSummaryDto,
  RunHelperDto,
  SubagentProgressSnapshot,
  ThreadSnapshotDto,
  ToolCallDto,
} from "@/shared/types/api";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import type { PromptInputMessage } from "@/components/ai-elements/prompt-input";
import { WorkbenchPromptComposer } from "@/modules/workbench-shell/ui/workbench-prompt-composer";

type SurfaceMessage = {
  createdAt: string;
  id: string;
  messageType: MessageDto["messageType"];
  role: "user" | "assistant" | "system";
  runId: string | null;
  content: string;
  status: "streaming" | "completed" | "failed";
};

type SurfaceToolState =
  | "approval-requested"
  | "approval-responded"
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
  cacheReadTokens: number;
  cacheWriteTokens: number;
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
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
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
  prompt: string;
};

type ThinkingPlaceholder = {
  createdAt: string;
  id: string;
  runId?: string | null;
};

type RuntimeThreadSurfaceProps = {
  activeAgentProfileId: string;
  agentProfiles: ReadonlyArray<AgentProfile>;
  initialPromptRequest?: InitialPromptRequest | null;
  onConsumeInitialPrompt?: (id: string) => void;
  onContextUsageChange?: (usage: ThreadContextUsage | null) => void;
  onRunStateChange?: (state: RunState) => void;
  onSelectAgentProfile: (id: string) => void;
  onThreadTitleChange?: (threadId: string, title: string) => void;
  providers: ReadonlyArray<ProviderEntry>;
  threadId: string | null;
  threadTitle: string;
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

type TimelineRole = SurfaceMessage["role"];

function mapSnapshotMessage(message: MessageDto): SurfaceMessage {
  return {
    createdAt: message.createdAt ?? new Date().toISOString(),
    id: message.id,
    messageType: message.messageType,
    role:
      message.role === "user" || message.role === "assistant" || message.role === "system"
        ? message.role
        : "assistant",
    runId: message.runId,
    content: message.contentMarkdown,
    status: message.status,
  };
}

function mapSnapshotToolState(tool: ToolCallDto): SurfaceToolState {
  switch (tool.status) {
    case "waiting_approval":
      return "approval-requested";
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
    cacheReadTokens: helper.usage.cacheReadTokens,
    cacheWriteTokens: helper.usage.cacheWriteTokens,
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
    totalTokens: helper.usage.totalTokens,
    inputTokens: helper.usage.inputTokens,
    outputTokens: helper.usage.outputTokens,
  };
}

function mapSnapshotToRunState(snapshot: ThreadSnapshotDto): RunState {
  if (snapshot.activeRun) {
    switch (snapshot.activeRun.status) {
      case "waiting_approval":
        return "waiting_approval";
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
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
    default:
      return "completed";
  }
}

function getLatestVisibleRun(snapshot: ThreadSnapshotDto) {
  return snapshot.activeRun ?? snapshot.latestRun;
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

  if (run.status !== "failed" && run.status !== "denied" && run.status !== "interrupted") {
    return null;
  }

  return {
    message:
      run.errorMessage
      ?? "The app closed or the run was terminated before completion. This thread was restored as interrupted.",
    runId: run.id,
  };
}

function buildPromptText(message: PromptInputMessage) {
  const nextText = message.text?.trim() ?? "";
  const attachmentNames = message.files
    .map((file: PromptInputMessage["files"][number], index: number) => file.filename?.trim() || `Attachment ${index + 1}`)
    .filter((value: string) => value.length > 0);

  if (!nextText && attachmentNames.length === 0) {
    return "";
  }

  if (attachmentNames.length === 0) {
    return nextText;
  }

  const attachmentSection = attachmentNames.map((name: string) => `- ${name}`).join("\n");
  return [nextText, "Attached files:", attachmentSection].filter(Boolean).join("\n\n");
}

function formatPlan(plan: unknown) {
  if (!plan || typeof plan !== "object" || Array.isArray(plan)) {
    return {
      title: "Execution Plan",
      description: "Latest plan artifact emitted by the runtime.",
      steps: [JSON.stringify(plan, null, 2)],
    };
  }

  const value = plan as {
    description?: unknown;
    overview?: unknown;
    steps?: unknown;
    title?: unknown;
  };

  const rawSteps = Array.isArray(value.steps) ? value.steps : [];
  const steps = rawSteps.map((step) => {
    if (typeof step === "string") {
      return step;
    }

    return JSON.stringify(step, null, 2);
  });

  return {
    title: typeof value.title === "string" && value.title.trim() ? value.title : "Execution Plan",
    description:
      typeof value.description === "string" && value.description.trim()
        ? value.description
        : typeof value.overview === "string" && value.overview.trim()
          ? value.overview
          : "Latest plan artifact emitted by the runtime.",
    steps,
  };
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
  | "cacheReadTokens"
  | "cacheWriteTokens"
  | "completedSteps"
  | "currentAction"
  | "inputTokens"
  | "outputTokens"
  | "recentActions"
  | "toolCounts"
  | "totalToolCalls"
  | "totalTokens"
> {
  return {
    cacheReadTokens: snapshot.usage.cacheReadTokens ?? 0,
    cacheWriteTokens: snapshot.usage.cacheWriteTokens ?? 0,
    completedSteps: snapshot.completedSteps ?? 0,
    currentAction: snapshot.currentAction ?? null,
    inputTokens: snapshot.usage.inputTokens ?? 0,
    outputTokens: snapshot.usage.outputTokens ?? 0,
    recentActions: snapshot.recentActions ?? [],
    toolCounts: snapshot.toolCounts ?? {},
    totalToolCalls: snapshot.totalToolCalls ?? 0,
    totalTokens: snapshot.usage.totalTokens ?? 0,
  };
}

function formatHelperKind(kind: string) {
  switch (kind) {
    case "helper_scout":
      return "Research Agent";
    case "helper_planner":
    case "helper_plan_reviewer":
      return "Plan Review Agent";
    case "helper_reviewer":
      return "Review Agent";
    default:
      return kind;
  }
}

function formatToolCallCount(count: number) {
  return `${count} tool call${count === 1 ? "" : "s"}`;
}

function formatToolStatusLabel(state: SurfaceToolState) {
  switch (state) {
    case "approval-requested":
      return "approval";
    case "approval-responded":
      return "approved";
    case "input-available":
      return "running";
    case "input-streaming":
      return "pending";
    case "output-available":
      return "done";
    case "output-denied":
      return "denied";
    case "output-error":
      return "error";
  }
}

function getToolStatusClass(state: SurfaceToolState) {
  switch (state) {
    case "approval-requested":
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

function countDiffLineChanges(diff: string) {
  let linesAdded = 0;
  let linesRemoved = 0;

  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ ") || line.startsWith("--- ") || line.startsWith("@@ ")) {
      continue;
    }

    if (line.startsWith("+")) {
      linesAdded += 1;
      continue;
    }

    if (line.startsWith("-")) {
      linesRemoved += 1;
    }
  }

  return { linesAdded, linesRemoved };
}

function isFileMutationToolName(toolName: string) {
  return toolName === "edit" || toolName === "patch" || toolName === "write";
}

type FileMutationPresentation = {
  actionLabel: string;
  contentPreview: string | null;
  diff: string | null;
  fileName: string;
  linesAdded: number | null;
  linesRemoved: number | null;
  path: string;
};

type DiffPreviewRow = {
  kind: "add" | "context" | "hunk" | "remove";
  lineNumber: number | null;
  text: string;
};

type ReadToolPresentation = {
  fileName: string;
  path: string;
  rangeLabel: string;
};

type QueryToolPresentation = {
  actionLabel: "Find" | "Search";
  countLabel: string | null;
  primaryLabel: string;
  scopeLabel: string | null;
};

type ListToolPresentation = {
  countLabel: string | null;
  directoryLabel: string;
  path: string;
};

function getFileMutationPresentation(tool: SurfaceToolEntry): FileMutationPresentation | null {
  if (!isFileMutationToolName(tool.name)) {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const path = getToolDataString(result, "path") ?? getToolDataString(input, "path");

  if (!path) {
    return null;
  }

  const diff = getToolDataString(result, "diff");
  const derivedDiffCounts = diff ? countDiffLineChanges(diff) : null;
  const linesAdded = getToolDataNumber(result, "linesAdded") ?? derivedDiffCounts?.linesAdded ?? null;
  const linesRemoved = getToolDataNumber(result, "linesRemoved") ?? derivedDiffCounts?.linesRemoved ?? null;
  const created = result?.created === true;
  const actionLabel = tool.state === "output-available"
    ? created
      ? "Created"
      : "Edited"
    : tool.name === "write"
      ? "Writing"
      : "Editing";

  return {
    actionLabel,
    contentPreview: getToolDataString(input, "content"),
    diff,
    fileName: path.split(/[\\/]/).filter(Boolean).pop() ?? path,
    linesAdded,
    linesRemoved,
    path,
  };
}

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
  const endLine = shownLines ?? lineCount;
  const rangeLabel = endLine && endLine > 0 ? `[1-${endLine}]` : "[1-…]";

  return {
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
    const filePattern = getToolDataString(input, "filePattern");

    return {
      actionLabel: "Search",
      countLabel: typeof count === "number" ? `${count} match${count === 1 ? "" : "es"}` : null,
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
    path,
  };
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

function getApprovalTagLabel(tool: SurfaceToolEntry) {
  if (tool.state === "approval-requested") {
    return "Approval";
  }

  if (isApprovalDenied(tool.approval)) {
    return "Denied";
  }

  if (tool.approval && "approved" in tool.approval && tool.approval.approved) {
    return "Approved";
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
    formatHelperKind(helper.kind),
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

function formatCompactNumber(value: number) {
  return new Intl.NumberFormat("en", {
    maximumFractionDigits: 1,
    notation: "compact",
  }).format(value);
}

type HelperExecutionSummaryMetrics = {
  elapsedText?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  toolUses?: number | null;
};

function formatExecutionSummary({
  elapsedText,
  inputTokens,
  outputTokens,
  toolUses,
}: HelperExecutionSummaryMetrics) {
  const fragments = [
    typeof toolUses === "number" && toolUses > 0
      ? `${toolUses} tool use${toolUses === 1 ? "" : "s"}`
      : null,
    elapsedText ?? null,
    typeof inputTokens === "number" && inputTokens > 0
      ? `input tokens ${formatCompactNumber(inputTokens)}`
      : null,
    typeof outputTokens === "number" && outputTokens > 0
      ? `output tokens ${formatCompactNumber(outputTokens)}`
      : null,
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
    toolName === "agent_research"
    || toolName === "agent_review"
    || toolName === "agent_plan"
    || toolName === "delegate_research"
    || toolName === "delegate_plan_review"
    || toolName === "delegate_code_review"
  );
}

function isVisibleTimelineTool(
  tool: SurfaceToolEntry,
  helperIds: ReadonlySet<string>,
) {
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
    case "run_started":
    case "reasoning_updated":
    case "subagent_usage_updated":
    case "thread_usage_updated":
      return false;
    default:
      return true;
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
  initialPromptRequest = null,
  onConsumeInitialPrompt,
  onContextUsageChange,
  onRunStateChange,
  onSelectAgentProfile,
  onThreadTitleChange,
  providers,
  threadId,
  threadTitle,
}: RuntimeThreadSurfaceProps) {
  const activeProfile = useMemo(
    () => agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0] ?? null,
    [activeAgentProfileId, agentProfiles],
  );
  const [composerError, setComposerError] = useState<string | null>(null);
  const [composerValue, setComposerValue] = useState("");
  const [helpers, setHelpers] = useState<Array<SurfaceHelperEntry>>([]);
  const [helperOpen, setHelperOpen] = useState<Record<string, boolean>>({});
  const [isLoading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Array<SurfaceMessage>>([]);
  const [planArtifact, setPlanArtifact] = useState<unknown>(null);
  const [queueArtifact, setQueueArtifact] = useState<unknown>(null);
  const [runtimeError, setRuntimeError] = useState<SurfaceRuntimeError | null>(null);
  const [runState, setRunState] = useState<RunState>("idle");
  const [snapshotReady, setSnapshotReady] = useState(false);
  const [snapshotThreadId, setSnapshotThreadId] = useState<string | null>(null);
  const [thinkingPlaceholder, setThinkingPlaceholder] = useState<ThinkingPlaceholder | null>(null);
  const [tools, setTools] = useState<Array<SurfaceToolEntry>>([]);
  const [completedToolOpen, setCompletedToolOpen] = useState<Record<string, boolean>>({});
  const previousHelperStatusesRef = useRef<Record<string, SurfaceHelperEntry["status"]>>({});
  const previousToolStatesRef = useRef<Record<string, SurfaceToolState>>({});
  const snapshotLoadRequestRef = useRef(0);
  const streamRef = useRef<ThreadStream | null>(null);
  const submittingRef = useRef(false);
  const handledInitialPromptRequestIdRef = useRef<string | null>(null);
  const thinkingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  const loadSnapshot = useCallback(async () => {
    const requestId = snapshotLoadRequestRef.current + 1;
    snapshotLoadRequestRef.current = requestId;

    if (!threadId) {
      clearScheduledThinkingPhase();
      setMessages([]);
      setLoadError(null);
      setLoading(false);
      onContextUsageChange?.(null);
      setRuntimeError(null);
      setRunState("idle");
      setSnapshotReady(true);
      setSnapshotThreadId(null);
      setThinkingPlaceholder(null);
      onRunStateChange?.("idle");
      return;
    }

    setLoading(true);
    setLoadError(null);

    try {
      const snapshot = await threadLoad(threadId);
      if (snapshotLoadRequestRef.current !== requestId) {
        return;
      }

      const nextState = mapSnapshotToRunState(snapshot);
      const snapshotMessages = snapshot.messages.map(mapSnapshotMessage);
      // Use functional update to ensure we replace the entire list atomically,
      // discarding any local-user optimistic messages that may still be in state.
      setMessages(() => snapshotMessages);
      setTools((snapshot.toolCalls ?? []).map(mapSnapshotTool));
      setHelpers((snapshot.helpers ?? []).map((helper) => mapSnapshotHelper(helper, snapshot.toolCalls ?? [])));
      setRuntimeError(getSnapshotRuntimeError(snapshot));
      setRunState(nextState);
      onContextUsageChange?.(mapRunSummaryToContextUsage(getLatestVisibleRun(snapshot)));
      setSnapshotReady(true);
      setSnapshotThreadId(threadId);
      if (snapshot.thread.title.trim()) {
        onThreadTitleChange?.(snapshot.thread.id, snapshot.thread.title.trim());
      }
      onRunStateChange?.(nextState);
    } catch (error) {
      if (snapshotLoadRequestRef.current !== requestId) {
        return;
      }

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
  }, [clearScheduledThinkingPhase, onContextUsageChange, onRunStateChange, onThreadTitleChange, threadId]);

  useEffect(() => {
    setComposerError(null);
    setHelpers([]);
    setLoadError(null);
    setMessages([]);
    setPlanArtifact(null);
    setQueueArtifact(null);
    setRuntimeError(null);
    setRunState("idle");
    setSnapshotReady(false);
    setSnapshotThreadId(null);
    clearScheduledThinkingPhase();
    setThinkingPlaceholder(null);
    setTools([]);
    void loadSnapshot();
  }, [clearScheduledThinkingPhase, loadSnapshot]);

  useEffect(() => {
    if (!threadId) {
      streamRef.current = null;
      return;
    }

    const stream = new ThreadStream();

    stream.onRawEvent = (event) => {
      if (shouldCompleteThinkingPhase(event)) {
        completeThinkingPhase(event.runId);
      }
    };

    stream.onMessage = (event) => {
      if (event.kind === "delta") {
        setMessages((current) =>
          appendOrReplaceMessage(current, {
            createdAt:
              current.find((entry) => entry.id === event.messageId)?.createdAt
              ?? new Date().toISOString(),
            id: event.messageId,
            messageType: "plain_message",
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
          role: "assistant",
          runId: event.runId,
          content: event.content ?? "",
          status: "completed",
        }),
      );
    };

    stream.onPlan = (event) => {
      setPlanArtifact(event.plan);
    };

    stream.onReasoning = (event) => {
      clearScheduledThinkingPhase();
      setThinkingPlaceholder(null);
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
            role: "assistant",
            runId: event.runId,
            content: event.reasoning,
            status: "streaming",
          },
        ),
      );
    };

    stream.onQueue = (event: QueueEvent) => {
      setQueueArtifact(event.queue);
    };

    stream.onThreadTitle = (event: ThreadTitleEvent) => {
      onThreadTitleChange?.(event.threadId, event.title);
    };

    stream.onUsage = (event: UsageEvent) => {
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
    };

    stream.onHelperEvent = (event: HelperEvent) => {
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
          case "usage":
            return updateHelper(current, event.subtaskId, (entry) => ({
              ...entry,
              ...applyHelperSnapshot(event.snapshot),
              finishedAt: entry?.finishedAt ?? null,
              id: event.subtaskId,
              inputSummary: entry?.inputSummary,
              kind: event.helperKind,
              latestMessage: entry?.latestMessage,
              runId: event.runId,
              startedAt: entry?.startedAt ?? event.startedAt,
              status: entry?.status ?? "running",
              summary: entry?.summary,
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
    };

    stream.onToolEvent = (event) => {
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
          case "running":
            return updateTool(current, event.toolCallId, (entry) => ({
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
            }));
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
    };

    stream.onApproval = (event) => {
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
    };

    stream.onRunStateChange = (state, runId) => {
      setRunState(state);
      onRunStateChange?.(state);

      if (state === "running" || state === "waiting_approval") {
        setRuntimeError(null);
      }

      if (state === "completed" || state === "failed" || state === "cancelled" || state === "interrupted") {
        completeThinkingPhase(runId);
      }

      if (state === "running") {
        return;
      }

      if (state === "completed" || state === "failed" || state === "cancelled" || state === "interrupted") {
        void loadSnapshot();
      }
    };

    stream.onError = (message, runId) => {
      if (runId) {
        setRuntimeError({
          message,
          runId,
        });
        return;
      }

      setComposerError(message);
    };

    streamRef.current = stream;
    return () => {
      clearScheduledThinkingPhase();
      stream.reset();
      streamRef.current = null;
    };
  }, [
    clearScheduledThinkingPhase,
    completeThinkingPhase,
    loadSnapshot,
    onRunStateChange,
    onContextUsageChange,
    onThreadTitleChange,
    scheduleThinkingPhase,
    threadId,
  ]);

  const submitPrompt = useCallback(async (prompt: string) => {
    if (!threadId) {
      setComposerError("This thread is still preparing. Try again in a moment.");
      return;
    }

    if (!activeProfile) {
      setComposerError("Select an agent profile with an enabled model before starting a run.");
      return;
    }

    if (runState === "running" || runState === "waiting_approval") {
      setComposerError("This thread already has an active run.");
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
    setPlanArtifact(null);
    setQueueArtifact(null);
    const userCreatedAt = new Date().toISOString();
    const localUserMessageId = `local-user-${Date.now()}`;

    setMessages((current) => {
      // Remove any previous local-user optimistic messages to avoid duplicates
      // when a snapshot load races with this insertion.
      const withoutStaleLocal = current.filter(
        (entry) => !(entry.role === "user" && entry.id.startsWith("local-user-")),
      );
      return [
        ...withoutStaleLocal,
        {
          createdAt: userCreatedAt,
          id: localUserMessageId,
          messageType: "plain_message",
          role: "user",
          runId: null,
          content: prompt,
          status: "completed",
        },
      ];
    });
    showThinkingPlaceholder(null, userCreatedAt);

    try {
      await streamRef.current?.startRun(threadId, prompt, "default", modelPlan);
    } catch (error) {
      setThinkingPlaceholder(null);
      throw error;
    } finally {
      submittingRef.current = false;
    }
  }, [activeAgentProfileId, activeProfile, agentProfiles, providers, runState, showThinkingPlaceholder, threadId]);

  useEffect(() => {
    const isCurrentThreadSnapshotReady =
      snapshotReady && snapshotThreadId === threadId;
    const initialPromptRequestId = initialPromptRequest?.id ?? null;

    if (
      !initialPromptRequest
      || !isCurrentThreadSnapshotReady
      || runState === "running"
      || runState === "waiting_approval"
      || handledInitialPromptRequestIdRef.current === initialPromptRequestId
    ) {
      return;
    }

    // Parent state clears this request asynchronously, so mark it handled
    // before awaiting to keep effect re-runs from starting the same run twice.
    handledInitialPromptRequestIdRef.current = initialPromptRequestId;
    void submitPrompt(initialPromptRequest.prompt)
      .finally(() => {
        onConsumeInitialPrompt?.(initialPromptRequest.id);
      });
  }, [initialPromptRequest, onConsumeInitialPrompt, runState, snapshotReady, snapshotThreadId, submitPrompt, threadId]);

  const composerStatus: ChatStatus =
    runState === "running" || runState === "waiting_approval" ? "streaming" : "ready";
  const helperIds = useMemo(
    () => new Set(helpers.map((helper) => helper.id)),
    [helpers],
  );
  const visibleTools = useMemo(
    () => tools.filter((tool) => isVisibleTimelineTool(tool, helperIds)),
    [helperIds, tools],
  );
  const hasRuntimeArtifacts =
    Boolean(runtimeError)
    || Boolean(planArtifact)
    || Boolean(queueArtifact)
    || helpers.length > 0
    || visibleTools.length > 0;
  const formattedPlan = planArtifact ? formatPlan(planArtifact) : null;
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
  const planPreviousRole = lastPresentationRole;
  const queuePreviousRole: TimelineRole | null = formattedPlan ? "assistant" : lastPresentationRole;
  const runtimeErrorPreviousRole: TimelineRole | null = queueArtifact
    ? "assistant"
    : formattedPlan
      ? "assistant"
      : lastPresentationRole;

  useEffect(() => {
    const previousToolStates = previousToolStatesRef.current;
    const nextToolStates = Object.fromEntries(visibleTools.map((tool) => [tool.id, tool.state]));

    setCompletedToolOpen((current) => {
      const next: Record<string, boolean> = {};

      for (const tool of visibleTools) {
        const previousState = previousToolStates[tool.id];

        if (previousState !== tool.state) {
          next[tool.id] = tool.state !== "output-available";
          continue;
        }

        if (tool.id in current) {
          next[tool.id] = current[tool.id];
          continue;
        }

        next[tool.id] = tool.state !== "output-available";
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

  const handleSubmit = useCallback(async (message: PromptInputMessage) => {
    const prompt = buildPromptText(message);
    if (!prompt) {
      return;
    }

    setComposerValue("");
    await submitPrompt(prompt);
  }, [submitPrompt]);

  const handleCompletedToolOpenChange = useCallback((toolId: string, open: boolean) => {
    setCompletedToolOpen((current) => (current[toolId] === open ? current : { ...current, [toolId]: open }));
  }, []);

  const handleHelperOpenChange = useCallback((helperId: string, open: boolean) => {
    setHelperOpen((current) => (current[helperId] === open ? current : { ...current, [helperId]: open }));
  }, []);

  const renderToolEntry = useCallback((tool: SurfaceToolEntry, key: string, inset = false) => {
    const fileMutation = getFileMutationPresentation(tool);
    const readTool = getReadToolPresentation(tool);
    const queryTool = getQueryToolPresentation(tool);
    const listTool = getListToolPresentation(tool);
    const approvalTagLabel = getApprovalTagLabel(tool);
    const showStatusLabel = !fileMutation || tool.state !== "output-available";
    const showGenericInput = !fileMutation && tool.input !== undefined;
    const showGenericOutput =
      !fileMutation
      && (tool.state === "output-available" || tool.state === "output-denied" || tool.state === "output-error")
      && (tool.result !== undefined || tool.error);

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
                <span className="shrink-0 font-mono text-[12px] text-app-subtle">
                  {readTool.rangeLabel}
                </span>
              </div>
              <span className={cn("shrink-0 pt-0.5 text-xs", getToolStatusClass(tool.state))}>
                {formatToolStatusLabel(tool.state)}
              </span>
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
              <span className={cn("shrink-0 pt-0.5 text-xs", getToolStatusClass(tool.state))}>
                {formatToolStatusLabel(tool.state)}
              </span>
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
              <span className={cn("shrink-0 pt-0.5 text-xs", getToolStatusClass(tool.state))}>
                {formatToolStatusLabel(tool.state)}
              </span>
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
              if (tool.state !== "output-available") {
                return;
              }

              handleCompletedToolOpenChange(tool.id, open);
            }}
            open={tool.state !== "output-available" ? true : (completedToolOpen[tool.id] ?? false)}
          >
            <CompactCollapsibleHeader
              className={cn(
                "items-start gap-3 text-left text-app-subtle hover:text-app-foreground",
                inset ? "pl-0" : undefined,
              )}
              trailing={showStatusLabel ? (
                <span className={cn("shrink-0 text-xs", getToolStatusClass(tool.state))}>
                  {formatToolStatusLabel(tool.state)}
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
              ) : (
                <div className="flex min-w-0 items-start gap-3">
                  <WrenchIcon className={cn("mt-0.5 size-4 shrink-0", getToolStatusClass(tool.state))} />
                  <span className="truncate text-app-foreground text-sm" title={tool.name}>
                    {tool.name}
                  </span>
                </div>
              )}
            </CompactCollapsibleHeader>
            <CompactCollapsibleContent className={fileMutation ? "pl-0" : "pl-7"}>
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
                        {tool.state === "approval-requested" ? (
                          <div className="ml-auto flex items-center gap-1.5">
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
                              Reject
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
                              Approve
                            </ConfirmationAction>
                          </div>
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

                {showGenericInput ? (
                  <div className="space-y-1.5">
                    <p className="text-[11px] font-medium uppercase tracking-[0.18em] text-app-subtle">
                      Input
                    </p>
                    <div className="overflow-hidden rounded-xl bg-app-surface/20 ring-1 ring-app-border/18">
                      <CodeBlock code={stringifyToolValue(tool.input)} language="json" />
                    </div>
                  </div>
                ) : null}

                {!fileMutation ? (
                  <Confirmation
                    className={cn(
                      "gap-3 rounded-xl border px-3 py-3 shadow-none",
                      isApprovalDenied(tool.approval)
                        ? "border-app-danger/18 bg-app-danger/6"
                        : "border-app-border/18 bg-app-surface/14",
                    )}
                    approval={tool.approval}
                    state={tool.state}
                  >
                    <ConfirmationTitle className="text-sm text-app-muted">
                      <ConfirmationRequest>
                        This tool requires approval before continuing the run.
                      </ConfirmationRequest>
                      <ConfirmationAccepted>
                        <span>{getApprovalReason(tool.approval) || "Approval granted. Execution resumed."}</span>
                      </ConfirmationAccepted>
                      <ConfirmationRejected>
                        <span>{tool.error || getApprovalReason(tool.approval) || "Approval denied."}</span>
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
                        Reject
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
                        Approve
                      </ConfirmationAction>
                    </ConfirmationActions>
                  </Confirmation>
                ) : null}

                {showGenericOutput ? (
                  <div className="space-y-1.5">
                    <p className={cn(
                      "text-[11px] font-medium uppercase tracking-[0.18em]",
                      tool.state === "output-available" ? "text-app-subtle" : "text-app-danger",
                    )}>
                      {tool.state === "output-available" ? "Output" : "Error"}
                    </p>
                    <div
                      className={cn(
                        "overflow-hidden rounded-xl ring-1",
                        tool.state === "output-available"
                          ? "bg-app-surface/20 ring-app-border/18"
                          : "bg-app-danger/6 text-app-danger ring-app-danger/18",
                      )}
                    >
                      <CodeBlock
                        code={stringifyToolValue(tool.state === "output-available" ? tool.result : tool.error ?? tool.result)}
                        language="json"
                      />
                    </div>
                  </div>
                ) : null}
              </div>
            </CompactCollapsibleContent>
          </CompactCollapsible>
        </MessageContent>
      </Message>
    );
  }, [completedToolOpen, handleCompletedToolOpenChange]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-app-canvas">
      <div className="pointer-events-none absolute left-1/2 top-0 h-56 w-[72rem] -translate-x-1/2 rounded-full bg-[radial-gradient(circle,rgba(120,180,255,0.11),transparent_68%)] blur-3xl" />
      <div className="relative min-h-0 flex-1">
        <Conversation className="size-full">
          <ConversationContent className="mx-auto w-full max-w-4xl gap-0 px-6 pb-10 pt-8">
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
                        <MessageResponse>{message.content || (message.status === "streaming" ? "…" : "")}</MessageResponse>
                      </MessageContent>
                    </Message>
                  </div>
                );
              }

              if (entry.kind === "helper") {
                const { helper } = entry;
                const helperSummary = formatHelperSummary(helper);
                const helperToolCounts = formatHelperToolCounts(helper.toolCounts);
                const executionSummary = formatExecutionSummary({
                  elapsedText: formatElapsedSeconds(getHelperElapsedSeconds(helper)),
                  inputTokens: helper.inputTokens,
                  outputTokens: helper.outputTokens,
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
                                className="truncate text-app-foreground text-sm"
                                title={helperSummary}
                              >
                                {helperSummary}
                              </span>
                            </div>
                          </CompactCollapsibleHeader>
                          <CompactCollapsibleContent className="pl-7">
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
                            <CompactCollapsibleFootnote className="pl-7">
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

            {formattedPlan ? (
              <div className={getRoleSpacingClass(planPreviousRole, "assistant")}>
                <Message className="max-w-full" from="assistant">
                  <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                    <Plan className="overflow-hidden rounded-2xl border border-app-border/28 bg-app-surface/28 shadow-none" defaultOpen>
                      <PlanHeader>
                        <div className="space-y-3">
                          <PlanTitle>{formattedPlan.title}</PlanTitle>
                          <PlanDescription>{formattedPlan.description}</PlanDescription>
                        </div>
                        <PlanTrigger />
                      </PlanHeader>
                      <PlanContent className="space-y-3">
                        {formattedPlan.steps.length > 0 ? (
                          <ol className="space-y-2 text-sm leading-6 text-app-muted">
                            {formattedPlan.steps.map((step, index) => (
                              <li className="flex items-start gap-3" key={`${step}-${index}`}>
                                <span className="mt-0.5 inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-app-surface-muted text-[11px] font-semibold text-app-foreground ring-1 ring-app-border/45">
                                  {index + 1}
                                </span>
                                <span className="whitespace-pre-wrap">{step}</span>
                              </li>
                            ))}
                          </ol>
                        ) : (
                          <MessageResponse>{JSON.stringify(planArtifact, null, 2)}</MessageResponse>
                        )}
                      </PlanContent>
                    </Plan>
                  </MessageContent>
                </Message>
              </div>
            ) : null}

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

            {runtimeError ? (
              <div className={getRoleSpacingClass(runtimeErrorPreviousRole, "assistant")}>
                <Message className="max-w-full" from="assistant">
                  <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                      <div className="rounded-2xl border border-app-danger/25 bg-app-danger/8 px-4 py-3 text-sm text-app-danger">
                        <div className="flex items-center gap-2 font-medium">
                          <AlertCircleIcon className="size-4" />
                        {runState === "interrupted" ? "Last run interrupted" : "Last run failed"}
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
          error={composerError}
          onErrorMessageChange={setComposerError}
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
            });
          }}
          onSubmit={(message) => {
            void handleSubmit(message);
          }}
          placeholder="Ask Tiy anything, @ to add files, / for commands, $ for skills"
          providers={providers}
          status={composerStatus}
          value={composerValue}
          onValueChange={setComposerValue}
        />
      </div>
    </div>
  );
}
