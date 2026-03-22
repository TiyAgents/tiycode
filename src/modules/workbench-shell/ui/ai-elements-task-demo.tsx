"use client";

import type { ChatStatus } from "ai";
import { SparklesIcon } from "lucide-react";
import { nanoid } from "nanoid";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChainOfThought,
  ChainOfThoughtContent,
  ChainOfThoughtHeader,
  ChainOfThoughtSearchResult,
  ChainOfThoughtSearchResults,
  ChainOfThoughtStep,
} from "@/components/ai-elements/chain-of-thought";
import {
  Confirmation,
  ConfirmationAccepted,
  ConfirmationAction,
  ConfirmationActions,
  ConfirmationRejected,
  ConfirmationRequest,
  ConfirmationTitle,
} from "@/components/ai-elements/confirmation";
import {
  Conversation,
  ConversationContent,
  ConversationScrollButton,
} from "@/components/ai-elements/conversation";
import {
  Message,
  MessageContent,
  MessageResponse,
} from "@/components/ai-elements/message";
import {
  Plan,
  PlanContent,
  PlanDescription,
  PlanHeader,
  PlanTitle,
  PlanTrigger,
} from "@/components/ai-elements/plan";
import {
  Queue,
  QueueItem,
  QueueItemContent,
  QueueItemDescription,
  QueueItemIndicator,
  QueueList,
  QueueSection,
  QueueSectionContent,
  QueueSectionLabel,
  QueueSectionTrigger,
} from "@/components/ai-elements/queue";
import {
  Reasoning,
  ReasoningContent,
  ReasoningTrigger,
} from "@/components/ai-elements/reasoning";
import { Source, Sources, SourcesContent, SourcesTrigger } from "@/components/ai-elements/sources";
import type { ComposerSubmission } from "@/modules/workbench-shell/model/composer-commands";
import { Tool, ToolContent, ToolHeader, ToolInput, ToolOutput } from "@/components/ai-elements/tool";
import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import {
  AI_ELEMENTS_CHAIN_STEPS,
  AI_ELEMENTS_INITIAL_QUEUE,
  AI_ELEMENTS_PLAN,
  AI_ELEMENTS_REASONING_TEXT,
  AI_ELEMENTS_REQUEST,
  AI_ELEMENTS_SOURCES,
  AI_ELEMENTS_SUGGESTIONS,
  AI_ELEMENTS_TOOL_INPUT,
  AI_ELEMENTS_TOOL_SUCCESS_OUTPUT,
  buildProfileAwareFollowUp,
  type DemoQueueItem,
} from "@/modules/workbench-shell/model/ai-elements-task-demo";
import {
  ComposerMessageAttachments,
  mapComposerAttachments,
  WorkbenchPromptComposer,
} from "@/modules/workbench-shell/ui/workbench-prompt-composer";
import { Badge } from "@/shared/ui/badge";

type DemoToolState =
  | "approval-requested"
  | "approval-responded"
  | "input-available"
  | "output-available"
  | "output-denied";

type DemoApprovalState =
  | { id: string }
  | { approved: true; id: string; reason?: string }
  | { approved: false; id: string; reason?: string };

type FollowUpEntry = {
  attachments?: Array<DemoAttachment>;
  id: string;
  label?: string;
  role: "assistant" | "user";
  text: string;
};

type DemoAttachment = ReturnType<typeof mapComposerAttachments>[number];

const TOOL_APPROVAL_ID = "ai-elements-install";
const TOOL_RUN_DELAY_MS = 500;
const TOOL_COMPLETE_DELAY_MS = 1400;
const FOLLOW_UP_STREAM_DELAY_MS = 280;
const FOLLOW_UP_COMPLETE_DELAY_MS = 920;

