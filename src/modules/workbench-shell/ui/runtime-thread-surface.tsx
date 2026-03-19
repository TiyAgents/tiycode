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
import type { MessageDto, SubagentProgressSnapshot, ThreadSnapshotDto } from "@/shared/types/api";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import type { PromptInputMessage } from "@/components/ai-elements/prompt-input";
import { WorkbenchPromptComposer } from "@/modules/workbench-shell/ui/workbench-prompt-composer";

type SurfaceMessage = {
  id: string;
  role: "user" | "assistant" | "system";
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
  id: string;
  input?: unknown;
  name: string;
  result?: unknown;
  state: SurfaceToolState;
};

type SurfaceHelperEntry = {
  completedSteps: number;
  currentAction?: string | null;
  error?: string;
  id: string;
  kind: string;
  latestMessage?: string;
  recentActions: string[];
  status: "pending" | "completed" | "failed";
  summary?: string | null;
  toolCounts: Record<string, number>;
  totalToolCalls: number;
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
    id: message.id,
    role:
      message.role === "user" || message.role === "assistant" || message.role === "system"
        ? message.role
        : "assistant",
    content: message.contentMarkdown,
    status: message.status,
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
    completedSteps: snapshot.completedSteps,
    currentAction: snapshot.currentAction,
    recentActions: snapshot.recentActions,
    toolCounts: snapshot.toolCounts,
    totalToolCalls: snapshot.totalToolCalls,
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
  return Object.entries(toolCounts)
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([toolName, count]) => `${toolName} ${count}`);
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
  const [reasoning, setReasoning] = useState("");
  const [runState, setRunState] = useState<RunState>("idle");
  const [snapshotReady, setSnapshotReady] = useState(false);
  const [tools, setTools] = useState<Array<SurfaceToolEntry>>([]);
  const streamRef = useRef<ThreadStream | null>(null);

  const loadSnapshot = useCallback(async () => {
    if (!threadId) {
      setMessages([]);
      setLoadError(null);
      setLoading(false);
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
    setReasoning("");
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
            id: event.messageId,
            role: "assistant",
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
          id: event.messageId,
          role: "assistant",
          content: event.content ?? "",
          status: "completed",
        }),
      );
    };

    stream.onPlan = (event) => {
      setPlanArtifact(event.plan);
    };

    stream.onReasoning = (event) => {
      setReasoning(event.reasoning);
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
              id: event.subtaskId,
              kind: event.helperKind,
              latestMessage: undefined,
              status: "pending",
              summary: entry?.summary,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
          case "progress":
            return updateHelper(current, event.subtaskId, (entry) => ({
              ...applyHelperSnapshot(event.snapshot),
              error: entry?.error,
              id: event.subtaskId,
              kind: event.helperKind,
              latestMessage: event.message,
              status: entry?.status ?? "pending",
              summary: entry?.summary,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
          case "completed":
            return updateHelper(current, event.subtaskId, (_entry) => ({
              ...applyHelperSnapshot(event.snapshot),
              error: undefined,
              id: event.subtaskId,
              kind: event.helperKind,
              latestMessage: undefined,
              status: "completed",
              summary: event.summary,
              totalToolCalls: event.snapshot.totalToolCalls,
            }));
          case "failed":
            return updateHelper(current, event.subtaskId, (_entry) => ({
              ...applyHelperSnapshot(event.snapshot),
              error: event.error,
              id: event.subtaskId,
              kind: event.helperKind,
              latestMessage: undefined,
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
              id: event.toolCallId,
              input: event.toolInput,
              name: event.toolName ?? entry?.name ?? "tool",
              result: entry?.result,
              state: entry?.state === "approval-requested" ? "approval-requested" : "input-streaming",
            }));
          case "running":
            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: undefined,
              id: event.toolCallId,
              input: entry?.input,
              name: entry?.name ?? "tool",
              result: undefined,
              state: "input-available",
            }));
          case "completed":
            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: undefined,
              id: event.toolCallId,
              input: entry?.input,
              name: entry?.name ?? "tool",
              result: event.result,
              state: "output-available",
            }));
          case "failed": {
            const denied =
              isApprovalDenied(current.find((entry) => entry.id === event.toolCallId)?.approval)
              || event.error?.toLowerCase().includes("denied");

            return updateTool(current, event.toolCallId, (entry) => ({
              approval: entry?.approval,
              error: event.error,
              id: event.toolCallId,
              input: entry?.input,
              name: entry?.name ?? "tool",
              result: undefined,
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
          id: event.toolCallId,
          input: event.toolInput ?? entry?.input,
          name: event.toolName ?? entry?.name ?? "tool",
          result: entry?.result,
          state: event.kind === "required" ? "approval-requested" : "approval-responded",
        })),
      );
    };

    stream.onRunStateChange = (state) => {
      setRunState(state);
      onRunStateChange?.(state);

      if (state === "running") {
        return;
      }

      if (state === "completed" || state === "failed" || state === "cancelled" || state === "interrupted") {
        void loadSnapshot();
      }
    };

    stream.onError = (message) => {
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
    setPlanArtifact(null);
    setQueueArtifact(null);
    setReasoning("");
    setHelpers([]);
    setTools([]);
    setMessages((current) => [
      ...current,
      {
        id: `local-user-${Date.now()}`,
        role: "user",
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
  const activeHelpersCount = helpers.filter((entry) => entry.status === "pending").length;
  const hasRuntimeArtifacts =
    Boolean(reasoning)
    || Boolean(planArtifact)
    || Boolean(queueArtifact)
    || helpers.length > 0
    || tools.length > 0;
  const formattedPlan = planArtifact ? formatPlan(planArtifact) : null;

  const handleSubmit = useCallback(async (message: PromptInputMessage) => {
    const prompt = buildPromptText(message);
    if (!prompt) {
      return;
    }

    setComposerValue("");
    await submitPrompt(prompt);
  }, [submitPrompt]);

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

            {!isLoading && !loadError && messages.length === 0 ? (
              <ConversationEmptyState
                description="Ask Tiy to inspect the workspace, run tools, or plan the next task."
                icon={<BotIcon className="size-5" />}
                title={threadTitle || "No messages yet"}
              />
            ) : null}

            {messages.map((message) => (
              <Message
                className={message.role === "assistant" ? "max-w-full" : undefined}
                from={message.role}
                key={message.id}
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
            ))}

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

            {reasoning ? (
              <Message className="max-w-full" from="assistant">
                <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                  <Reasoning className="w-full bg-transparent px-0 py-0" defaultOpen={runState === "running"}>
                    <ReasoningTrigger />
                    <ReasoningContent>{reasoning}</ReasoningContent>
                  </Reasoning>
                </MessageContent>
              </Message>
            ) : null}

            {queueArtifact || helpers.length > 0 ? (
              <Message className="max-w-full" from="assistant">
                <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                  <Queue className="rounded-2xl border border-app-border/24 bg-app-surface/16 p-2 shadow-none">
                    {helpers.length > 0 ? (
                      <QueueSection defaultOpen>
                        <QueueSectionTrigger>
                          <QueueSectionLabel
                            count={helpers.length}
                            label={activeHelpersCount > 0 ? "Helper Tasks Active" : "Helper Tasks"}
                          />
                        </QueueSectionTrigger>
                        <QueueSectionContent>
                          <QueueList>
                            {helpers.map((helper) => (
                              <QueueItem key={helper.id}>
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
                            ))}
                          </QueueList>
                        </QueueSectionContent>
                      </QueueSection>
                    ) : null}

                    {queueArtifact ? (
                      <div className={cn(helpers.length > 0 && "border-t border-app-border/45 pt-2")}>
                        <div className="px-3 py-2 text-sm font-medium text-app-foreground">Runtime Queue</div>
                        <div className="rounded-xl bg-app-surface/45 px-3 py-3 text-sm text-app-muted">
                          <MessageResponse>{JSON.stringify(queueArtifact, null, 2)}</MessageResponse>
                        </div>
                      </div>
                    ) : null}
                  </Queue>
                </MessageContent>
              </Message>
            ) : null}

            {tools.length > 0 ? (
              <Message className="max-w-full" from="assistant">
                <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                  {tools.map((tool) => (
                    <Tool
                      className="rounded-2xl border border-app-border/28 bg-app-surface/24 shadow-none"
                      defaultOpen={tool.state !== "output-available"}
                      key={tool.id}
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
                  ))}
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
