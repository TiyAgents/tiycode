"use client";

import type { ChatStatus, FileUIPart } from "ai";
import { Bot, CheckIcon, FileIcon, ImageIcon, PaperclipIcon, SparklesIcon, XIcon } from "lucide-react";
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
  ModelSelector,
  ModelSelectorContent,
  ModelSelectorEmpty,
  ModelSelectorGroup,
  ModelSelectorInput,
  ModelSelectorItem,
  ModelSelectorList,
  ModelSelectorTrigger,
} from "@/components/ai-elements/model-selector";
import {
  Plan,
  PlanContent,
  PlanDescription,
  PlanHeader,
  PlanTitle,
  PlanTrigger,
} from "@/components/ai-elements/plan";
import {
  PromptInput,
  PromptInputBody,
  PromptInputButton,
  PromptInputFooter,
  PromptInputHeader,
  type PromptInputMessage,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
  usePromptInputAttachments,
} from "@/components/ai-elements/prompt-input";
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
import { Suggestion, Suggestions } from "@/components/ai-elements/suggestion";
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
  getProfilePrimaryModelId,
  getProfilePrimaryModelLabel,
  type DemoQueueItem,
} from "@/modules/workbench-shell/model/ai-elements-task-demo";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { ModelBrandIcon } from "@/shared/ui/model-brand-icon";

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

type DemoAttachment = {
  id: string;
  mediaType?: string;
  name: string;
};

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

function mapDemoAttachments(files: Array<FileUIPart>) {
  return files.map((file, index) => ({
    id: nanoid(),
    mediaType: file.mediaType,
    name: file.filename?.trim() || `附件 ${index + 1}`,
  }));
}

function getAttachmentGlyph(mediaType?: string) {
  if (mediaType?.startsWith("image/")) {
    return <ImageIcon className="size-3.5" />;
  }

  return <FileIcon className="size-3.5" />;
}

function AttachmentChip({
  attachment,
  onRemove,
  tone = "message",
}: {
  attachment: DemoAttachment;
  onRemove?: (id: string) => void;
  tone?: "composer" | "message";
}) {
  return (
    <div
      className={cn(
        "inline-flex max-w-full items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium",
        tone === "composer"
          ? "border-app-border/55 bg-app-surface-muted/80 text-app-foreground"
          : "border-app-border/45 bg-app-surface/60 text-app-muted",
      )}
    >
      <span className={cn("shrink-0", tone === "composer" ? "text-app-foreground" : "text-app-subtle")}>
        {getAttachmentGlyph(attachment.mediaType)}
      </span>
      <span className="truncate">{attachment.name}</span>
      {onRemove ? (
        <button
          aria-label={`移除附件 ${attachment.name}`}
          className="inline-flex size-4 shrink-0 items-center justify-center rounded-full text-app-subtle transition hover:bg-app-surface-hover hover:text-app-foreground"
          onClick={(event) => {
            event.preventDefault();
            onRemove(attachment.id);
          }}
          type="button"
        >
          <XIcon className="size-3" />
        </button>
      ) : null}
    </div>
  );
}

function ComposerAttachmentHeader() {
  const attachments = usePromptInputAttachments();

  if (attachments.files.length === 0) {
    return null;
  }

  return (
    <PromptInputHeader className="border-b border-app-border/45 bg-app-surface-muted/35 px-3 py-2">
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
        <Badge className="rounded-full px-2 py-0.5" variant="secondary">
          <PaperclipIcon className="size-3" />
          {attachments.files.length} 个附件
        </Badge>
        {attachments.files.map((attachment) => (
          <AttachmentChip
            attachment={{
              id: attachment.id,
              mediaType: attachment.mediaType,
              name: attachment.filename?.trim() || "未命名附件",
            }}
            key={attachment.id}
            onRemove={attachments.remove}
            tone="composer"
          />
        ))}
      </div>
    </PromptInputHeader>
  );
}

