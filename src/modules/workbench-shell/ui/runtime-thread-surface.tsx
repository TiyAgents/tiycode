"use client";

import type { ChatStatus } from "ai";
import { AlertCircleIcon, BotIcon, Info, RefreshCcwIcon, SparklesIcon, WrenchIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useT } from "@/i18n";
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
import { useViewportAutoCollapse, type ViewportAutoCollapseEntry } from "@/shared/hooks/use-viewport-auto-collapse";
import { buildRunModelPlanFromSelection } from "@/modules/settings-center/model/run-model-plan";
import type { AgentProfile, CommandEntry, ProviderEntry } from "@/modules/settings-center/model/types";
import { threadClearContext, threadLoad } from "@/services/bridge";
import {
  ThreadStream,
  type HelperEvent,
  type QueueEvent,
  type RunState,
  type ThreadTitleEvent,
  type UsageEvent,
} from "@/services/thread-stream";
import type {
  MessageAttachmentDto,
  RunMode,
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
import {
  getDefaultToolOpenState,
  isCompletedToolState,
  isTaskBoardTool,
  mapSnapshotToRunState,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-logic";
import { LongMessageBody } from "@/modules/workbench-shell/ui/long-message-body";
import { FileMutationDiffPreview } from "@/modules/workbench-shell/ui/runtime-thread-surface-diff";
import {
  ToolCommandOutputBlocks,
  TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS,
  getCommandOutputToolPresentation,
  getListToolPresentation,
  getQueryToolPresentation,
  getReadToolPresentation,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-tools";
import {
  applyHelperSnapshot,
  formatElapsedSeconds,
  formatExecutionSummary,
  formatHelperDetailSummary,
  formatHelperName,
  formatHelperStatusLabel,
  formatHelperSummary,
  formatHelperToolCounts,
  formatToolCallCount,
  getHelperElapsedSeconds,
  mapSnapshotHelper,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-helpers";
import { TaskBoardCard } from "@/modules/workbench-shell/ui/task-board-card";
import { TaskHistoryTimeline } from "@/modules/workbench-shell/ui/task-stage-history-card";
import {
  appendOrReplaceMessage,
  compareTimelineEntries,
  deriveSelectedRunMode,
  formatApprovalPromptState,
  formatToolStatusLabel,
  getApprovalReason,
  getApprovalTagClass,
  getApprovalTagLabel,
  getLatestVisibleRun,
  getPresentationEntryRole,
  getRoleSpacingClass,
  getSnapshotRuntimeError,
  getToolStatusClass,
  isApprovalDenied,
  isMoreAdvancedMessageStatus,
  isRenderableTimelineMessage,
  isVisibleTimelineTool,
  mapRunSummaryToContextUsage,
  mapSnapshotMessage,
  mapSnapshotTool,
  mergeSnapshotTools,
  prependOlderMessages,
  shouldCompleteThinkingPhase,
  shouldFinalizeReasoningOnly,
  stringifyToolValue,
  updateHelper,
  updateTool,
  type InitialPromptRequest,
  type SurfaceHelperEntry,
  type SurfaceMessage,
  type SurfaceRuntimeError,
  type SurfaceToolEntry,
  type SurfaceToolState,
  type ThinkingPlaceholder,
  type TimelineEntry,
  type TimelineRole,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-state";
import {
  type PlanApprovalAction,
  asObjectRecord,
  parseApprovalPromptMetadata,
  parseCommandComposerMetadata,
  parseSummaryMarkerMetadata,
  parseClarifyPrompt,
  formatPlanMetadata,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-metadata";

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
  onOpenProfileSettings?: () => void;
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


const BASE_CONVERSATION_BOTTOM_PADDING = 40;

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
  onOpenProfileSettings,
  onSelectAgentProfile,
  onThreadTitleChange,
  providers,
  threadId,
  threadTitle,
  workspaceId,
}: RuntimeThreadSurfaceProps) {
  const t = useT();
  const activeProfile = useMemo(() => {
    const matchedProfile = agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? null;
    return matchedProfile;
  }, [activeAgentProfileId, agentProfiles]);
  const hasMissingActiveProfile = Boolean(activeAgentProfileId) && activeProfile === null;
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
      wrapperRefsMap.current.clear();
      userManuallyOpenedIds.current.clear();
    }
  }, [threadId]);
  const [thinkingPlaceholder, setThinkingPlaceholder] = useState<ThinkingPlaceholder | null>(null);
  const [tools, setTools] = useState<Array<SurfaceToolEntry>>([]);
  const [completedToolOpen, setCompletedToolOpen] = useState<Record<string, boolean>>({});
  const [reasoningOpen, setReasoningOpen] = useState<Record<string, boolean>>({});
  const [taskBoards, setTaskBoards] = useState<TaskBoardState>(initialTaskBoardState);
  const previousHelperStatusesRef = useRef<Record<string, SurfaceHelperEntry["status"]>>({});
  const previousToolStatesRef = useRef<Record<string, SurfaceToolState>>({});
  const snapshotLoadRequestRef = useRef(0);
  const completedMessageResyncRequestRef = useRef(0);
  const streamRef = useRef<ThreadStream | null>(null);
  const pendingThreadRestoreScrollRef = useRef(false);
  const submittingRef = useRef(false);
  const subscribingRef = useRef(false);
  const handledInitialPromptRequestIdRef = useRef<string | null>(null);
  const thinkingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const preserveContextUsageOnNextEmptySnapshotRef = useRef(false);
  const conversationContextRef = useRef<StickToBottomContext | null>(null);
  const lastOptimisticUserIdRef = useRef<string | null>(null);

  // --- Viewport auto-collapse infrastructure ---
  const [scrollContainerEl, setScrollContainerEl] = useState<HTMLElement | null>(null);
  const contentSentinelRef = useRef<HTMLDivElement | null>(null);
  const wrapperRefsMap = useRef<Map<string, HTMLElement>>(new Map());
  const userManuallyOpenedIds = useRef<Set<string>>(new Set());

  const clearScheduledThinkingPhase = useCallback(() => {
    if (thinkingTimerRef.current !== null) {
      clearTimeout(thinkingTimerRef.current);
      thinkingTimerRef.current = null;
    }
  }, []);

  const showThinkingPlaceholder = useCallback((runId?: string | null, createdAt?: string, label?: string) => {
    setThinkingPlaceholder((current) => {
      if (current && current.runId === (runId ?? null)) {
        // Same placeholder run — just update the label in-place (e.g. when
        // "Thinking…" switches to "Compressing context…"). Keeping the same
        // id avoids React remounting the placeholder and preserves the
        // Shimmer animation state.
        if (current.label === label) {
          return current;
        }
        return { ...current, label };
      }

      return {
        createdAt: createdAt ?? new Date().toISOString(),
        id:
          typeof crypto !== "undefined" && "randomUUID" in crypto
            ? crypto.randomUUID()
            : `thinking-${Date.now()}`,
        runId: runId ?? null,
        label,
      };
    });
  }, []);

  const scheduleThinkingPhase = useCallback((runId?: string | null, delayMs = 500) => {
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
    lastOptimisticUserIdRef.current = localUserMessageId;

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
            // Message exists locally but not in snapshot.
            if (
              localMsg.id.startsWith("local-user-") && localMsg.role === "user"
              && lastOptimisticUserIdRef.current === localMsg.id
            ) {
              // Optimistic user message — check if snapshot contains its
              // persisted counterpart (same role + content, new to this snapshot).
              const persistedIdx = merged.findIndex(
                (m) =>
                  m.role === "user"
                  && m.content === localMsg.content
                  && !currentMessages.some((c) => c.id === m.id),
              );
              if (persistedIdx !== -1) {
                // Backend has persisted the message. Replace the snapshot
                // entry's id with the optimistic id so the React key stays
                // stable and no DOM remount / scroll jump occurs.
                merged[persistedIdx] = { ...merged[persistedIdx], id: localMsg.id };
                lastOptimisticUserIdRef.current = null;
              } else {
                // DB write hasn't landed yet — keep the optimistic message
                // so it doesn't vanish from the list mid-frame.
                merged.push(localMsg);
              }
            } else if (
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
    pendingThreadRestoreScrollRef.current = Boolean(threadId);
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
    lastOptimisticUserIdRef.current = null;
    clearScheduledThinkingPhase();
    setThinkingPlaceholder(null);
    setTools([]);
    void loadSnapshot();
  }, [clearScheduledThinkingPhase, loadSnapshot, onComposerDraftChange, threadId]);

  useEffect(() => {
    const isCurrentThreadSnapshotReady = snapshotReady && snapshotThreadId === threadId;
    if (!threadId || !isCurrentThreadSnapshotReady || messages.length === 0 || !pendingThreadRestoreScrollRef.current) {
      return;
    }

    pendingThreadRestoreScrollRef.current = false;
    const rafId = window.requestAnimationFrame(() => {
      void conversationContextRef.current?.scrollToBottom("instant");
    });

    return () => {
      window.cancelAnimationFrame(rafId);
    };
  }, [messages.length, snapshotReady, snapshotThreadId, threadId]);

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
      } else if (shouldFinalizeReasoningOnly(event)) {
        clearScheduledThinkingPhase();
        finalizeReasoningForRun(event.runId);
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

    stream.onContextCompressing = withActiveStream((runId) => {
      // Context compression is happening — keep the thinking placeholder on
      // screen but relabel it to "Compressing context…". The show-helper
      // updates the label in place (same placeholder id) so the shimmer
      // doesn't remount. This deliberately does NOT go through
      // `completeThinkingPhase` / `showThinkingPlaceholder` separately, which
      // would produce a one-frame empty state between the close and reopen.
      showThinkingPlaceholder(runId, undefined, t("contextCompressing"));
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
    const prompt = submission.effectivePrompt ?? "";
    const trimmedPrompt = prompt.trim();

    if (!trimmedPrompt) {
      setComposerError("Type a prompt before starting a run.");
      return;
    }

    if (!activeProfile) {
      setComposerError(
        hasMissingActiveProfile
          ? t("composer.profileDeletedHint")
          : "Select an agent profile with an enabled model before starting a run.",
      );
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
      conversationContextRef.current?.scrollToBottom("instant");
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
      conversationContextRef.current?.scrollToBottom("instant");
      try {
        preserveContextUsageOnNextEmptySnapshotRef.current = false;
        onContextUsageChange?.(null);
        // Route through the ThreadStream so the frontend receives the
        // RunStarted + ContextCompressing events that drive the thinking
        // placeholder and the "running" thread state during the LLM call.
        // The stream's onRunStateChange callback will flip the thread back
        // to idle once RunCompleted / RunFailed arrives.
        await streamRef.current?.compactContext(
          threadId,
          submission.command.argumentsText || null,
          modelPlan,
        );
        await loadSnapshot();
      } catch (error) {
        setThinkingPlaceholder(null);
        throw error;
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
    conversationContextRef.current?.scrollToBottom("instant");

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
    conversationContextRef.current?.scrollToBottom("instant");

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
    [helpers, messages, visibleTools],
  );
  const presentationEntries = timelineEntries;

  // --- Viewport auto-collapse: detect scroll container once content mounts ---
  // We derive the scroll container from StickToBottom's scrollRef rather than
  // walking the DOM with findScrollParent.  A ref callback runs *before*
  // layout effects, so StickToBottom hasn't set `overflow: auto` on the scroll
  // div yet, causing findScrollParent to miss it and return null.
  // A passive effect fires *after* layout effects, guaranteeing the ref is ready.
  const contentSentinelCallback = useCallback((node: HTMLDivElement | null) => {
    contentSentinelRef.current = node;
  }, []);

  useEffect(() => {
    if (!snapshotReady) {
      setScrollContainerEl(null);
      return;
    }
    const scrollEl = conversationContextRef.current?.scrollRef?.current ?? null;
    setScrollContainerEl(scrollEl);
  }, [snapshotReady]);

  const getIsStuckToBottom = useCallback(
    () => conversationContextRef.current?.isAtBottom ?? true,
    [],
  );

  // Build entries for the viewport auto-collapse hook.
  const viewportCollapseEntries = useMemo<ReadonlyArray<ViewportAutoCollapseEntry>>(() => {
    const result: ViewportAutoCollapseEntry[] = [];
    for (const entry of presentationEntries) {
      if (entry.kind === "tool") {
        const isCompleted = isCompletedToolState(entry.tool.state);
        const isOpen = getDefaultToolOpenState(entry.tool.name, entry.tool.state, completedToolOpen[entry.tool.id]);
        result.push({ id: entry.tool.id, completed: isCompleted, currentOpen: isOpen });
      } else if (entry.kind === "helper") {
        const isCompleted = entry.helper.status === "completed";
        const isOpen = helperOpen[entry.helper.id] ?? true;
        result.push({ id: entry.helper.id, completed: isCompleted, currentOpen: isOpen });
      } else if (entry.kind === "message" && entry.message.messageType === "reasoning") {
        const isCompleted = entry.message.status !== "streaming";
        const isOpen = reasoningOpen[entry.message.id] ?? true;
        result.push({ id: entry.message.id, completed: isCompleted, currentOpen: isOpen });
      }
    }
    return result;
  }, [presentationEntries, completedToolOpen, helperOpen, reasoningOpen]);

  // Keep a ref to presentationEntries for the viewport collapse callback.
  const presentationEntriesRef = useRef(presentationEntries);
  presentationEntriesRef.current = presentationEntries;

  const handleViewportCollapse = useCallback((id: string) => {
    // Determine whether this id belongs to a tool, helper, or reasoning
    // and only update the relevant state map.
    const entry = presentationEntriesRef.current.find(
      (e) =>
        (e.kind === "tool" && e.tool.id === id)
        || (e.kind === "helper" && e.helper.id === id)
        || (e.kind === "message" && e.message.messageType === "reasoning" && e.message.id === id),
    );
    if (!entry) return;
    if (entry.kind === "tool") {
      setCompletedToolOpen((current) => (current[id] === false ? current : { ...current, [id]: false }));
    } else if (entry.kind === "helper") {
      setHelperOpen((current) => (current[id] === false ? current : { ...current, [id]: false }));
    } else {
      setReasoningOpen((current) => (current[id] === false ? current : { ...current, [id]: false }));
    }
  }, []);

  useViewportAutoCollapse({
    scrollContainer: scrollContainerEl,
    getIsStuckToBottom,
    entries: viewportCollapseEntries,
    wrapperRefs: wrapperRefsMap.current,
    userManuallyOpenedIds: userManuallyOpenedIds.current,
    onCollapse: handleViewportCollapse,
  });
  const lastPresentationRole = presentationEntries.length > 0
    ? getPresentationEntryRole(presentationEntries[presentationEntries.length - 1])
    : null;

  // Show the thinking indicator at the bottom when the run is active and no
  // tool / helper / streaming-message is already occupying the "latest action"
  // slot. Because this is derived from render-time state rather than toggled
  // by individual stream events, it survives React 18 batching that would
  // otherwise swallow a create+clear in the same frame.
  const hasActiveToolOrHelper =
    visibleTools.some((tool) => !isCompletedToolState(tool.state))
    || helpers.some((helper) => helper.status === "running");
  const showThinkingIndicator =
    Boolean(thinkingPlaceholder)
    && runState === "running"
    && !hasActiveToolOrHelper;

  const thinkingIndicatorPreviousRole: TimelineRole | null =
    showThinkingIndicator ? lastPresentationRole : null;
  const queuePreviousRole: TimelineRole | null =
    showThinkingIndicator ? "assistant" : lastPresentationRole;
  const hasTaskHistoryTimeline = taskBoards.boards.some((board) => board.status !== "active");
  const historyPreviousRole: TimelineRole | null = queueArtifact ? "assistant" : lastPresentationRole;
  const runtimeErrorPreviousRole: TimelineRole | null = hasTaskHistoryTimeline
    ? "assistant"
    : queueArtifact || showThinkingIndicator
      ? "assistant"
      : lastPresentationRole;

  const conversationBottomPadding = BASE_CONVERSATION_BOTTOM_PADDING;

  useEffect(() => {
    const previousToolStates = previousToolStatesRef.current;
    const nextToolStates = Object.fromEntries(visibleTools.map((tool) => [tool.id, tool.state]));

    setCompletedToolOpen((current) => {
      const next: Record<string, boolean> = {};

      for (const tool of visibleTools) {
        const previousState = previousToolStates[tool.id];

        if (previousState !== tool.state) {
          // State changed — keep the block open (don't auto-collapse on
          // completion).  Only force open when transitioning *to* a
          // non-completed state so newly-started tools expand.
          // Task board tools always default to collapsed regardless of state.
          if (isTaskBoardTool(tool.name)) {
            next[tool.id] = tool.id in current ? current[tool.id] : false;
          } else if (!isCompletedToolState(tool.state)) {
            next[tool.id] = true;
          } else {
            // Completed: preserve current open state (default open).
            next[tool.id] = tool.id in current ? current[tool.id] : true;
          }
          continue;
        }

        if (tool.id in current) {
          next[tool.id] = current[tool.id];
          continue;
        }

        next[tool.id] = isTaskBoardTool(tool.name) ? false : !isCompletedToolState(tool.state);
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
        const isCompleted = helper.status === "completed";

        if (previousStatus !== helper.status) {
          // Status changed — only force open when transitioning *to* a
          // non-completed state.  On completion, keep current open state
          // (default open) so the block doesn't auto-collapse.
          if (!isCompleted) {
            next[helper.id] = true;
          } else {
            next[helper.id] = helper.id in current ? current[helper.id] : true;
          }
          continue;
        }

        if (helper.id in current) {
          next[helper.id] = current[helper.id];
          continue;
        }

        next[helper.id] = !isCompleted;
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
    const prompt = submission.effectivePrompt ?? "";
    const trimmedPrompt = prompt.trim();
    if (!trimmedPrompt) {
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
    if (open) {
      userManuallyOpenedIds.current.add(toolId);
    }
  }, []);

  const handleHelperOpenChange = useCallback((helperId: string, open: boolean) => {
    setHelperOpen((current) => (current[helperId] === open ? current : { ...current, [helperId]: open }));
    if (open) {
      userManuallyOpenedIds.current.add(helperId);
    }
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
                "flex w-full text-left",
                readTool.error
                  ? "flex-col gap-1"
                  : "items-start justify-between gap-3",
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
                <span className="line-clamp-1 break-words text-xs text-app-danger" title={readTool.error}>
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
                "flex w-full text-left",
                queryTool.error
                  ? "flex-col gap-1"
                  : "items-start justify-between gap-3",
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
                <span className="line-clamp-1 break-words text-xs text-app-danger" title={queryTool.error}>
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
                "flex w-full text-left",
                listTool.error
                  ? "flex-col gap-1"
                  : "items-start justify-between gap-3",
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
                <span className="line-clamp-1 break-words text-xs text-app-danger" title={listTool.error}>
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
            open={getDefaultToolOpenState(tool.name, tool.state, completedToolOpen[tool.id])}
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
                    output={stringifyToolValue(tool.result)}
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
        <Conversation
          className="size-full"
          contextRef={conversationContextRef}
          initialBehavior="instant"
          resizeBehavior="instant"
        >
          <ConversationContent
            className="mx-auto w-full max-w-4xl gap-0 px-6 pt-8"
            style={{ paddingBottom: `${conversationBottomPadding}px` }}
          >
            {/* Invisible sentinel used to locate the scroll container for viewport auto-collapse */}
            <div ref={contentSentinelCallback} className="hidden" />
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
                  const reasoningIsStreaming = message.status === "streaming";
                  const reasoningIsOpen = reasoningOpen[message.id] ?? (reasoningIsStreaming || runState === "running");
                  return (
                    <div
                      className={spacingClass}
                      data-timeline-entry-id={message.id}
                      key={entry.key}
                      ref={(node) => {
                        if (node) {
                          wrapperRefsMap.current.set(message.id, node);
                        } else {
                          wrapperRefsMap.current.delete(message.id);
                        }
                      }}
                    >
                      <Message className="max-w-full" from="assistant">
                        <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                          <Reasoning
                            className="mb-0 w-full bg-transparent px-0 py-0"
                            autoClose={false}
                            open={reasoningIsOpen}
                            onOpenChange={(open) => {
                              setReasoningOpen((current) =>
                                current[message.id] === open ? current : { ...current, [message.id]: open },
                              );
                              if (open) {
                                userManuallyOpenedIds.current.add(message.id);
                              }
                            }}
                            isStreaming={reasoningIsStreaming}
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
                                {<LongMessageBody message={message} t={t} />}
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
                  <div
                    className={spacingClass}
                    data-timeline-entry-id={helper.id}
                    key={entry.key}
                    ref={(node) => {
                      if (node) {
                        wrapperRefsMap.current.set(helper.id, node);
                      } else {
                        wrapperRefsMap.current.delete(helper.id);
                      }
                    }}
                  >
                    <Message className="max-w-full" from="assistant">
                      <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                        <CompactCollapsible
                          onOpenChange={(open) => handleHelperOpenChange(helper.id, open)}
                          open={helperOpen[helper.id] ?? true}
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
                <div
                  className={spacingClass}
                  data-timeline-entry-id={tool.id}
                  key={entry.key}
                  ref={(node) => {
                    if (node) {
                      wrapperRefsMap.current.set(tool.id, node);
                    } else {
                      wrapperRefsMap.current.delete(tool.id);
                    }
                  }}
                >
                  {renderToolEntry(tool, entry.key)}
                </div>
              );
            })}

            {/* Thinking indicator — rendered outside the timeline so it is
                immune to React 18 batched-state flicker.  The outer wrapper
                always stays in the DOM; visibility is driven by grid-rows and
                opacity so the element can transition smoothly in/out without
                causing a layout jump. */}
            <div
              className={`grid transition-[grid-template-rows,opacity] duration-200 ease-in-out ${
                showThinkingIndicator
                  ? "grid-rows-[1fr] opacity-100"
                  : "grid-rows-[0fr] opacity-0"
              }`}
            >
              <div className="overflow-hidden">
                <div className={getRoleSpacingClass(thinkingIndicatorPreviousRole, "assistant")}>
                  <Message className="max-w-full" from="assistant">
                    <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                      <Reasoning
                        className="mb-0 w-full bg-transparent px-0 py-0"
                        defaultOpen={false}
                        isStreaming
                      >
                        <ReasoningTrigger
                          showChevron={false}
                          getThinkingMessage={
                            thinkingPlaceholder?.label
                              ? (isStreaming: boolean) =>
                                  isStreaming
                                    ? <Shimmer>{thinkingPlaceholder.label!}</Shimmer>
                                    : <p>{thinkingPlaceholder.label}</p>
                              : undefined
                          }
                        />
                      </Reasoning>
                    </MessageContent>
                  </Message>
                </div>
              </div>
            </div>

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

            {hasTaskHistoryTimeline ? (
              <div className={getRoleSpacingClass(historyPreviousRole, "assistant")}>
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
        <div className="mx-auto flex w-full max-w-4xl flex-col gap-0">
          {taskBoards.activeBoard ? (
            <div
              className="min-h-0 max-h-[min(36vh,320px)] overflow-hidden rounded-t-[24px] rounded-b-none border border-b-0 border-app-border/80 bg-app-menu/96 px-2 pb-0 pt-2 shadow-[0_26px_70px_-42px_rgba(15,23,42,0.45)] backdrop-blur-xl"
            >
              <TaskBoardCard
                board={taskBoards.activeBoard}
                variant="composer"
                className="rounded-[18px] rounded-b-none border-x-0 border-t-0 border-b border-app-border/55 bg-app-surface/52 px-4 pb-3 pt-3 shadow-none"
              />
            </div>
          ) : null}

          <WorkbenchPromptComposer
            activeAgentProfileId={activeAgentProfileId}
            agentProfiles={agentProfiles}
            allowMissingActiveProfile
            canSubmitWhenAttachmentsOnly={false}
            className="w-full max-w-none gap-0"
            commands={commands}
            composerShellClassName={taskBoards.activeBoard
              ? "rounded-t-none border-t-0"
              : undefined}
            enabledSkills={enabledSkills}
            error={composerError}
            onErrorMessageChange={setComposerError}
            onRunModeChange={setSelectedRunMode}
            onOpenProfileSettings={onOpenProfileSettings}
            onSelectAgentProfile={onSelectAgentProfile}
            onStop={() => {
              if (!threadId) {
                return;
              }

              void streamRef.current?.cancelRun(threadId).then((didCancel) => {
                if (!didCancel) {
                  // The backend no longer has an active run for this thread.
                  // Reload the snapshot to reconcile the stale UI state with
                  // the actual persisted terminal state without surfacing a
                  // technical error to the user.
                  void loadSnapshot();
                  return;
                }

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
                // The cancel request failed due to a real backend/runtime error.
                // Reload the snapshot to reconcile the UI after surfacing that
                // failure through the normal stream error path.
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
    </div>
  );
}