function getQueueAfterApproval() {
  return AI_ELEMENTS_INITIAL_QUEUE.map((item) => {
    if (item.id === "install-primitives") {
      return { ...item, status: "completed" as const, description: "Native AI Elements primitives are now in place." };
    }

    if (item.id === "replace-thread-body") {
      return { ...item, status: "completed" as const, description: "The existing thread surface is ready to render the new single-threaded demo." };
    }

    if (item.id === "validate-profile-follow-up") {
      return { ...item, description: "Waiting for one more prompt to validate that the active profile changes the next local response." };
    }

    return item;
  });
}

function getQueueAfterRejection() {
  return AI_ELEMENTS_INITIAL_QUEUE.map((item) => {
    if (item.id === "install-primitives") {
      return { ...item, description: "Blocked: the install step was denied before the native primitives could be applied." };
    }

    if (item.id === "replace-thread-body") {
      return { ...item, description: "Cannot swap the thread surface until the install step is approved." };
    }

    if (item.id === "verify-shell") {
      return { ...item, description: "Verification is blocked until the integration step is approved." };
    }

    return item;
  });
}

function getQueueAfterFollowUp(profile: AgentProfile, toolState: DemoToolState) {
  return getQueueAfterApproval().map((item) => {
    if (item.id === "validate-profile-follow-up") {
      return {
        ...item,
        status: "completed" as const,
        description: `Validated with ${profile.name}; the next local response now reflects the active profile tone.`,
      };
    }

    if (item.id === "verify-shell") {
      return {
        ...item,
        description:
          toolState === "output-available"
            ? "The queue is now aligned with the successful thread replacement and is ready for typecheck/build verification."
            : "Profile switching was validated, but final verification is still blocked until the install step is approved.",
        status: toolState === "output-available" ? ("completed" as const) : item.status,
      };
    }

    return item;
  });
}

function getExecutionSummary(toolState: DemoToolState) {
  if (toolState === "approval-requested") {
    return "安装与接线动作已经准备好，等待你批准后继续。";
  }

  if (toolState === "approval-responded" || toolState === "input-available") {
    return "批准已收到，正在把官方 AI Elements primitives 接入当前线程表面。";
  }

  if (toolState === "output-denied") {
    return "批准被拒绝后，安装步骤保持阻塞状态，Queue 也会继续标记为待处理。";
  }

  return "官方 AI Elements primitives 已经准备到位，线程可以继续验证 profile-aware composer 与后续执行流。";
}

