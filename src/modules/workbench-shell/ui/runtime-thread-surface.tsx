"use client";

import type { ChatStatus } from "ai";
import { AlertCircleIcon, BotIcon, CheckIcon, ChevronDownIcon, CopyIcon, Info, RefreshCcwIcon, SparklesIcon, WrenchIcon } from "lucide-react";
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
import { Message, MessageAction, MessageActions, MessageContent, MessageResponse } from "@/components/ai-elements/message";
import { Plan, PlanContent, PlanDescription, PlanHeader, PlanTitle, PlanTrigger } from "@/components/ai-elements/plan";
import { Reasoning, ReasoningContent, ReasoningTrigger } from "@/components/ai-elements/reasoning";
import { Shimmer } from "@/components/ai-elements/shimmer";
import { ToolInput, ToolOutput } from "@/components/ai-elements/tool";
import { Confirmation, ConfirmationAccepted, ConfirmationAction, ConfirmationActions, ConfirmationRejected, ConfirmationRequest, ConfirmationTitle } from "@/components/ai-elements/confirmation";
import { useDelayedAutoCollapse, type DelayedAutoCollapseEntry } from "@/shared/hooks/use-delayed-auto-collapse";
import { buildRunModelPlanFromSelection } from "@/modules/settings-center/model/run-model-plan";
import type { CommandEntry } from "@/modules/settings-center/model/types";
import { threadClearContext, threadLoad } from "@/services/bridge";
import {
  goalSet,
  goalGetState,
  type GoalPayload,
} from "@/services/bridge/agent-commands";
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
  RuntimeQueueMessageKind,
  RuntimeQueueMessageDto,
  RuntimeQueueSnapshotDto,
  TaskBoardDto,
} from "@/shared/types/api";
import { cn } from "@/shared/lib/utils";
import {
  threadStore,
  useStore,
  shallowEqual,
  isPendingRunHandled,
  markPendingRunHandled,
} from "@/modules/workbench-shell/model/thread-store";
import {
  createRunLifecycleMachine,
  mapStreamEventToMachineEvent,
  type RunMachineContext,
  type RunMachineEvent,
  type RunMachinePayload,
  type RunMachineState,
} from "@/modules/workbench-shell/model/run-lifecycle-machine";
import {
  registerRunMachine,
  unregisterRunMachine,
} from "@/modules/workbench-shell/model/run-event-dispatcher";
import { composerStore, setDraft, getDraft, type SerializableAttachment } from "@/modules/workbench-shell/model/composer-store";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
import { updateAgentProfile } from "@/modules/settings-center/model/settings-ipc-actions";
import { threadUpdateProfile } from "@/services/bridge";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { Button } from "@/shared/ui/button";
import type { ComposerSubmission, ComposerReferencedFile } from "@/modules/workbench-shell/model/composer-commands";
import type { SkillRecord } from "@/shared/types/extensions";
import {
  getFileMutationPresentation,
} from "@/modules/workbench-shell/model/file-mutation-presentation";
import { GoalStatusBar } from "@/modules/workbench-shell/ui/goal-status-bar";
import { WorkbenchPromptComposer, ComposerMessageAttachments } from "@/modules/workbench-shell/ui/workbench-prompt-composer";
import {
  initialTaskBoardState,
  taskBoardsFromSnapshot,
  applyTaskBoardUpdate,
  type TaskBoardState,
} from "@/modules/workbench-shell/model/task-board";
import {
  getDefaultToolOpenState,
  isDefaultCollapsedTool,
  isCompletedToolState,
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
import { RuntimeQueueTimeline } from "@/modules/workbench-shell/ui/runtime-queue-timeline";
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
  getAssistantRunCopyState,
  getCopyableThreadMessageText,
  getLatestVisibleRun,
  getPresentationEntryRole,
  getRoleSpacingClass,
  getSnapshotRuntimeError,
  getToolStatusClass,
  isApprovalDenied,
  isRenderableTimelineMessage,
  isVisibleTimelineTool,
  mapRunSummaryToContextUsage,
  mapSnapshotMessage,
  mapRecordedUserMessage,
  mapSnapshotTool,
  mergeArtifactEventIntoMessages,
  mergeSnapshotMessages,
  mergeSnapshotTools,
  prependOlderMessages,
  removeRequestRetryEntriesForRun,
  shouldCompleteThinkingPhase,
  shouldFinalizeReasoningOnly,
  stringifyToolValue,
  updateHelper,
  updateTool,
  type SurfaceHelperEntry,
  type SurfaceMessage,
  type SurfaceRequestRetryEntry,
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
  parseRuntimeQueueComposerMetadata,
  parseSummaryMarkerMetadata,
  parseGoalContinuationMetadata,
  parseClarifyPrompt,
  formatPlanMetadata,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-metadata";

type RuntimeThreadSurfaceProps = {
  commands?: ReadonlyArray<CommandEntry>;
  enabledSkills?: ReadonlyArray<Pick<SkillRecord, "id" | "name" | "description" | "scope" | "source" | "tags" | "triggers" | "contentPreview">>;
  threadId: string | null;
};

// re-exported from thread-store for backward compat — prefer importing from thread-store directly
export type { ThreadContextUsage } from "@/modules/workbench-shell/model/thread-store";
import { findWorkspaceForThread, resolveActiveThreadWorkbenchProfileId } from "@/modules/workbench-shell/ui/dashboard-workbench-logic";
import { uiLayoutStore } from "@/modules/workbench-shell/model/ui-layout-store";

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
type RuntimeQueueSubmitMode = RuntimeQueueMessageKind;
const THREAD_AUTO_COLLAPSE_DELAY_MS = 8000;

/** Idle context used when resetting the run-lifecycle machine. */
const RESET_IDLE_CONTEXT: RunMachineContext = {
  runId: null,
  errorMessage: null,
  retryCount: 0,
};

function setThreadGoalState(threadId: string, goal: GoalPayload | null): void {
  threadStore.setState((prev) => ({
    goalState: { ...prev.goalState, [threadId]: goal },
  }));
}

export function RuntimeThreadSurface({
  commands = [],
  enabledSkills = [],
  threadId,
}: RuntimeThreadSurfaceProps) {
  const t = useT();
  const globalAgentProfileId = useStore(settingsStore, (s) => s.activeAgentProfileId);
  const agentProfiles = useStore(settingsStore, (s) => s.agentProfiles);
  const customSubagents = useStore(settingsStore, (s) => s.customSubagents, shallowEqual);
  const customAgentSlugToName = useMemo(
    () => new Map(customSubagents.map((a) => [a.slug, a.name])),
    [customSubagents],
  );
  const providers = useStore(settingsStore, (s) => s.providers);
  const defaultAppendMessageKind = useStore(settingsStore, (s) => s.general.defaultAppendMessageKind);
  const isNewThreadMode = useStore(threadStore, (s) => s.isNewThreadMode);
  const activeThreadProfileIdOverride = useStore(threadStore, (s) => s.activeThreadProfileIdOverride);
  const pendingRuns = useStore(threadStore, (s) => s.pendingRuns);
  const activeAgentProfileId = useMemo(
    () => resolveActiveThreadWorkbenchProfileId(
      isNewThreadMode ? null : activeThreadProfileIdOverride,
      globalAgentProfileId,
    ),
    [isNewThreadMode, activeThreadProfileIdOverride, globalAgentProfileId],
  );
  const activeProfile = useMemo(() => {
    const matchedProfile = agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? null;
    return matchedProfile;
  }, [activeAgentProfileId, agentProfiles]);
  const hasMissingActiveProfile = Boolean(activeAgentProfileId) && activeProfile === null;
  const [composerError, setComposerError] = useState<string | null>(null);
  useEffect(() => {
    if (!composerError) return;
    const timer = setTimeout(() => setComposerError(null), 5000);
    return () => clearTimeout(timer);
  }, [composerError]);
  const [composerClearSignal, setComposerClearSignal] = useState(0);
  const composerValue = useStore(composerStore, () => (threadId ? getDraft(threadId).text : ""));
  const setComposerValue = useCallback(
    (value: string) => {
      if (threadId) {
        const existing = getDraft(threadId);
        setDraft(threadId, { ...existing, text: value });
      }
    },
    [threadId],
  );
  const [approvingPlanMessageId, setApprovingPlanMessageId] = useState<string | null>(null);
  const [copiedCopyTargetId, setCopiedCopyTargetId] = useState<string | null>(null);
  const messageCopyResetTimeoutRef = useRef<number>(0);
  const [helpers, setHelpers] = useState<Array<SurfaceHelperEntry>>([]);
  const [helperOpen, setHelperOpen] = useState<Record<string, boolean>>({});
  const [hasMoreMessages, setHasMoreMessages] = useState(false);
  const [historyLoadError, setHistoryLoadError] = useState<string | null>(null);
  const [isLoading, setLoading] = useState(false);
  const [isLoadingMoreMessages, setIsLoadingMoreMessages] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Array<SurfaceMessage>>([]);
  const [runtimeQueue, setRuntimeQueue] = useState<RuntimeQueueSnapshotDto | null>(null);
  const [cancellingRuntimeQueueMessageIds, setCancellingRuntimeQueueMessageIds] = useState<Set<string>>(() => new Set());
  const [promotingRuntimeQueueMessageIds, setPromotingRuntimeQueueMessageIds] = useState<Set<string>>(() => new Set());
  const [editingRuntimeQueueMessageIds, setEditingRuntimeQueueMessageIds] = useState<Set<string>>(() => new Set());
  const [composerRestoreSignal, setComposerRestoreSignal] = useState(0);
  const [runtimeQueueSubmitMode, setRuntimeQueueSubmitMode] = useState<RuntimeQueueSubmitMode>(defaultAppendMessageKind);
  const previousDefaultAppendMessageKindRef = useRef(defaultAppendMessageKind);
  const [requestRetryEntries, setRequestRetryEntries] = useState<Array<SurfaceRequestRetryEntry>>([]);
  const [requestRetryOpen, setRequestRetryOpen] = useState<Record<string, boolean>>({});
  const clearRequestRetryForRun = useCallback((runId: string) => {
    setRequestRetryEntries((current) => removeRequestRetryEntriesForRun(current, runId));
    setRequestRetryOpen((current) => {
      const retryId = `request-retry-${runId}`;
      if (!(retryId in current)) {
        return current;
      }

      const next = { ...current };
      delete next[retryId];
      return next;
    });
  }, []);
  const [runtimeError, setRuntimeError] = useState<SurfaceRuntimeError | null>(null);
  const runState = useStore(
    threadStore,
    (s) => (threadId ? s.threadStatuses[threadId]?.status ?? "idle" : "idle") as RunState,
  );
  const activeRunId = useStore(
    threadStore,
    (s) => (threadId ? s.threadStatuses[threadId]?.runId ?? null : null),
  );
  const [selectedRunMode, setSelectedRunMode] = useState<RunMode>("default");
  const [snapshotReady, setSnapshotReady] = useState(false);
  const [snapshotThreadId, setSnapshotThreadId] = useState<string | null>(null);

  useEffect(() => {
    const previousDefault = previousDefaultAppendMessageKindRef.current;
    if (previousDefault === defaultAppendMessageKind) return;

    previousDefaultAppendMessageKindRef.current = defaultAppendMessageKind;
    setRuntimeQueueSubmitMode((current) => (
      current === previousDefault ? defaultAppendMessageKind : current
    ));
  }, [defaultAppendMessageKind]);

  // Reset run mode (plan toggle) when switching to a different thread so it
  // doesn't leak from one thread to another.
  const prevThreadIdRef = useRef(threadId);
  useEffect(() => {
    if (prevThreadIdRef.current !== threadId) {
      prevThreadIdRef.current = threadId;
      setSelectedRunMode("default");
      setRuntimeQueueSubmitMode(defaultAppendMessageKind);
      setRequestRetryEntries([]);
      setRequestRetryOpen({});
      setCancellingRuntimeQueueMessageIds(new Set());
      setPromotingRuntimeQueueMessageIds(new Set());
      setEditingRuntimeQueueMessageIds(new Set());
      setCompletedToolOpen({});
      setHelperOpen({});
      setReasoningOpen({});
      setCopiedCopyTargetId(null);
      if (typeof window !== "undefined") {
        window.clearTimeout(messageCopyResetTimeoutRef.current);
      }
      userManuallyOpenedIds.current.clear();
    }
  }, [defaultAppendMessageKind, threadId]);
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

  // Per-thread run-lifecycle state machine — the authoritative source for
  // this thread's run status. Every state change is auto-synced to threadStore.
  const runMachine = useMemo(
    () => createRunLifecycleMachine(threadId ?? ""),
    [threadId],
  );
  const snapshotLoadingRef = useRef(false);
  const eventBufferRef = useRef<Array<{ event: RunMachineEvent; payload?: RunMachinePayload }>>([]);
  const thinkingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const preserveContextUsageOnNextEmptySnapshotRef = useRef(false);
  const conversationContextRef = useRef<StickToBottomContext | null>(null);
  const lastOptimisticUserIdRef = useRef<string | null>(null);

  // Track the currently active thread ID so stale closures (e.g. onStop
  // timeout/callback) can detect that the thread has changed and bail out.
  const activeThreadIdRef = useRef(threadId);
  activeThreadIdRef.current = threadId;

  // Store the stop safety-net timeout so it can be cleared on thread switch.
  const stopTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // --- Delayed auto-collapse infrastructure ---
  const userManuallyOpenedIds = useRef<Set<string>>(new Set());

  const clearScheduledThinkingPhase = useCallback(() => {
    if (thinkingTimerRef.current !== null) {
      clearTimeout(thinkingTimerRef.current);
      thinkingTimerRef.current = null;
    }
  }, []);

  const handleCopyMessage = useCallback(async (copyTargetId: string, text: string) => {
    if (typeof window === "undefined" || !navigator?.clipboard?.writeText) {
      return;
    }

    const normalizedText = text.trim();
    if (!normalizedText) {
      return;
    }

    try {
      await navigator.clipboard.writeText(normalizedText);
      window.clearTimeout(messageCopyResetTimeoutRef.current);
      setCopiedCopyTargetId(copyTargetId);
      messageCopyResetTimeoutRef.current = window.setTimeout(() => {
        setCopiedCopyTargetId((current) => (current === copyTargetId ? null : current));
      }, 2000);
    } catch {
      // Ignore clipboard permission/focus failures; manual selection still works.
    }
  }, []);

  const blurActiveCopyAction = useCallback((scope: HTMLElement | null) => {
    if (typeof document === "undefined") {
      return;
    }

    const activeElement = document.activeElement;
    if (activeElement instanceof HTMLElement && (!scope || scope.contains(activeElement))) {
      activeElement.blur();
    }
  }, []);

  useEffect(() => () => {
    if (typeof window !== "undefined") {
      window.clearTimeout(messageCopyResetTimeoutRef.current);
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
          parts: [{ type: "text", text: content }],
          status: "completed",
        },
      ];
    });

    if (showThinking) {
      showThinkingPlaceholder(null, userCreatedAt);
    }
  }, [showThinkingPlaceholder]);

  const loadSnapshot = useCallback(async () => {
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
      threadStore.setState({ runtimeContextUsage: null });
      setApprovingPlanMessageId(null);
      setRuntimeError(null);
      if (threadId) runMachine.reset("idle", RESET_IDLE_CONTEXT);
      setSnapshotReady(true);
      setSnapshotThreadId(null);
      setThinkingPlaceholder(null);

      return;
    }

    // Bail out if this loadSnapshot was captured for a different thread
    // than the one currently being displayed — prevents stale closures
    // (e.g. onStop timeout/callback) from overwriting the active thread's
    // UI. This must happen before snapshotLoadRequestRef is incremented so
    // that stale calls don't interfere with the active thread's tracking.
    if (activeThreadIdRef.current !== threadId) {
      return;
    }

    const requestId = snapshotLoadRequestRef.current + 1;
    snapshotLoadRequestRef.current = requestId;

    setLoading(true);
    setHistoryLoadError(null);
    setLoadError(null);
    snapshotLoadingRef.current = true;
    eventBufferRef.current = [];

    try {
      const [snapshot, activeGoal] = await Promise.all([
        threadLoad(threadId),
        goalGetState(threadId).catch(() => undefined),
      ]);
      if (snapshotLoadRequestRef.current !== requestId) {
        // Stale request — do NOT clear snapshotLoadingRef or eventBufferRef
        // because a newer request owns them now.
        return;
      }
      if (activeGoal !== undefined) {
        setThreadGoalState(threadId, activeGoal);
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
        const result = mergeSnapshotMessages(
          snapshotMessages,
          currentMessages,
          lastOptimisticUserIdRef.current,
        );
        lastOptimisticUserIdRef.current = result.lastOptimisticUserId;
        return result.messages;
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

      // Reset the machine to snapshot state, then replay any lifecycle events
      // that were buffered during the async IPC round-trip.  The machine will
      // naturally reject transitions that are invalid from the reset state,
      // but will accept forward transitions (e.g. running → waiting_approval
      // that arrived while the snapshot was in flight).
      if (threadId) {
        snapshotLoadingRef.current = false;
        runMachine.reset(nextState as RunMachineState, {
          runId: snapshot.activeRun?.id ?? null, errorMessage: null, retryCount: 0,
        });
        for (const buffered of eventBufferRef.current) {
          runMachine.send(buffered.event, buffered.payload);
        }
        eventBufferRef.current = [];
      }
      setSelectedRunMode((current) => deriveSelectedRunMode(snapshot, current));
      if (!shouldPreserveContextUsage) {
        threadStore.setState({ runtimeContextUsage: nextContextUsage });
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
        threadStore.setState((prev) => ({ workspaces: prev.workspaces.map((w) => ({ ...w, threads: w.threads.map((t) => t.id === snapshot.thread.id ? { ...t, name: snapshot.thread.title.trim() } : t) })) }));
      }
    } catch (error) {
      if (snapshotLoadRequestRef.current !== requestId) {
        return;
      }

      preserveContextUsageOnNextEmptySnapshotRef.current = false;
      const message = error instanceof Error ? error.message : String(error);
      setLoadError(message);
      threadStore.setState({ runtimeContextUsage: null });
      // Reset run machine so the optimistic "running" state doesn't permanently
      // block the pending run effect when the snapshot IPC fails.
      // Use "failed" rather than "idle" because Guard 2 in threadStore rejects
      // idle/null writes when an optimistic running state with a real runId exists.
      if (threadId) runMachine.reset("failed", { runId: null, errorMessage: message, retryCount: 0 });
      setSnapshotReady(true);
      setSnapshotThreadId(threadId);
    } finally {
      // Only the current (latest) request should clear snapshot-loading state.
      // A stale request must not touch these refs — a newer request owns them.
      if (snapshotLoadRequestRef.current === requestId) {
        snapshotLoadingRef.current = false;
        eventBufferRef.current = [];
        setLoading(false);
      }
    }
  }, [clearScheduledThinkingPhase, showThinkingPlaceholder, threadId]);

  useEffect(() => {
    const isActiveBackendRun = runState === "running" || runState === "waiting_approval" || runState === "needs_reply";
    if (
      !threadId
      || !isActiveBackendRun
      || !activeRunId
      || !streamRef.current
      || streamRef.current.runId
      || subscribingRef.current
    ) {
      return;
    }

    subscribingRef.current = true;
    void streamRef.current.subscribe(threadId)
      .finally(() => {
        subscribingRef.current = false;
      });
  }, [activeRunId, runState, threadId]);

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
      if (threadId) runMachine.reset(nextState as RunMachineState, {
        runId: snapshot.activeRun?.id ?? null, errorMessage: null, retryCount: 0 });
      setSelectedRunMode((current) => deriveSelectedRunMode(snapshot, current));

      const latestVisibleRun = getLatestVisibleRun(snapshot);
      if (latestVisibleRun?.id === runId) {
        const nextContextUsage = mapRunSummaryToContextUsage(latestVisibleRun);
        if (nextContextUsage) {
          threadStore.setState({ runtimeContextUsage: nextContextUsage });
        }
      }

      if (snapshot.thread.title.trim()) {
        threadStore.setState((prev) => ({ workspaces: prev.workspaces.map((w) => ({ ...w, threads: w.threads.map((t) => t.id === snapshot.thread.id ? { ...t, name: snapshot.thread.title.trim() } : t) })) }));
      }
    } catch {
      // Keep the local completed fallback message if snapshot resync is not ready yet.
    }
  }, [threadId]);

  useEffect(() => {
    subscribingRef.current = false;
    // Clear the stop safety-net timeout from a previous thread — it was
    // captured in a stale closure and would load the wrong thread's snapshot.
    if (stopTimerRef.current !== null) {
      clearTimeout(stopTimerRef.current);
      stopTimerRef.current = null;
    }
    pendingThreadRestoreScrollRef.current = Boolean(threadId);
    setComposerError(null);
    setComposerClearSignal((prev) => prev + 1);
    setHelpers([]);
    setHasMoreMessages(false);
    setHistoryLoadError(null);
    setLoadError(null);
    setMessages([]);
    setIsLoadingMoreMessages(false);
    setApprovingPlanMessageId(null);
    setRuntimeQueue(null);
    setCancellingRuntimeQueueMessageIds(new Set());
    setPromotingRuntimeQueueMessageIds(new Set());
    setEditingRuntimeQueueMessageIds(new Set());
    setRuntimeError(null);
    if (threadId) runMachine.reset("idle", RESET_IDLE_CONTEXT);
    setSnapshotReady(false);
    setSnapshotThreadId(null);
    lastOptimisticUserIdRef.current = null;
    clearScheduledThinkingPhase();
    setThinkingPlaceholder(null);
    setTools([]);
    void loadSnapshot();
  }, [clearScheduledThinkingPhase, loadSnapshot, threadId]);

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
      // Route lifecycle events to the run-lifecycle machine for validated
      // state transitions. The machine auto-syncs to threadStore.
      const machineEvent = mapStreamEventToMachineEvent(event.type);
      if (machineEvent && threadId) {
        const payload: RunMachinePayload = {};
        if ("runId" in event && typeof event.runId === "string") {
          payload.runId = event.runId;
        }
        if ("error" in event && typeof event.error === "string") {
          payload.message = event.error;
        }
        if (machineEvent === "RUN_RETRYING" && "runId" in event && typeof event.runId === "string") {
          payload.newRunId = event.runId;
        }
        // Buffer lifecycle events during snapshot loading to prevent the
        // subsequent reset() from overwriting them.  The buffer is replayed
        // after reset() completes — the machine naturally rejects any
        // transitions that are invalid from the reset state.
        if (snapshotLoadingRef.current) {
          eventBufferRef.current.push({ event: machineEvent, payload });
        } else {
          runMachine.send(machineEvent, payload);
        }
      }

      // Goal events don't participate in thinking-phase lifecycle.
      if (event.type === "goal_state_updated") {
        setThreadGoalState(event.threadId, event.goal);
        return;
      }
      if (
        event.type === "goal_continuation" ||
        event.type === "goal_paused" ||
        event.type === "goal_completed"
      ) {
        return;
      }

      if (shouldCompleteThinkingPhase(event)) {
        completeThinkingPhase(event.runId);
      } else if (shouldFinalizeReasoningOnly(event)) {
        clearScheduledThinkingPhase();
        finalizeReasoningForRun(event.runId);
      }

      // Show retry progress in the thinking placeholder for turn-level
      // retries emitted by tiycore as AgentEvent::TurnRetrying.
      if (event.type === "run_retrying") {
        showThinkingPlaceholder(
          event.runId,
          undefined,
          t("run.retrying", {
            attempt: String(event.attempt),
            maxAttempts: String(event.maxAttempts),
          }),
        );
      }

      if (event.type === "request_retrying") {
        const now = new Date().toISOString();
        setRequestRetryEntries((current) => {
          const existing = current.find((entry) => entry.runId === event.runId);
          const nextEntry: SurfaceRequestRetryEntry = {
            createdAt: existing?.createdAt ?? now,
            delayMs: event.delayMs,
            id: existing?.id ?? `request-retry-${event.runId}`,
            maxRetries: event.maxRetries,
            attempt: event.attempt,
            reason: event.reason,
            runId: event.runId,
            status: event.status,
            updatedAt: now,
          };

          if (existing) {
            return current.map((entry) => entry.runId === event.runId ? nextEntry : entry);
          }

          return [...current, nextEntry];
        });
        setRequestRetryOpen((current) => ({
          ...current,
          [`request-retry-${event.runId}`]: event.attempt > 1,
        }));
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

    stream.onUserMessage = withActiveStream((event) => {
      setMessages((current) => appendOrReplaceMessage(current, mapRecordedUserMessage(event)));
      conversationContextRef.current?.scrollToBottom("instant");
    });

    stream.onMessage = withActiveStream((event) => {
      clearRequestRetryForRun(event.runId);

      if (event.kind === "delta") {
        setMessages((current) => {
          const existing = current.find((entry) => entry.id === event.messageId);
          const accumulatedText = existing?.content.concat(event.delta ?? "") ?? (event.delta ?? "");
          const nonTextParts = existing?.parts.filter((p) => p.type !== "text") ?? [];
          let result = appendOrReplaceMessage(current, {
            createdAt: existing?.createdAt ?? new Date().toISOString(),
            id: event.messageId,
            messageType: "plain_message",
            attachments: [],
            role: "assistant",
            runId: event.runId,
            content: accumulatedText,
            parts: [{ type: "text" as const, text: accumulatedText }, ...nonTextParts],
            status: "streaming",
          });
          return result;
        });
        return;
      }

      setMessages((current) => {
        const existing = current.find((entry) => entry.id === event.messageId);
        const nonTextParts = existing?.parts.filter((p) => p.type !== "text") ?? [];
        const result = appendOrReplaceMessage(current, {
          createdAt: existing?.createdAt ?? new Date().toISOString(),
          id: event.messageId,
          messageType: "plain_message",
          attachments: [],
          role: "assistant",
          runId: event.runId,
          content: event.content ?? "",
          parts: [{ type: "text" as const, text: event.content ?? "" }, ...nonTextParts],
          status: "completed",
        });
        return result;
      });

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
            parts: [{ type: "text", text: event.reasoning }],
            status: "streaming",
          },
        ),
      );
    });

    stream.onArtifact = withActiveStream((event) => {
      setMessages((current) => mergeArtifactEventIntoMessages(current, event, new Date().toISOString()));
    });

    stream.onQueue = withActiveStream((event: QueueEvent) => {
      setRuntimeQueue(event.queue);
    });

    stream.onTaskBoard = withActiveStream((event: { taskBoard: TaskBoardDto }) => {
      setTaskBoards((current) => applyTaskBoardUpdate(current, event.taskBoard));
    });

    stream.onThreadTitle = withActiveStream((event: ThreadTitleEvent) => {
      threadStore.setState((prev) => ({ workspaces: prev.workspaces.map((w) => ({ ...w, threads: w.threads.map((t) => t.id === event.threadId ? { ...t, name: event.title } : t) })) }));
    });

    stream.onUsage = withActiveStream((event: UsageEvent) => {
      threadStore.setState({
        runtimeContextUsage: {
          contextWindow: event.contextWindow,
          inputTokens: event.usage.inputTokens,
          outputTokens: event.usage.outputTokens,
          cacheReadTokens: event.usage.cacheReadTokens,
          cacheWriteTokens: event.usage.cacheWriteTokens,
          totalTokens: event.usage.totalTokens,
          modelDisplayName: event.modelDisplayName,
          runId: event.runId,
        },
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
      // Machine already handles state transitions via onRawEvent.
      // Perform side effects based on the new state.

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
        clearRequestRetryForRun(runId);
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
    // Register this thread's machine to receive global Tauri events.
    if (threadId) registerRunMachine(threadId, runMachine);
    return () => {
      if (threadId) unregisterRunMachine(threadId);
      runMachine.destroy();
      streamRef.current = null;
      subscribingRef.current = false;
      clearScheduledThinkingPhase();
      stream.dispose();
    };
  }, [
    clearRequestRetryForRun,
    clearScheduledThinkingPhase,
    completeThinkingPhase,
    loadSnapshot,
    resyncCompletedMessage,
    runMachine,
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
        const threadStatus = threadStore.getState().threadStatuses[threadId];
        const currentStatus = threadStatus?.status ?? "idle";
        if (currentStatus === "running" || currentStatus === "waiting_approval" || currentStatus === "needs_reply") {
          void loadSnapshot();
        }
      },
    );

    return () => {
      setup.then((fn) => fn());
    };
  }, [loadSnapshot, threadId]);

  const submitPrompt = useCallback(async (
    submissionOrPrompt: ComposerSubmission | string,
    runModeOverride?: RunMode,
  ): Promise<boolean> => {
    if (!threadId) {
      setComposerError("This thread is still preparing. Try again in a moment.");
      return false;
    }

    let submission = typeof submissionOrPrompt === "string"
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
      return false;
    }

    if (!activeProfile) {
      setComposerError(
        hasMissingActiveProfile
          ? t("composer.profileDeletedHint")
          : "Select an agent profile with an enabled model before starting a run.",
      );
      return false;
    }

    const activeRunId = streamRef.current?.runId ?? null;
    if (runState === "running" && activeRunId) {
      if (submission.attachments.length > 0) {
        setComposerError("Attachments can only be sent when starting a new run.");
        return false;
      }
      setComposerError(null);
      setRuntimeError(null);
      try {
        const queue = await streamRef.current?.enqueueQueueMessage(
          threadId,
          runtimeQueueSubmitMode,
          prompt,
          submission.metadata ?? null,
        );
        if (queue) {
          setRuntimeQueue(queue);
        }
        conversationContextRef.current?.scrollToBottom("instant");
      } catch (error) {
        setThinkingPlaceholder(null);
        throw error;
      }
      return true;
    }

    if (runState === "waiting_approval" && activeRunId) {
      setComposerError(t("queue.waitingApprovalError"));
      return false;
    }

    if (runState === "needs_reply" && activeRunId) {
      setComposerError("Reply to the pending question before starting a new run.");
      return false;
    }

    // Guard against concurrent invocations. The `initialPromptRequest` effect
    // may re-fire while an `await startRun()` is still in flight because
    // `runState` hasn't transitioned to "running" yet (it only changes when
    // the Rust backend sends back a `run_started` event). Without this ref
    // guard, a second `startRun` invoke reaches Rust where the first run is
    // already registered in `active_runs`, producing `thread.run.already_active`.
    if (submittingRef.current) {
      return false;
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
      return false;
    }

    setComposerError(null);
    setRuntimeError(null);

    if (submission.kind === "command" && submission.command?.behavior === "clear") {
      appendOptimisticUserMessage(submission.displayText, submission.metadata ?? null, [], false);
      conversationContextRef.current?.scrollToBottom("instant");
      try {
        preserveContextUsageOnNextEmptySnapshotRef.current = false;
        threadStore.setState({ runtimeContextUsage: null });
        await threadClearContext(threadId);
        await loadSnapshot();
      } finally {
        submittingRef.current = false;
      }
      return true;
    }

    if (submission.kind === "command" && submission.command?.behavior === "compact") {
      appendOptimisticUserMessage(submission.displayText, submission.metadata ?? null, [], false);
      conversationContextRef.current?.scrollToBottom("instant");
      try {
        preserveContextUsageOnNextEmptySnapshotRef.current = false;
        threadStore.setState({ runtimeContextUsage: null });
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
      return true;
    }

    if (submission.kind === "command" && submission.command?.behavior === "goal") {
      const argText = (submission.command.argumentsText ?? "").trim();

      try {
        // /goal <objective> — persist the goal, then start a run
        if (!argText) {
          setComposerError("Provide a goal objective, e.g. /goal fix the auth bugs");
          submittingRef.current = false;
          return true;
        }
        // Reject old sub-commands that are now handled by GoalStatusBar buttons
        const OBSOLETE_SUBCOMMANDS = new Set([
          "pause", "resume", "clear", "status",
          "取消", "查看状态",
        ]);
        const firstWord = argText.split(/\s+/)[0] ?? "";
        const firstWordLower = firstWord.toLowerCase();
        if (OBSOLETE_SUBCOMMANDS.has(firstWordLower)) {
          setComposerError(
            `"${firstWord}" is now available via the goal status bar — use the ⏸ / ▶ / ✕ buttons instead.`,
          );
          submittingRef.current = false;
          return true;
        }
        await goalSet(threadId, argText);
        // Sync goal state to threadStore immediately so GoalStatusBar appears
        try {
          setThreadGoalState(threadId, await goalGetState(threadId));
        } catch {
          // Goal state fetch can fail silently — the status bar will
          // pick it up on the next goal_evaluate cycle anyway.
        }
      } catch (error) {
        setComposerError(`Failed to manage goal: ${error}`);
        submittingRef.current = false;
        return false;
      }

      // Build a structured kickoff prompt so the model knows the goal exists
      const kickoffPrompt = [
        "## Persistent Goal Started",
        "",
        "You are now working on the following goal:",
        "",
        "**" + argText + "**",
        "",
        "This goal has been created and is now **active**. Work toward it.",
        "When the goal is fully achieved, you MUST call:",
        "```json",
        "goal_scored(status=\"complete\", evidence=\"test output, file changes, verification steps\", pledge=\"I hereby declare: I confirm that I have fully achieved this goal, and I have confirmed that there are no remaining pending tasks or follow-up items. I confirm that I have repeatedly reviewed the output of this work, and I take responsibility for the quality of this output.\")",
        "```",
        "Do NOT mark complete without verified evidence.",
        "",
        "If you need user input before proceeding, use the clarify tool.",
        "The goal will automatically pause and resume when the user responds.",
      ].join("\n");

      submission = {
        ...submission,
        displayText: argText,
        effectivePrompt: kickoffPrompt,
        kind: "plain",
      };
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
          prompt: submission.effectivePrompt ?? "",
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
    return true;
  }, [activeAgentProfileId, activeProfile, agentProfiles, appendOptimisticUserMessage, loadSnapshot, providers, runState, runtimeQueueSubmitMode, selectedRunMode, t, threadId]);

  const cancelRuntimeQueueMessage = useCallback(async (messageId: string) => {
    if (!threadId || !streamRef.current) {
      return;
    }

    setComposerError(null);
    setRuntimeError(null);
    setCancellingRuntimeQueueMessageIds((current) => {
      const next = new Set(current);
      next.add(messageId);
      return next;
    });

    try {
      const queue = await streamRef.current.cancelRuntimeQueueMessage(threadId, messageId);
      setRuntimeQueue(queue);
    } catch {
      // ThreadStream already routes the formatted backend error to runtimeError.
    } finally {
      setCancellingRuntimeQueueMessageIds((current) => {
        const next = new Set(current);
        next.delete(messageId);
        return next;
      });
    }
  }, [threadId]);

  const promoteRuntimeQueueMessage = useCallback(async (messageId: string) => {
    if (!threadId || !streamRef.current) {
      return;
    }

    setComposerError(null);
    setRuntimeError(null);
    setPromotingRuntimeQueueMessageIds((current) => {
      const next = new Set(current);
      next.add(messageId);
      return next;
    });

    try {
      const queue = await streamRef.current.promoteRuntimeQueueMessage(threadId, messageId);
      setRuntimeQueue(queue);
    } catch {
      // ThreadStream already routes the formatted backend error to runtimeError.
    } finally {
      setPromotingRuntimeQueueMessageIds((current) => {
        const next = new Set(current);
        next.delete(messageId);
        return next;
      });
    }
  }, [threadId]);

  const restoreRuntimeQueueMessageToComposer = useCallback((message: RuntimeQueueMessageDto) => {
    if (!threadId) {
      return;
    }

    const composer = parseRuntimeQueueComposerMetadata(message.metadata);
    const displayText = composer?.displayText?.trim()
      ? composer.displayText
      : message.content;
    const referencedFiles = composer?.referencedFiles ?? [];
    const existing = getDraft(threadId);
    setDraft(threadId, {
      ...existing,
      text: displayText,
      referencedFiles: referencedFiles as ComposerReferencedFile[],
      attachmentData: [],
    });
    setRuntimeQueueSubmitMode(message.kind);
    setComposerClearSignal((current) => current + 1);
    setComposerRestoreSignal((current) => current + 1);
  }, [threadId]);

  const editRuntimeQueueMessage = useCallback(async (message: RuntimeQueueMessageDto) => {
    if (!threadId || !streamRef.current) {
      return;
    }

    setComposerError(null);
    setRuntimeError(null);
    setEditingRuntimeQueueMessageIds((current) => {
      const next = new Set(current);
      next.add(message.id);
      return next;
    });

    try {
      const queue = await streamRef.current.cancelRuntimeQueueMessage(threadId, message.id);
      setRuntimeQueue(queue);
      restoreRuntimeQueueMessageToComposer(message);
    } catch {
      // ThreadStream already routes the formatted backend error to runtimeError.
    } finally {
      setEditingRuntimeQueueMessageIds((current) => {
        const next = new Set(current);
        next.delete(message.id);
        return next;
      });
    }
  }, [restoreRuntimeQueueMessageToComposer, threadId]);

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
    const initialPromptRequest = threadId ? (pendingRuns[threadId] ?? null) : null;
    const initialPromptRequestId = initialPromptRequest?.id ?? null;
    const hasBlockingRun =
      runState === "running"
      || ((runState === "waiting_approval" || runState === "needs_reply") && Boolean(streamRef.current?.runId));

    if (
      !initialPromptRequest
      || initialPromptRequest.threadId !== threadId
      || !isCurrentThreadSnapshotReady
      || hasBlockingRun
      || isPendingRunHandled(initialPromptRequestId!)
    ) {
      return;
    }

    // Mark as handled at the store level — survives component unmount/remount.
    markPendingRunHandled(initialPromptRequestId!);
    if (initialPromptRequest.runMode) {
      setSelectedRunMode(initialPromptRequest.runMode);
    }
    const pendingRunSubmission: ComposerSubmission = {
      kind: initialPromptRequest.command ? "command" : "plain",
      displayText: initialPromptRequest.displayText,
      effectivePrompt: initialPromptRequest.effectivePrompt,
      rawMessage: { text: initialPromptRequest.displayText, files: [] },
      attachments: initialPromptRequest.attachments,
      command: initialPromptRequest.command,
      metadata: initialPromptRequest.metadata,
      runMode: initialPromptRequest.runMode,
    };
    void submitPrompt(pendingRunSubmission, initialPromptRequest.runMode)
      .finally(() => {
        threadStore.setState((prev) => {
          const next = Object.fromEntries(
            Object.entries(prev.pendingRuns).filter(([, r]) => r.id !== initialPromptRequest.id)
          );
          return Object.keys(next).length === Object.keys(prev.pendingRuns).length ? {} : { pendingRuns: next };
        });
      });
  }, [pendingRuns, runState, snapshotReady, snapshotThreadId, submitPrompt, threadId]);

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
  const hasPendingRuntimeQueue = Boolean(
    runtimeQueue?.messages.some((message) => message.status === "pending"),
  );
  const composerReferencedFiles = threadId
    ? getDraft(threadId).referencedFiles
    : [];
  const composerAttachmentData = threadId
    ? getDraft(threadId).attachmentData
    : undefined;
  const hasTaskHistoryTimeline = taskBoards.boards.some((board) => board.status !== "active");
  // Keep empty-state suppression aligned with panels that are actually visible:
  // the composer-adjacent queue panel only renders pending messages.
  const hasRuntimeArtifacts =
    Boolean(runtimeError)
    || hasPendingRuntimeQueue
    || hasTaskHistoryTimeline
    || requestRetryEntries.length > 0
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
        ...requestRetryEntries.map((requestRetry) => ({
          kind: "request_retry" as const,
          key: `request-retry:${requestRetry.id}`,
          occurredAt: requestRetry.createdAt,
          requestRetry,
        })),
        ...visibleTools.map((tool) => ({
          kind: "tool" as const,
          key: `tool:${tool.id}`,
          occurredAt: tool.startedAt,
          tool,
        })),
      ].sort(compareTimelineEntries),
    [helpers, messages, requestRetryEntries, visibleTools],
  );
  const presentationEntries = timelineEntries;

  const assistantRunCopyState = useMemo(() => {
    const copyableMessages: SurfaceMessage[] = [];

    for (const entry of presentationEntries) {
      if (entry.kind === "message") {
        copyableMessages.push(entry.message);
      }
    }

    const activeCopyExcludedRunIds = activeRunId && (
      runState === "running"
      || runState === "waiting_approval"
      || runState === "needs_reply"
    ) ? new Set([activeRunId]) : undefined;

    return getAssistantRunCopyState(copyableMessages, {
      excludedRunIds: activeCopyExcludedRunIds,
    });
  }, [activeRunId, presentationEntries, runState]);

  // Build entries for the delayed auto-collapse hook.
  const delayedCollapseEntries = useMemo<ReadonlyArray<DelayedAutoCollapseEntry>>(() => {
    const result: DelayedAutoCollapseEntry[] = [];
    for (const entry of presentationEntries) {
      if (entry.kind === "tool") {
        const isCompleted = isCompletedToolState(entry.tool.state);
        const isOpen = getDefaultToolOpenState(entry.tool.name, entry.tool.state, completedToolOpen[entry.tool.id]);
        result.push({ id: entry.tool.id, completed: isCompleted, currentOpen: isOpen });
      } else if (entry.kind === "helper") {
        const isCompleted = entry.helper.status === "completed";
        const isOpen = helperOpen[entry.helper.id] ?? !isCompleted;
        result.push({ id: entry.helper.id, completed: isCompleted, currentOpen: isOpen });
      } else if (entry.kind === "request_retry") {
        const isOpen = requestRetryOpen[entry.requestRetry.id] ?? entry.requestRetry.attempt > 1;
        result.push({ id: entry.requestRetry.id, completed: true, currentOpen: isOpen });
      } else if (entry.kind === "message" && entry.message.messageType === "reasoning") {
        const isCompleted = entry.message.status !== "streaming";
        const isOpen = reasoningOpen[entry.message.id] ?? !isCompleted;
        result.push({ id: entry.message.id, completed: isCompleted, currentOpen: isOpen });
      }
    }
    return result;
  }, [presentationEntries, completedToolOpen, helperOpen, requestRetryOpen, reasoningOpen]);

  // Keep a ref to presentationEntries for the delayed collapse callback.
  const presentationEntriesRef = useRef(presentationEntries);
  presentationEntriesRef.current = presentationEntries;

  const handleDelayedCollapse = useCallback((id: string) => {
    // Determine whether this id belongs to a tool, helper, request retry, or reasoning
    // and only update the relevant state map.
    const entry = presentationEntriesRef.current.find(
      (e) =>
        (e.kind === "tool" && e.tool.id === id)
        || (e.kind === "helper" && e.helper.id === id)
        || (e.kind === "request_retry" && e.requestRetry.id === id)
        || (e.kind === "message" && e.message.messageType === "reasoning" && e.message.id === id),
    );
    if (!entry) return;
    if (entry.kind === "tool") {
      setCompletedToolOpen((current) => (current[id] === false ? current : { ...current, [id]: false }));
    } else if (entry.kind === "helper") {
      setHelperOpen((current) => (current[id] === false ? current : { ...current, [id]: false }));
    } else if (entry.kind === "request_retry") {
      setRequestRetryOpen((current) => (current[id] === false ? current : { ...current, [id]: false }));
    } else {
      setReasoningOpen((current) => (current[id] === false ? current : { ...current, [id]: false }));
    }
  }, []);

  useDelayedAutoCollapse({
    delayMs: THREAD_AUTO_COLLAPSE_DELAY_MS,
    entries: delayedCollapseEntries,
    userManuallyOpenedIds: userManuallyOpenedIds.current,
    onCollapse: handleDelayedCollapse,
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
  const runtimeErrorPreviousRole: TimelineRole | null = showThinkingIndicator
    ? "assistant"
    : lastPresentationRole;

  const hasComposerStatusPanel = hasPendingRuntimeQueue || hasTaskHistoryTimeline;
  const conversationBottomPadding = BASE_CONVERSATION_BOTTOM_PADDING;

  useEffect(() => {
    const previousToolStates = previousToolStatesRef.current;
    const nextToolStates = Object.fromEntries(visibleTools.map((tool) => [tool.id, tool.state]));

    setCompletedToolOpen((current) => {
      const next: Record<string, boolean> = { ...current };

      for (const tool of visibleTools) {
        const previousState = previousToolStates[tool.id];

        if (previousState !== tool.state) {
          // State changed — keep the block open (don't auto-collapse on
          // completion).  Only force open when transitioning *to* a
          // non-completed state so newly-started tools expand.
          // Default-collapsed tools always stay collapsed regardless of state.
          if (isDefaultCollapsedTool(tool.name)) {
            next[tool.id] = tool.id in current ? current[tool.id] : false;
          } else if (!isCompletedToolState(tool.state)) {
            next[tool.id] = true;
          } else {
            // Completed: preserve current open state; otherwise default to collapsed.
            next[tool.id] = tool.id in current ? current[tool.id] : false;
          }
          continue;
        }

        if (tool.id in current) {
          next[tool.id] = current[tool.id];
          continue;
        }

        next[tool.id] = isDefaultCollapsedTool(tool.name) ? false : !isCompletedToolState(tool.state);
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
      const next: Record<string, boolean> = { ...current };

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
            next[helper.id] = helper.id in current ? current[helper.id] : false;
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
    if (!trimmedPrompt && submission.attachments.length === 0) {
      return;
    }

    if (runState === "running" && streamRef.current?.runId && submission.attachments.length > 0) {
      setComposerError("Attachments can only be sent when starting a new run.");
      throw new Error("submit_rejected");
    }

    if (pendingClarifyTool) {
      setComposerValue("");
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

    const accepted = await submitPrompt(submission);
    if (!accepted) {
      // Throw so that upstream layers (workbench-prompt-composer and
      // prompt-input) detect the rejection and preserve composer state
      // (referenced files, attachments, $skills, etc.).
      throw new Error("submit_rejected");
    }
    setComposerValue("");
  }, [pendingClarifyTool, respondToClarify, runState, submitPrompt]);

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
      threadStore.setState({ runtimeContextUsage: null });
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
        {threadId && <GoalStatusBar threadId={threadId} />}
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
                title={"No messages yet"}
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
                  const reasoningIsOpen = reasoningOpen[message.id] ?? reasoningIsStreaming;
                  return (
                    <div className={spacingClass} key={entry.key}>
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

                const commandComposer = message.role === "user"
                  ? parseCommandComposerMetadata(message.metadata)
                  : null;
                const expandedPrompt = commandComposer?.kind === "command"
                  ? (commandComposer.effectivePrompt?.trim() ?? "")
                  : "";
                const goalContinuation = message.role === "user"
                  ? parseGoalContinuationMetadata(message.metadata)
                  : null;
                const messageCopyTarget = (() => {
                  if (message.role === "user") {
                    return {
                      canCopy: message.status !== "discarded",
                      id: `message:${message.id}`,
                      text: getCopyableThreadMessageText(message, commandComposer),
                    };
                  }

                  if (message.role !== "assistant" || message.status === "discarded") {
                    return { canCopy: false, id: `message:${message.id}`, text: "" };
                  }

                  if (!message.runId) {
                    return {
                      canCopy: true,
                      id: `message:${message.id}`,
                      text: getCopyableThreadMessageText(message),
                    };
                  }

                  return {
                    canCopy: assistantRunCopyState.buttonMessageIdByRunId[message.runId] === message.id,
                    id: `assistant-run:${message.runId}`,
                    text: assistantRunCopyState.textByRunId[message.runId] ?? "",
                  };
                })();
                const copyableText = messageCopyTarget.text;
                const canCopyMessage = messageCopyTarget.canCopy && copyableText.length > 0;
                const copied = copiedCopyTargetId === messageCopyTarget.id;
                const copyLabel = copied ? t("message.copied") : t("message.copy");

                return (
                  <div className={spacingClass} key={entry.key}>
                    {goalContinuation ? (
                      <div className="flex items-center gap-3 py-2">
                        <div className="h-px flex-1 bg-app-border/28" />
                        <span className="text-[11px] font-medium tracking-[0.04em] text-app-subtle">
                          Goal continues
                          {goalContinuation.turnsUsed !== null && goalContinuation.maxTurns !== null
                            ? ` — turn ${goalContinuation.turnsUsed}/${goalContinuation.maxTurns}`
                            : ""}
                        </span>
                        <div className="h-px flex-1 bg-app-border/28" />
                      </div>
                    ) : null}
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
                        )}
                      </MessageContent>
                      {canCopyMessage ? (
                        <MessageActions
                          className={cn(
                            "pointer-events-none h-7 opacity-0 transition-opacity duration-150 group-hover:pointer-events-auto group-hover:opacity-100 focus-within:pointer-events-auto focus-within:opacity-100",
                            message.role === "user" ? "ml-auto justify-end" : "justify-start",
                          )}
                          onMouseLeave={(event) => blurActiveCopyAction(event.currentTarget)}
                        >
                          <MessageAction
                            aria-label={copyLabel}
                            className={cn(
                              "size-7 rounded-md border border-transparent bg-transparent text-app-subtle/85 shadow-none hover:bg-app-surface-hover hover:text-app-foreground focus-visible:bg-app-surface-hover",
                              copied && "bg-app-surface-hover text-app-foreground",
                            )}
                            label={copyLabel}
                            onClick={() => void handleCopyMessage(messageCopyTarget.id, copyableText)}
                            onMouseLeave={(event) => event.currentTarget.blur()}
                            tooltip={copyLabel}
                          >
                            {copied ? <CheckIcon className="size-3.5" /> : <CopyIcon className="size-3.5" />}
                          </MessageAction>
                        </MessageActions>
                      ) : null}
                    </Message>
                  </div>
                );
              }

              if (entry.kind === "request_retry") {
                const { requestRetry } = entry;
                const open = requestRetryOpen[requestRetry.id] ?? requestRetry.attempt > 1;
                const detailParts = [
                  requestRetry.status !== null
                    ? t("requestRetry.status", { status: String(requestRetry.status) })
                    : null,
                  requestRetry.delayMs > 0
                    ? t("requestRetry.delay", { seconds: (requestRetry.delayMs / 1000).toFixed(1) })
                    : null,
                ].filter(Boolean).join(" · ");

                return (
                  <div className={spacingClass} key={entry.key}>
                    <Message className="max-w-full" from="assistant">
                      <MessageContent className="w-full max-w-full bg-transparent px-0 py-0 shadow-none">
                        <div className="text-app-muted">
                          <button
                            aria-expanded={open}
                            className="group inline-flex max-w-full items-center gap-2 text-left text-sm font-medium leading-5 text-app-muted transition-colors hover:text-app-foreground"
                            onClick={() => {
                              const nextOpen = !open;
                              setRequestRetryOpen((current) => ({
                                ...current,
                                [requestRetry.id]: nextOpen,
                              }));
                              if (nextOpen) {
                                userManuallyOpenedIds.current.add(requestRetry.id);
                              }
                            }}
                            type="button"
                          >
                            <span className="truncate">
                              {t("requestRetry.reconnecting", {
                                attempt: String(requestRetry.attempt),
                                maxRetries: String(requestRetry.maxRetries),
                              })}
                            </span>
                            <ChevronDownIcon
                              className={cn(
                                "mt-1 size-5 shrink-0 transition-transform text-app-muted/70",
                                open ? "rotate-180" : "rotate-0",
                              )}
                            />
                          </button>
                          {open ? (
                            <div className="mt-2 max-w-3xl space-y-2 text-sm leading-6 text-app-muted">
                              <p className="whitespace-pre-wrap break-words">{requestRetry.reason}</p>
                              {detailParts ? (
                                <p className="text-sm leading-5 text-app-subtle">{detailParts}</p>
                              ) : null}
                            </div>
                          ) : null}
                        </div>
                      </MessageContent>
                    </Message>
                  </div>
                );
              }

              if (entry.kind === "helper") {
                const { helper } = entry;
                const helperName = formatHelperName(helper, customAgentSlugToName);
                const helperDetailSummary = formatHelperDetailSummary(helper);
                const helperSummary = formatHelperSummary(helper, customAgentSlugToName);
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
          <ConversationScrollButton />
        </Conversation>
      </div>

      <div className="shrink-0 px-6 pb-6 pt-4">
        <div className="mx-auto flex w-full max-w-4xl flex-col gap-0">
          {hasComposerStatusPanel ? (
            <div className="min-h-0 max-h-[min(24vh,220px)] overflow-y-auto rounded-t-[24px] rounded-b-none border border-b-0 border-app-border/80 bg-app-menu/96 px-3 py-2 shadow-[0_26px_70px_-42px_rgba(15,23,42,0.45)] backdrop-blur-xl">
              <div className="flex flex-col gap-2">
                {hasPendingRuntimeQueue ? (
                  <RuntimeQueueTimeline
                    queue={runtimeQueue}
                    variant="compact"
                    onCancelMessage={cancelRuntimeQueueMessage}
                    cancellingMessageIds={cancellingRuntimeQueueMessageIds}
                    onPromoteMessage={promoteRuntimeQueueMessage}
                    promotingMessageIds={promotingRuntimeQueueMessageIds}
                    onEditMessage={editRuntimeQueueMessage}
                    editingMessageIds={editingRuntimeQueueMessageIds}
                  />
                ) : null}
                {hasTaskHistoryTimeline ? (
                  <TaskHistoryTimeline boards={taskBoards.boards} />
                ) : null}
              </div>
            </div>
          ) : null}
          {taskBoards.activeBoard ? (
            <div
              className={cn(
                "min-h-0 max-h-[min(36vh,320px)] overflow-hidden border border-b-0 border-app-border/80 bg-app-menu/96 px-2 pb-0 pt-2 shadow-[0_26px_70px_-42px_rgba(15,23,42,0.45)] backdrop-blur-xl",
                hasComposerStatusPanel ? "rounded-none" : "rounded-t-[24px] rounded-b-none",
              )}
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
            canSubmitWhileRunning={runState === "running" && Boolean(streamRef.current?.runId)}
            className="w-full max-w-none gap-0"
            commands={commands}
            customSubagents={customSubagents}
            composerShellClassName={taskBoards.activeBoard || hasComposerStatusPanel
              ? "rounded-t-none border-t-0"
              : undefined}
            enabledSkills={enabledSkills}
            error={composerError}
            onErrorMessageChange={setComposerError}
            onOpenProfileSettings={() => {
              uiLayoutStore.setState({ activeOverlay: "settings", activeSettingsCategory: "profiles" });
            }}
            onRuntimeQueueSubmitModeChange={setRuntimeQueueSubmitMode}
            onUpdateAgentProfile={updateAgentProfile}
            onSelectAgentProfile={async (profileId: string) => {
              // In new-thread mode, just update the global active profile.
              if (isNewThreadMode || !threadId) {
                settingsStore.setState({ activeAgentProfileId: profileId });
                return;
              }
              try {
                await threadUpdateProfile(threadId, profileId);
              } catch (error) {
                const message = error instanceof Error ? error.message : getInvokeErrorMessage(error, "Failed to switch profile");
                composerStore.setState({ error: message });
                return;
              }
              threadStore.setState((prev) => ({
                workspaces: prev.workspaces.map((w) => ({
                  ...w,
                  threads: w.threads.map((t) =>
                    t.id === threadId ? { ...t, profileId } : t
                  ),
                })),
                activeThreadProfileIdOverride: profileId,
              }));
            }}
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
                runMachine.send("RUN_CANCELLED");

                // Safety net: if the backend event (`run_cancelled`) hasn't
                // arrived within 5 seconds, force a snapshot reload to reconcile
                // the UI with the actual backend state.
                const timer = setTimeout(() => {
                  void loadSnapshot();
                }, 5_000);
                stopTimerRef.current = timer;

                // If the stream delivers a terminal event before the timeout,
                // the next `onRunStateChange` + `loadSnapshot` will render the
                // correct state and this timer becomes a harmless no-op.
                return () => {
                  clearTimeout(timer);
                  stopTimerRef.current = null;
                };
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
            runtimeQueueSubmitMode={runtimeQueueSubmitMode}
            showRuntimeQueueSubmitMode={runState === "running" && Boolean(streamRef.current?.runId)}
            status={composerStatus}
            value={composerValue}
            workspaceId={
              threadId
                ? findWorkspaceForThread(
                    threadStore.getState().workspaces,
                    threadId,
                  )?.id ?? undefined
                : undefined
            }
            onValueChange={setComposerValue}
            initialReferencedFiles={composerReferencedFiles}
            initialAttachmentData={composerAttachmentData}
            restoreSignal={composerRestoreSignal}
            clearSignal={composerClearSignal}
            onReferencedFilesChange={(files) => {
              if (threadId) {
                const existing = getDraft(threadId);
                setDraft(threadId, { ...existing, referencedFiles: files as ComposerReferencedFile[] });
              }
            }}
            onAttachmentDataChange={(data: ReadonlyArray<SerializableAttachment>) => {
              if (threadId) {
                const existing = getDraft(threadId);
                setDraft(threadId, { ...existing, attachmentData: data as SerializableAttachment[] });
              }
            }}
          />
        </div>
      </div>
    </div>
  );
}
