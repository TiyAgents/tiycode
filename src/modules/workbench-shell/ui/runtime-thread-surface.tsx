"use client";

import type { ChatStatus } from "ai";
import { AlertCircleIcon, BotIcon, RefreshCcwIcon, SparklesIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Conversation, ConversationContent, ConversationEmptyState, ConversationScrollButton } from "@/components/ai-elements/conversation";
import { Message, MessageContent, MessageResponse } from "@/components/ai-elements/message";
import { Plan, PlanContent, PlanDescription, PlanHeader, PlanTitle, PlanTrigger } from "@/components/ai-elements/plan";
import { Queue, QueueItem, QueueItemContent, QueueItemDescription, QueueItemIndicator, QueueList, QueueSection, QueueSectionContent, QueueSectionLabel, QueueSectionTrigger } from "@/components/ai-elements/queue";
import { Reasoning, ReasoningContent, ReasoningTrigger } from "@/components/ai-elements/reasoning";
import { Confirmation, ConfirmationAccepted, ConfirmationAction, ConfirmationActions, ConfirmationRejected, ConfirmationRequest, ConfirmationTitle } from "@/components/ai-elements/confirmation";
import { Tool, ToolContent, ToolHeader, ToolInput, ToolOutput } from "@/components/ai-elements/tool";
import { buildRunModelPlanFromSelection } from "@/modules/settings-center/model/run-model-plan";
import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import { threadLoad } from "@/services/bridge";
import { ThreadStream, type HelperEvent, type QueueEvent, type RunState } from "@/services/thread-stream";
import type {
  MessageDto,
  RunHelperDto,
  SubagentProgressSnapshot,
  ThreadSnapshotDto,
  ToolCallDto,
} from "@/shared/types/api";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
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

type ToolTimelineEntry = Extract<TimelineEntry, { kind: "tool" }>;

type TimelinePresentationEntry =
  | TimelineEntry
  | {
      kind: "tool-group";
      key: string;
      occurredAt: string;
      tools: SurfaceToolEntry[];
    };

type SurfaceRuntimeError = {
  message: string;
  runId: string;
};

type InitialPromptRequest = {
  id: string;
  prompt: string;
};

type RuntimeThreadSurfaceProps = {
  activeAgentProfileId: string;
  agentProfiles: ReadonlyArray<AgentProfile>;
  initialPromptRequest?: InitialPromptRequest | null;
  onConsumeInitialPrompt?: (id: string) => void;
  onRunStateChange?: (state: RunState) => void;
  onSelectAgentProfile: (id: string) => void;
  providers: ReadonlyArray<ProviderEntry>;
  threadId: string | null;
  threadTitle: string;
};

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