export function AiElementsTaskDemo({
  activeAgentProfileId,
  agentProfiles,
  onSelectAgentProfile,
  providers,
}: {
  activeAgentProfileId: string;
  agentProfiles: ReadonlyArray<AgentProfile>;
  onSelectAgentProfile: (id: string) => void;
  providers: ReadonlyArray<ProviderEntry>;
}) {
  const [composerValue, setComposerValue] = useState("");
  const [composerError, setComposerError] = useState<string | null>(null);
  const [followUpEntries, setFollowUpEntries] = useState<Array<FollowUpEntry>>([]);
  const [queueItems, setQueueItems] = useState<Array<DemoQueueItem>>(() =>
    AI_ELEMENTS_INITIAL_QUEUE.map((item) => ({ ...item })),
  );
  const [status, setStatus] = useState<ChatStatus>("ready");
  const [toolState, setToolState] = useState<DemoToolState>("approval-requested");
  const [approval, setApproval] = useState<DemoApprovalState>({
    id: TOOL_APPROVAL_ID,
  });
  const toolTimerIdsRef = useRef<Array<number>>([]);
  const followUpTimerIdsRef = useRef<Array<number>>([]);

  const activeProfile = useMemo(
    () => agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0] ?? null,
    [activeAgentProfileId, agentProfiles],
  );

  const clearToolTimers = useCallback(() => {
    toolTimerIdsRef.current.forEach((timerId) => window.clearTimeout(timerId));
    toolTimerIdsRef.current = [];
  }, []);

  const clearFollowUpTimers = useCallback(() => {
    followUpTimerIdsRef.current.forEach((timerId) => window.clearTimeout(timerId));
    followUpTimerIdsRef.current = [];
  }, []);

  useEffect(() => {
    return () => {
      clearToolTimers();
      clearFollowUpTimers();
    };
  }, [clearFollowUpTimers, clearToolTimers]);

  const scheduleToolTimer = useCallback((callback: () => void, delay: number) => {
    const timerId = window.setTimeout(callback, delay);
    toolTimerIdsRef.current.push(timerId);
  }, []);

  const scheduleFollowUpTimer = useCallback((callback: () => void, delay: number) => {
    const timerId = window.setTimeout(callback, delay);
    followUpTimerIdsRef.current.push(timerId);
  }, []);

  const handleApproveTool = useCallback(() => {
    clearToolTimers();
    setApproval({ approved: true, id: TOOL_APPROVAL_ID });
    setToolState("approval-responded");

    scheduleToolTimer(() => {
      setToolState("input-available");
    }, TOOL_RUN_DELAY_MS);

    scheduleToolTimer(() => {
      setToolState("output-available");
      setQueueItems(getQueueAfterApproval());
    }, TOOL_COMPLETE_DELAY_MS);
  }, [clearToolTimers, scheduleToolTimer]);

  const handleRejectTool = useCallback(() => {
    clearToolTimers();
    setApproval({
      approved: false,
      id: TOOL_APPROVAL_ID,
      reason: "Keep the current shared primitives intact until the thread replacement is explicitly approved.",
    });
    setToolState("output-denied");
    setQueueItems(getQueueAfterRejection());
  }, [clearToolTimers]);

  const handleStopFollowUp = useCallback(() => {
    clearFollowUpTimers();
    setStatus("ready");
  }, [clearFollowUpTimers]);

  const handleSubmit = useCallback(
    (submission: ComposerSubmission) => {
      const nextText = submission.displayText?.trim();
      const nextAttachments = mapComposerAttachments(submission.rawMessage.files);
      const textForThread = nextText || (nextAttachments.length > 0 ? `请基于这 ${nextAttachments.length} 个附件继续细化 demo。` : "");

      if ((!textForThread && nextAttachments.length === 0) || !activeProfile) {
        return;
      }

      clearFollowUpTimers();
      setComposerError(null);
      setComposerValue("");
      setStatus("submitted");
      setFollowUpEntries((current) => [
        ...current,
        {
          attachments: nextAttachments,
          id: nanoid(),
          role: "user",
          text: textForThread,
        },
      ]);

      scheduleFollowUpTimer(() => {
        setStatus("streaming");
      }, FOLLOW_UP_STREAM_DELAY_MS);

      scheduleFollowUpTimer(() => {
        const reply = buildProfileAwareFollowUp(
          activeProfile,
          textForThread,
          nextAttachments.map((attachment) => attachment.name),
          providers,
        );
        setFollowUpEntries((current) => [
          ...current,
          {
            id: nanoid(),
            label: reply.label,
            role: "assistant",
            text: reply.body,
          },
        ]);
        setQueueItems(getQueueAfterFollowUp(activeProfile, toolState));
        setStatus("ready");
      }, FOLLOW_UP_COMPLETE_DELAY_MS);
    },
    [activeProfile, clearFollowUpTimers, providers, scheduleFollowUpTimer, toolState],
  );

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-app-canvas">
      <div className="pointer-events-none absolute left-1/2 top-0 h-56 w-[72rem] -translate-x-1/2 rounded-full bg-[radial-gradient(circle,rgba(120,180,255,0.11),transparent_68%)] blur-3xl" />
      <div className="relative min-h-0 flex-1">
        <Conversation className="size-full">
          <ConversationContent className="mx-auto w-full max-w-4xl gap-6 px-6 pb-10 pt-8">
            <Message from="user">
              <MessageContent className="rounded-2xl bg-app-surface/62 px-4 py-3 shadow-none backdrop-blur-sm">
                {AI_ELEMENTS_REQUEST}
              </MessageContent>
            </Message>

            <Message className="max-w-full" from="assistant">
              <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                <div className="mb-3 flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.16em] text-app-subtle">
                  <SparklesIcon className="size-3.5" />
                  Native AI Elements walkthrough
                </div>
                <MessageResponse>
                  我会把这次重构收束成一条真实的任务线程，让每个 AI Elements 组件都在同一条会话里承担明确职责。
                </MessageResponse>

                <Plan className="mt-5 overflow-hidden rounded-2xl border border-app-border/28 bg-app-surface/28 shadow-none">
                  <PlanHeader>
                    <div className="space-y-3">
                      <PlanTitle>{AI_ELEMENTS_PLAN.title}</PlanTitle>
                      <PlanDescription>{AI_ELEMENTS_PLAN.description}</PlanDescription>
                    </div>
                    <PlanTrigger />
                  </PlanHeader>
                  <PlanContent className="space-y-4">
                    <div className="text-sm leading-6 text-app-muted">{AI_ELEMENTS_PLAN.overview}</div>
                    <ol className="space-y-2 text-sm leading-6 text-app-muted">
                      {AI_ELEMENTS_PLAN.steps.map((step, index) => (
                        <li key={step} className="flex items-start gap-3">
                          <span className="mt-0.5 inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-app-surface-muted text-[11px] font-semibold text-app-foreground ring-1 ring-app-border/45">
                            {index + 1}
                          </span>
                          <span>{step}</span>
                        </li>
                      ))}
                    </ol>
                  </PlanContent>
                </Plan>

                <Queue className="mt-5 rounded-2xl border border-app-border/24 bg-app-surface/16 p-2 shadow-none">
                  <QueueSection defaultOpen>
                    <QueueSectionTrigger>
                      <QueueSectionLabel count={queueItems.length} label="Implementation Queue" />
                    </QueueSectionTrigger>
                    <QueueSectionContent>
                      <QueueList>
                        {queueItems.map((item) => (
                          <QueueItem key={item.id}>
                            <div className="flex items-start gap-3">
                              <QueueItemIndicator completed={item.status === "completed"} />
                              <QueueItemContent completed={item.status === "completed"}>{item.title}</QueueItemContent>
                            </div>
                            <QueueItemDescription completed={item.status === "completed"}>
                              {item.description}
                            </QueueItemDescription>
                          </QueueItem>
                        ))}
                      </QueueList>
                    </QueueSectionContent>
                  </QueueSection>
                </Queue>
              </MessageContent>
            </Message>

            <Message className="max-w-full" from="assistant">
              <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                <MessageResponse>
                  在真正替换已有会话区之前，我先把边界和组件映射想清楚，这样 New Thread 和现有 workbench chrome 就不会被一起牵动。
                </MessageResponse>

                <Reasoning className="mt-5 w-full bg-transparent px-0 py-0" defaultOpen>
                  <ReasoningTrigger />
                  <ReasoningContent>{AI_ELEMENTS_REASONING_TEXT}</ReasoningContent>
                </Reasoning>

                <ChainOfThought className="mt-5" defaultOpen>
                  <ChainOfThoughtHeader>Implementation checkpoints</ChainOfThoughtHeader>
                  <ChainOfThoughtContent>
                    {AI_ELEMENTS_CHAIN_STEPS.map((step) => (
                      <ChainOfThoughtStep key={step.id} label={step.label} status={step.status}>
                        {step.id === "map-components" ? (
                          <ChainOfThoughtSearchResults>
                            {["plan", "queue", "prompt-input", "tool", "sources"].map((component) => (
                              <ChainOfThoughtSearchResult key={component}>{component}</ChainOfThoughtSearchResult>
                            ))}
                          </ChainOfThoughtSearchResults>
                        ) : null}
                      </ChainOfThoughtStep>
                    ))}
                  </ChainOfThoughtContent>
                </ChainOfThought>
              </MessageContent>
            </Message>

            <Message className="max-w-full" from="assistant">
              <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                <MessageResponse>{getExecutionSummary(toolState)}</MessageResponse>

                <Tool className="mt-5 rounded-2xl border border-app-border/28 bg-app-surface/24 shadow-none" defaultOpen>
                  <ToolHeader
                    state={toolState}
                    title="install_ai_elements_components"
                    type="tool-install_ai_elements_components"
                  />
                  <ToolContent>
                    <ToolInput input={AI_ELEMENTS_TOOL_INPUT} />

                    <Confirmation approval={approval} state={toolState}>
                      <ConfirmationTitle>
                        <ConfirmationRequest>
                          This install step will add the official AI Elements primitives and keep the existing shared shadcn
                          primitives intact. Approve the integration?
                        </ConfirmationRequest>
                        <ConfirmationAccepted>
                          <span>The install step was approved and the thread can continue.</span>
                        </ConfirmationAccepted>
                        <ConfirmationRejected>
                          <span>The install step was denied and the queue remains blocked.</span>
                        </ConfirmationRejected>
                      </ConfirmationTitle>

                      <ConfirmationActions>
                        <ConfirmationAction onClick={handleRejectTool} variant="outline">
                          Reject
                        </ConfirmationAction>
                        <ConfirmationAction onClick={handleApproveTool}>Approve</ConfirmationAction>
                      </ConfirmationActions>
                    </Confirmation>

                    {toolState === "output-available" ? (
                      <ToolOutput errorText={undefined} output={AI_ELEMENTS_TOOL_SUCCESS_OUTPUT} />
                    ) : null}
                  </ToolContent>
                </Tool>

                <Sources className="mt-5 px-0 pt-1" defaultOpen={false}>
                  <SourcesTrigger count={AI_ELEMENTS_SOURCES.length} />
                  <SourcesContent>
                    {AI_ELEMENTS_SOURCES.map((source) => (
                      <Source href={source.href} key={source.href} title={source.title} />
                    ))}
                  </SourcesContent>
                </Sources>
              </MessageContent>
            </Message>

            {followUpEntries.map((entry) => (
              <Message className={entry.role === "assistant" ? "max-w-full" : undefined} from={entry.role} key={entry.id}>
                <MessageContent
                  className={
                    entry.role === "assistant"
                      ? "w-full max-w-full bg-transparent px-0 py-0 shadow-none"
                      : "rounded-2xl bg-app-surface/62 px-4 py-3 shadow-none backdrop-blur-sm"
                  }
                >
                  {entry.attachments && entry.attachments.length > 0 ? (
                    <ComposerMessageAttachments attachments={entry.attachments} />
                  ) : null}
                  {entry.role === "assistant" && entry.label ? (
                    <div className="mb-2 flex items-center gap-2 text-xs text-app-subtle">
                      <Badge className="rounded-full border-0 bg-app-surface-muted/70 px-2.5 py-0.5 text-app-subtle" variant="outline">
                        {entry.label}
                      </Badge>
                      <span>Next local execution now reflects the active profile.</span>
                    </div>
                  ) : null}
                  <MessageResponse>{entry.text}</MessageResponse>
                </MessageContent>
              </Message>
            ))}
          </ConversationContent>
          <ConversationScrollButton className="bottom-4" />
        </Conversation>
      </div>

      <div className="shrink-0 px-6 pb-6 pt-4">
        <WorkbenchPromptComposer
          activeAgentProfileId={activeAgentProfileId}
          agentProfiles={agentProfiles}
          error={composerError}
          onErrorMessageChange={setComposerError}
          onSelectAgentProfile={onSelectAgentProfile}
          onStop={handleStopFollowUp}
          onSubmit={handleSubmit}
          placeholder="继续细化这条 AI Elements 任务流..."
          providers={providers}
          status={status}
          suggestions={AI_ELEMENTS_SUGGESTIONS}
          value={composerValue}
          onValueChange={setComposerValue}
        />
      </div>
    </div>
  );
}