function ComposerAttachmentStateSync({
  onHasAttachmentsChange,
}: {
  onHasAttachmentsChange: (hasAttachments: boolean) => void;
}) {
  const attachments = usePromptInputAttachments();

  useEffect(() => {
    onHasAttachmentsChange(attachments.files.length > 0);
  }, [attachments.files.length, onHasAttachmentsChange]);

  return null;
}

function ComposerAttachmentTrigger() {
  const attachments = usePromptInputAttachments();

  return (
    <PromptInputButton
      aria-label="上传文件或图片"
      className="px-2.5"
      onClick={(event) => {
        event.preventDefault();
        attachments.openFileDialog();
      }}
      type="button"
    >
      <PaperclipIcon className="size-4" />
    </PromptInputButton>
  );
}

function PromptInputSubmitButton({
  activeProfile,
  composerValue,
  onStop,
  status,
}: {
  activeProfile: AgentProfile | null;
  composerValue: string;
  onStop: () => void;
  status: ChatStatus;
}) {
  const attachments = usePromptInputAttachments();
  const canSubmit = Boolean(activeProfile) && (Boolean(composerValue.trim()) || attachments.files.length > 0);

  return <PromptInputSubmit disabled={!canSubmit} onStop={onStop} status={status} />;
}

function ProfileInlineIdentity({
  badge = true,
  profile,
  providers,
  muted = false,
  showModel = true,
}: {
  badge?: boolean;
  muted?: boolean;
  profile: AgentProfile;
  providers: ReadonlyArray<ProviderEntry>;
  showModel?: boolean;
}) {
  const modelId = getProfilePrimaryModelId(profile, providers) || profile.name;
  const modelLabel = getProfilePrimaryModelLabel(profile, providers);

  return (
    <div className="flex min-w-0 items-center gap-2">
      <span
        className={cn(
          "flex shrink-0 items-center justify-center",
          badge ? "size-6 rounded-lg bg-app-surface-muted/75 ring-1 ring-app-border/45" : "size-4 rounded-none bg-transparent ring-0",
          muted ? "text-app-muted" : "text-app-foreground",
        )}
      >
        <Bot className="size-3.5" />
      </span>
      <span className={cn("shrink-0 text-sm font-medium", muted ? "text-app-foreground/88" : "text-app-foreground")}>
        {profile.name}
      </span>
      {showModel ? (
        <>
          <span aria-hidden="true" className="shrink-0 text-app-subtle">
            {" · "}
          </span>
          <ModelBrandIcon
            className={cn("size-4 shrink-0", muted ? "text-app-muted" : "text-app-foreground")}
            displayName={modelLabel}
            modelId={modelId}
          />
          <span className={cn("min-w-0 truncate text-xs", muted ? "text-muted-foreground" : "text-app-muted")}>
            {modelLabel}
          </span>
        </>
      ) : null}
    </div>
  );
}