function mapSnapshotHelper(helper: RunHelperDto): SurfaceHelperEntry {
  return {
    completedSteps: 0,
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
    toolCounts: {},
    totalToolCalls: 0,
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

function getSnapshotRuntimeError(snapshot: ThreadSnapshotDto): SurfaceRuntimeError | null {
  const run = getLatestVisibleRun(snapshot);
  if (!run?.errorMessage) {
    return null;
  }

  if (run.status !== "failed" && run.status !== "denied") {
    return null;
  }

  return {
    message: run.errorMessage,
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
  "completedSteps" | "currentAction" | "recentActions" | "toolCounts" | "totalToolCalls"
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
    case "helper_scout":
      return "Research Helper";
    case "helper_planner":
      return "Planning Helper";
    case "helper_reviewer":
      return "Review Helper";
    default:
      return kind;
  }
}

function formatHelperToolCounts(toolCounts: Record<string, number>) {
  return Object.entries(toolCounts ?? {})
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([toolName, count]) => `${toolName} ${count}`);
}

function compareTimelineEntries(left: TimelineEntry, right: TimelineEntry) {
  const timestampOrder = left.occurredAt.localeCompare(right.occurredAt);
  if (timestampOrder !== 0) {
    return timestampOrder;
  }

  return left.key.localeCompare(right.key);
}

function isCompletedToolEntry(entry: TimelineEntry): entry is ToolTimelineEntry {
  return entry.kind === "tool" && entry.tool.state === "output-available";
}

function groupCompletedToolEntries(
  entries: Array<TimelineEntry>,
): Array<TimelinePresentationEntry> {
  const grouped: Array<TimelinePresentationEntry> = [];
  let completedBuffer: Array<ToolTimelineEntry> = [];

  const flushCompletedBuffer = () => {
    if (completedBuffer.length === 0) {
      return;
    }

    if (completedBuffer.length === 1) {
      grouped.push(completedBuffer[0]);
    } else {
      grouped.push({
        kind: "tool-group",
        key: `tool-group:${completedBuffer.map((entry) => entry.tool.id).join(":")}`,
        occurredAt: completedBuffer[0].occurredAt,
        tools: completedBuffer.map((entry) => entry.tool),
      });
    }

    completedBuffer = [];
  };

  for (const entry of entries) {
    if (isCompletedToolEntry(entry)) {
      completedBuffer.push(entry);
      continue;
    }

    flushCompletedBuffer();
    grouped.push(entry);
  }

  flushCompletedBuffer();
  return grouped;
}

export function RuntimeThreadSurface({
  activeAgentProfileId,
  agentProfiles,
  initialPromptRequest = null,
  onConsumeInitialPrompt,
  onRunStateChange,
  onSelectAgentProfile,
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
  const [isLoading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Array<SurfaceMessage>>([]);
  const [planArtifact, setPlanArtifact] = useState<unknown>(null);
  const [queueArtifact, setQueueArtifact] = useState<unknown>(null);
  const [runtimeError, setRuntimeError] = useState<SurfaceRuntimeError | null>(null);
  const [runState, setRunState] = useState<RunState>("idle");
  const [snapshotReady, setSnapshotReady] = useState(false);
  const [tools, setTools] = useState<Array<SurfaceToolEntry>>([]);
  const [completedToolOpen, setCompletedToolOpen] = useState<Record<string, boolean>>({});
  const previousToolStatesRef = useRef<Record<string, SurfaceToolState>>({});
  const streamRef = useRef<ThreadStream | null>(null);

  const loadSnapshot = useCallback(async () => {
    if (!threadId) {
      setMessages([]);
      setLoadError(null);
      setLoading(false);
      setRuntimeError(null);
      setRunState("idle");
      setSnapshotReady(true);
      onRunStateChange?.("idle");
      return;
    }

    setLoading(true);
    setLoadError(null);

    try {
      const snapshot = await threadLoad(threadId);
      const nextState = mapSnapshotToRunState(snapshot);
      setMessages(snapshot.messages.map(mapSnapshotMessage));
      setTools((snapshot.toolCalls ?? []).map(mapSnapshotTool));
      setHelpers((snapshot.helpers ?? []).map(mapSnapshotHelper));
      setRuntimeError(getSnapshotRuntimeError(snapshot));
      setRunState(nextState);
      setSnapshotReady(true);
      onRunStateChange?.(nextState);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setLoadError(message);
      setSnapshotReady(true);
    } finally {
      setLoading(false);
    }
  }, [onRunStateChange, threadId]);

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
    setTools([]);
    void loadSnapshot();
  }, [loadSnapshot]);

  useEffect(() => {
    if (!threadId) {
      streamRef.current = null;
      return;
    }

    const stream = new ThreadStream();

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
      const reasoningMessageId = event.messageId ?? `reasoning-${event.runId}`;
      setMessages((current) =>
        appendOrReplaceMessage(current, {
          createdAt:
            current.find((entry) => entry.id === reasoningMessageId)?.createdAt
            ?? new Date().toISOString(),
          id: reasoningMessageId,
          messageType: "reasoning",
          role: "assistant",
          runId: event.runId,
          content: event.reasoning,
          status: "streaming",
        }),
      );
    };

    stream.onQueue = (event: QueueEvent) => {
      setQueueArtifact(event.queue);
    };

    stream.onHelperEvent = (event: HelperEvent) => {
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
              startedAt: entry?.startedAt ?? new Date().toISOString(),
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
              startedAt: entry?.startedAt ?? new Date().toISOString(),
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
              startedAt: _entry?.startedAt ?? new Date().toISOString(),
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
              startedAt: _entry?.startedAt ?? new Date().toISOString(),
              status: "failed",
              summary: undefined,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
        }
      });
    };

    stream.onToolEvent = (event) => {
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

    stream.onRunStateChange = (state) => {
      setRunState(state);
      onRunStateChange?.(state);

      if (state === "running" || state === "waiting_approval") {
        setRuntimeError(null);
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
      stream.reset();
      streamRef.current = null;
    };
  }, [loadSnapshot, onRunStateChange, threadId]);

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

    const modelPlan = buildRunModelPlanFromSelection(
      activeAgentProfileId,
      agentProfiles,
      providers,
    );

    if (!modelPlan) {
      setComposerError("No enabled model is available for the selected profile.");
      return;
    }

    setComposerError(null);
    setRuntimeError(null);
    setPlanArtifact(null);
    setQueueArtifact(null);
    setMessages((current) => [
      ...current,
      {
        createdAt: new Date().toISOString(),
        id: `local-user-${Date.now()}`,
        messageType: "plain_message",
        role: "user",
        runId: null,
        content: prompt,
        status: "completed",
      },
    ]);

    await streamRef.current?.startRun(threadId, prompt, "default", modelPlan);
  }, [activeAgentProfileId, activeProfile, agentProfiles, providers, runState, threadId]);

  useEffect(() => {
    if (!initialPromptRequest || !snapshotReady) {
      return;
    }

    void submitPrompt(initialPromptRequest.prompt)
      .finally(() => {
        onConsumeInitialPrompt?.(initialPromptRequest.id);
      });
  }, [initialPromptRequest, onConsumeInitialPrompt, snapshotReady, submitPrompt]);

  const composerStatus: ChatStatus =
    runState === "running" || runState === "waiting_approval" ? "streaming" : "ready";
  const hasRuntimeArtifacts =
    Boolean(runtimeError)
    || Boolean(planArtifact)
    || Boolean(queueArtifact)
    || helpers.length > 0
    || tools.length > 0;
  const formattedPlan = planArtifact ? formatPlan(planArtifact) : null;
  const timelineEntries = useMemo<Array<TimelineEntry>>(
    () =>
      [
        ...messages.map((message) => ({
          kind: "message" as const,
          key: `message:${message.id}`,
          occurredAt: message.createdAt,
          message,
        })),
        ...helpers.map((helper) => ({
          kind: "helper" as const,
          key: `helper:${helper.id}`,
          occurredAt: helper.startedAt,
          helper,
        })),
        ...tools.map((tool) => ({
          kind: "tool" as const,
          key: `tool:${tool.id}`,
          occurredAt: tool.startedAt,
          tool,
        })),
      ].sort(compareTimelineEntries),
    [helpers, messages, tools],
  );
  const presentationEntries = useMemo<Array<TimelinePresentationEntry>>(
    () => groupCompletedToolEntries(timelineEntries),
    [timelineEntries],
  );

  useEffect(() => {
    const previousToolStates = previousToolStatesRef.current;
    const nextToolStates = Object.fromEntries(tools.map((tool) => [tool.id, tool.state]));

    setCompletedToolOpen((current) => {
      const next: Record<string, boolean> = {};

      for (const tool of tools) {
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
  }, [tools]);

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

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-app-canvas">
      <div className="pointer-events-none absolute left-1/2 top-0 h-56 w-[72rem] -translate-x-1/2 rounded-full bg-[radial-gradient(circle,rgba(120,180,255,0.11),transparent_68%)] blur-3xl" />
      <div className="relative min-h-0 flex-1">
        <Conversation className="size-full">
          <ConversationContent className="mx-auto w-full max-w-4xl gap-6 px-6 pb-10 pt-8">
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

            {presentationEntries.map((entry) => {
              if (entry.kind === "message") {
                const { message } = entry;

                if (message.messageType === "reasoning") {
                  return (
                    <Message className="max-w-full" from="assistant" key={entry.key}>
                      <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                        <Reasoning
                          className="w-full bg-transparent px-0 py-0"
                          defaultOpen={message.status === "streaming" || runState === "running"}
                          isStreaming={message.status === "streaming"}
                        >
                          <ReasoningTrigger />
                          <ReasoningContent>{message.content}</ReasoningContent>
                        </Reasoning>
                      </MessageContent>
                    </Message>
                  );
                }

                return (
                  <Message
                    className={message.role === "assistant" ? "max-w-full" : undefined}
                    from={message.role}
                    key={entry.key}
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
                );
              }

              if (entry.kind === "helper") {
                const { helper } = entry;
                return (
                  <Message className="max-w-full" from="assistant" key={entry.key}>
                    <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                      <Queue className="rounded-2xl border border-app-border/24 bg-app-surface/16 p-2 shadow-none">
                        <QueueSection defaultOpen>
                          <QueueSectionTrigger>
                            <QueueSectionLabel count={1} label="Helper Task" />
                          </QueueSectionTrigger>
                          <QueueSectionContent>
                            <QueueList>
                              <QueueItem>
                                <div className="flex items-start gap-3">
                                  <QueueItemIndicator completed={helper.status === "completed"} />
                                  <QueueItemContent completed={helper.status === "completed"}>
                                    {formatHelperKind(helper.kind)}
                                  </QueueItemContent>
                                  <Badge
                                    className={cn(
                                      "ml-auto rounded-full",
                                      helper.status === "failed"
                                        ? "bg-app-danger/10 text-app-danger"
                                        : helper.status === "completed"
                                          ? "bg-app-success/10 text-app-success"
                                          : "bg-app-info/10 text-app-info",
                                    )}
                                    variant="outline"
                                  >
                                    {helper.status}
                                  </Badge>
                                </div>
                                {helper.inputSummary ? (
                                  <QueueItemDescription completed={helper.status === "completed"}>
                                    {helper.inputSummary}
                                  </QueueItemDescription>
                                ) : null}
                                {helper.totalToolCalls > 0 ? (
                                  <QueueItemDescription completed={helper.status === "completed"}>
                                    {`${helper.totalToolCalls} tool calls, ${helper.completedSteps} finished`}
                                  </QueueItemDescription>
                                ) : null}
                                {formatHelperToolCounts(helper.toolCounts).length > 0 ? (
                                  <QueueItemDescription completed={helper.status === "completed"}>
                                    {formatHelperToolCounts(helper.toolCounts).join(" · ")}
                                  </QueueItemDescription>
                                ) : null}
                                {helper.currentAction ? (
                                  <QueueItemDescription completed={helper.status === "completed"}>
                                    {`Current: ${helper.currentAction}`}
                                  </QueueItemDescription>
                                ) : null}
                                {helper.latestMessage ? (
                                  <QueueItemDescription completed={helper.status === "completed"}>
                                    {helper.latestMessage}
                                  </QueueItemDescription>
                                ) : null}
                                {helper.recentActions.length > 0 ? (
                                  <div className="space-y-1">
                                    {helper.recentActions.slice(-3).map((action, index) => (
                                      <QueueItemDescription
                                        completed={helper.status === "completed"}
                                        key={`${helper.id}-action-${index}`}
                                      >
                                        {action}
                                      </QueueItemDescription>
                                    ))}
                                  </div>
                                ) : null}
                                {helper.summary ? (
                                  <QueueItemDescription completed={helper.status === "completed"}>
                                    {helper.summary}
                                  </QueueItemDescription>
                                ) : null}
                                {helper.error ? (
                                  <QueueItemDescription className="text-app-danger">
                                    {helper.error}
                                  </QueueItemDescription>
                                ) : null}
                              </QueueItem>
                            </QueueList>
                          </QueueSectionContent>
                        </QueueSection>
                      </Queue>
                    </MessageContent>
                  </Message>
                );
              }

              if (entry.kind === "tool-group") {
                return (
                  <Message className="max-w-full" from="assistant" key={entry.key}>
                    <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                      <Tool
                        className="rounded-2xl border border-app-border/28 bg-app-surface/24 shadow-none"
                        defaultOpen={false}
                      >
                        <ToolHeader
                          state="output-available"
                          title={`Tools × ${entry.tools.length}`}
                          toolName="tools"
                          type="dynamic-tool"
                        />
                        <ToolContent className="space-y-3">
                          {entry.tools.map((tool) => (
                            <div
                              className="space-y-3 rounded-xl border border-app-border/24 bg-app-surface/18 p-3"
                              key={tool.id}
                            >
                              <div className="flex items-center gap-2">
                                <span className="text-sm font-medium text-app-foreground">{tool.name}</span>
                                <Badge className="rounded-full bg-app-success/10 text-app-success" variant="outline">
                                  completed
                                </Badge>
                              </div>
                              {tool.input !== undefined ? <ToolInput input={tool.input} /> : null}
                              <ToolOutput errorText={undefined} output={tool.result} />
                            </div>
                          ))}
                        </ToolContent>
                      </Tool>
                    </MessageContent>
                  </Message>
                );
              }

              const { tool } = entry;
              return (
                <Message className="max-w-full" from="assistant" key={entry.key}>
                  <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                    <Tool
                      className="rounded-2xl border border-app-border/28 bg-app-surface/24 shadow-none"
                      onOpenChange={(open) => {
                        if (tool.state !== "output-available") {
                          return;
                        }

                        handleCompletedToolOpenChange(tool.id, open);
                      }}
                      open={tool.state !== "output-available" ? true : (completedToolOpen[tool.id] ?? false)}
                    >
                      <ToolHeader state={tool.state} title={tool.name} toolName={tool.name} type="dynamic-tool" />
                      <ToolContent>
                        {tool.input !== undefined ? <ToolInput input={tool.input} /> : null}

                        <Confirmation approval={tool.approval} state={tool.state}>
                          <ConfirmationTitle>
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

                          <ConfirmationActions>
                            <ConfirmationAction
                              onClick={() => {
                                if (!streamRef.current?.runId) {
                                  return;
                                }

                                void streamRef.current.respondToApproval(tool.id, streamRef.current.runId, false);
                              }}
                              variant="outline"
                            >
                              Reject
                            </ConfirmationAction>
                            <ConfirmationAction
                              onClick={() => {
                                if (!streamRef.current?.runId) {
                                  return;
                                }

                                void streamRef.current.respondToApproval(tool.id, streamRef.current.runId, true);
                              }}
                            >
                              Approve
                            </ConfirmationAction>
                          </ConfirmationActions>
                        </Confirmation>

                        {tool.state === "output-available" || tool.state === "output-denied" || tool.state === "output-error" ? (
                          <ToolOutput errorText={tool.state === "output-available" ? undefined : tool.error} output={tool.result} />
                        ) : null}
                      </ToolContent>
                    </Tool>
                  </MessageContent>
                </Message>
              );
            })}

            {formattedPlan ? (
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
            ) : null}

            {queueArtifact ? (
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
            ) : null}

            {runtimeError ? (
              <Message className="max-w-full" from="assistant">
                <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                  <div className="rounded-2xl border border-app-danger/25 bg-app-danger/8 px-4 py-3 text-sm text-app-danger">
                    <div className="flex items-center gap-2 font-medium">
                      <AlertCircleIcon className="size-4" />
                      Last run failed
                    </div>
                    <p className="mt-2 whitespace-pre-wrap leading-6 text-app-danger/90">{runtimeError.message}</p>
                  </div>
                </MessageContent>
              </Message>
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

            void streamRef.current?.cancelRun(threadId);
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