function ProfileSelectorItem({
  isActive,
  onSelect,
  profile,
  providers,
}: {
  isActive: boolean;
  onSelect: () => void;
  profile: AgentProfile;
  providers: ReadonlyArray<ProviderEntry>;
}) {
  return (
    <ModelSelectorItem onSelect={onSelect} value={profile.id}>
      <div className="min-w-0 flex flex-1 items-center">
        <ProfileInlineIdentity profile={profile} providers={providers} />
      </div>
      {isActive ? <CheckIcon className="ml-auto size-4" /> : <span className="ml-auto size-4" />}
    </ModelSelectorItem>
  );
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
  const [isProfileSelectorOpen, setProfileSelectorOpen] = useState(false);
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
  const canSwitchProfiles = agentProfiles.length > 1 && Boolean(activeProfile);

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
    (message: PromptInputMessage) => {
      const nextText = message.text?.trim();
      const nextAttachments = mapDemoAttachments(message.files);
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

                <Plan className="mt-5 overflow-hidden rounded-2xl border border-app-border/28 bg-app-surface/28 shadow-none" defaultOpen>
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
                    <div className="mb-3 flex flex-wrap gap-2">
                      {entry.attachments.map((attachment) => (
                        <AttachmentChip attachment={attachment} key={attachment.id} />
                      ))}
                    </div>
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

      <div className="shrink-0 bg-[linear-gradient(180deg,rgba(255,255,255,0.02),rgba(255,255,255,0))] px-6 pb-6 pt-4 backdrop-blur-sm">
        <div className="mx-auto flex max-w-4xl flex-col gap-3">
          <Suggestions className="gap-2">
            {AI_ELEMENTS_SUGGESTIONS.map((suggestion) => (
              <Suggestion
                key={suggestion}
                onClick={(nextSuggestion) => setComposerValue(nextSuggestion)}
                suggestion={suggestion}
                variant="secondary"
              />
            ))}
          </Suggestions>

          <div className="rounded-[26px] border border-app-border/60 bg-app-surface/82 p-1.5 shadow-[0_22px_50px_-42px_rgba(15,23,42,0.38)] backdrop-blur-sm">
            <PromptInput
              accept="image/*,.pdf,.md,.txt,.json,.ts,.tsx"
              className="[&_[data-slot=input-group]]:shadow-none [&_[data-slot=input-group]:focus-within]:!border-app-border/60 [&_[data-slot=input-group]:focus-within]:!ring-0"
              maxFileSize={10 * 1024 * 1024}
              maxFiles={4}
              onError={(error) => setComposerError(error.message)}
              onSubmit={handleSubmit}
            >
              <PromptInputBody>
                <ComposerAttachmentStateSync
                  onHasAttachmentsChange={(hasAttachments) => {
                    if (hasAttachments) {
                      setComposerError(null);
                    }
                  }}
                />
                <ComposerAttachmentHeader />
                <PromptInputTextarea
                  className="min-h-[88px]"
                  onChange={(event) => setComposerValue(event.currentTarget.value)}
                  placeholder="继续细化这条 AI Elements 任务流..."
                  value={composerValue}
                />
              </PromptInputBody>
              <PromptInputFooter>
                <PromptInputTools>
                  <ComposerAttachmentTrigger />

                  {activeProfile ? (
                    canSwitchProfiles ? (
                      <ModelSelector onOpenChange={setProfileSelectorOpen} open={isProfileSelectorOpen}>
                        <ModelSelectorTrigger asChild>
                          <PromptInputButton className="h-auto max-w-[360px] justify-start gap-3 px-3 py-2" size="sm">
                            <ProfileInlineIdentity badge={false} profile={activeProfile} providers={providers} showModel={false} />
                          </PromptInputButton>
                        </ModelSelectorTrigger>
                        <ModelSelectorContent title="Profile Selector">
                          <ModelSelectorInput placeholder="Search profiles..." />
                          <ModelSelectorList>
                            <ModelSelectorEmpty>未找到可用的 profile。</ModelSelectorEmpty>
                            <ModelSelectorGroup heading="Agent Profiles">
                              {agentProfiles.map((profile) => (
                                <ProfileSelectorItem
                                  isActive={profile.id === activeAgentProfileId}
                                  key={profile.id}
                                  onSelect={() => {
                                    onSelectAgentProfile(profile.id);
                                    setProfileSelectorOpen(false);
                                  }}
                                  profile={profile}
                                  providers={providers}
                                />
                              ))}
                            </ModelSelectorGroup>
                          </ModelSelectorList>
                        </ModelSelectorContent>
                      </ModelSelector>
                    ) : (
                      <PromptInputButton className="h-auto max-w-[360px] justify-start gap-3 px-3 py-2" disabled size="sm">
                        <ProfileInlineIdentity badge={false} muted profile={activeProfile} providers={providers} showModel={false} />
                      </PromptInputButton>
                    )
                  ) : null}
                </PromptInputTools>

                <PromptInputSubmitButton
                  activeProfile={activeProfile}
                  composerValue={composerValue}
                  onStop={handleStopFollowUp}
                  status={status}
                />
              </PromptInputFooter>
            </PromptInput>
          </div>

          {composerError ? <p className="text-xs text-app-danger">{composerError}</p> : null}
          {!activeProfile ? (
            <p className="text-xs text-app-danger">No active profile is available for the composer right now.</p>
          ) : null}
        </div>
      </div>
    </div>
  );
}
