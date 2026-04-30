/**
 * Thread Stream adapter.
 *
 * Maps raw ThreadStreamEvent from Rust into AI Elements-friendly
 * state updates that React components can consume.
 *
 * Usage:
 *   const stream = new ThreadStream();
 *   stream.onMessage = (msg) => updateConversation(msg);
 *   stream.onToolEvent = (ev) => updateToolStatus(ev);
 *   stream.onApproval = (ev) => showApprovalDialog(ev);
 *   stream.onRunStateChange = (state) => updateRunIndicator(state);
 *
 *   // Start a run — events flow automatically
 *   await stream.startRun(threadId, prompt);
 */

import {
  threadCancelRun,
  threadCompactContext,
  threadExecuteApprovedPlan,
  threadStartRun,
  threadSubscribeRun,
  toolApprovalRespond,
  toolClarifyRespond,
  type ThreadRunInput,
} from "@/services/bridge";
import type {
  RunModelPlanDto,
  RunUsageDto,
  SubagentActivityStatus,
  SubagentProgressSnapshot,
  TaskBoardDto,
} from "@/shared/types/api";
import type { ThreadStreamEvent } from "./types";
import { formatInvokeErrorMessage } from "@/shared/lib/invoke-error";

// ---------------------------------------------------------------------------
// Callback types for AI Elements mapping
// ---------------------------------------------------------------------------

export type MessageEvent = {
  kind: "delta" | "completed";
  messageId: string;
  runId: string;
  delta?: string;
  content?: string;
};

export type ToolEvent = {
  kind:
    | "requested"
    | "running"
    | "completed"
    | "failed"
    | "clarify-required"
    | "clarify-resolved";
  runId: string;
  toolCallId: string;
  toolName?: string;
  toolInput?: unknown;
  result?: unknown;
  error?: string;
  response?: unknown;
};

export type ApprovalEvent = {
  kind: "required" | "resolved";
  runId: string;
  toolCallId: string;
  toolName?: string;
  toolInput?: unknown;
  reason?: string;
  approved?: boolean;
};

export type RunState =
  | "idle"
  | "running"
  | "waiting_approval"
  | "needs_reply"
  | "limit_reached"
  | "completed"
  | "failed"
  | "cancelled"
  | "interrupted";

export type PlanEvent = {
  runId: string;
  plan: unknown;
};

export type ReasoningEvent = {
  runId: string;
  messageId: string;
  reasoning: string;
};

export type QueueEvent = {
  runId: string;
  queue: unknown;
};

export type HelperEvent =
  | {
      kind: "started";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      kind: "progress";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      activity: SubagentActivityStatus;
      message: string;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      kind: "completed";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      summary?: string | null;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      kind: "failed";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      error: string;
      snapshot: SubagentProgressSnapshot;
    };

export type ThreadTitleEvent = {
  runId: string;
  threadId: string;
  title: string;
};

export type UsageEvent = {
  runId: string;
  modelDisplayName: string | null;
  contextWindow: string | null;
  usage: RunUsageDto;
};

// ---------------------------------------------------------------------------
// ThreadStream class
// ---------------------------------------------------------------------------

export class ThreadStream {
  // Callbacks — set by the consuming component
  onMessage: ((event: MessageEvent) => void) | null = null;
  onToolEvent: ((event: ToolEvent) => void) | null = null;
  onApproval: ((event: ApprovalEvent) => void) | null = null;
  onRunStateChange: ((state: RunState, runId: string) => void) | null = null;
  onContextCompressing: ((runId: string) => void) | null = null;
  onPlan: ((event: PlanEvent) => void) | null = null;
  onReasoning: ((event: ReasoningEvent) => void) | null = null;
  onQueue: ((event: QueueEvent) => void) | null = null;
  onTaskBoard: ((event: { taskBoard: TaskBoardDto }) => void) | null = null;
  onHelperEvent: ((event: HelperEvent) => void) | null = null;
  onThreadTitle: ((event: ThreadTitleEvent) => void) | null = null;
  onUsage: ((event: UsageEvent) => void) | null = null;
  onError: ((error: string, runId: string) => void) | null = null;
  onRawEvent: ((event: ThreadStreamEvent) => void) | null = null;

  private currentRunId: string | null = null;
  private hiddenToolCallIds = new Set<string>();
  private toolNameCache = new Map<string, string>();
  private disposed = false;

  get runId() {
    return this.currentRunId;
  }

  get isActive() {
    return this.currentRunId !== null;
  }

  /**
   * Start a new run. Events will flow to the registered callbacks.
   */
  async startRun(
    threadId: string,
    input: ThreadRunInput,
    runMode?: string,
    modelPlan?: RunModelPlanDto | null,
  ): Promise<string> {
    try {
      const runId = await threadStartRun(threadId, input, (event) => {
        if (this.disposed) {
          return;
        }
        this.handleEvent(event);
      }, runMode, modelPlan);

      if (this.disposed) {
        return runId;
      }
      this.currentRunId = runId;
      return runId;
    } catch (error) {
      const message = formatInvokeErrorMessage(error) ?? "Unknown error";
      if (!this.disposed) {
        this.onError?.(message, "");
      }
      throw error;
    }
  }

  /**
   * Subscribe to an already-running thread so a remounted surface can resume
   * receiving live updates after loading the persisted snapshot.
   */
  async subscribe(threadId: string): Promise<string | null> {
    try {
      const runId = await threadSubscribeRun(threadId, (event) => {
        if (this.disposed) {
          return;
        }
        this.handleEvent(event);
      });

      if (this.disposed) {
        return runId;
      }

      this.currentRunId = runId;
      return runId;
    } catch (error) {
      const message = formatInvokeErrorMessage(error) ?? "Unknown error";
      if (!this.disposed) {
        this.onError?.(message, "");
      }
      throw error;
    }
  }

  /**
   * Cancel the currently active run.
   */
  async cancelRun(threadId: string): Promise<boolean> {
    try {
      return await threadCancelRun(threadId);
    } catch (error) {
      const message = formatInvokeErrorMessage(error) ?? "Unknown error";
      this.onError?.(message, this.currentRunId ?? "");
      throw error;
    }
  }

  async executeApprovedPlan(
    threadId: string,
    approvalMessageId: string,
    action: "apply_plan" | "apply_plan_with_context_reset",
  ): Promise<string> {
    try {
      const runId = await threadExecuteApprovedPlan(
        threadId,
        approvalMessageId,
        action,
        (event) => {
          if (this.disposed) {
            return;
          }
          this.handleEvent(event);
        },
      );

      if (this.disposed) {
        return runId;
      }

      this.currentRunId = runId;
      return runId;
    } catch (error) {
      const message = formatInvokeErrorMessage(error) ?? "Unknown error";
      if (!this.disposed) {
        this.onError?.(message, "");
      }
      throw error;
    }
  }

  /**
   * Kick off a manual `/compact` and route its ThreadStreamEvents through
   * the same pipeline as a regular run. Unlike `startRun`, the backend
   * performs the LLM summary in a spawned task; events like
   * `context_compressing` and `run_completed` arrive asynchronously so the
   * UI can show the "Compressing context…" thinking placeholder and a
   * running thread state.
   */
  async compactContext(
    threadId: string,
    instructions: string | null,
    modelPlan: RunModelPlanDto | null,
  ): Promise<string> {
    try {
      const runId = await threadCompactContext(
        threadId,
        instructions,
        modelPlan,
        (event) => {
          if (this.disposed) {
            return;
          }
          this.handleEvent(event);
        },
      );

      if (this.disposed) {
        return runId;
      }

      this.currentRunId = runId;
      return runId;
    } catch (error) {
      const message = formatInvokeErrorMessage(error) ?? "Unknown error";
      if (!this.disposed) {
        this.onError?.(message, "");
      }
      throw error;
    }
  }

  /**
   * Respond to a tool approval request.
   */
  async respondToApproval(
    toolCallId: string,
    runId: string,
    approved: boolean,
  ): Promise<void> {
    try {
      await toolApprovalRespond(toolCallId, runId, approved);
    } catch (error) {
      const message = formatInvokeErrorMessage(error) ?? "Unknown error";
      this.onError?.(message, runId);
      throw error;
    }
  }

  async respondToClarify(
    toolCallId: string,
    response: unknown,
  ): Promise<void> {
    try {
      await toolClarifyRespond(toolCallId, response);
    } catch (error) {
      const message = formatInvokeErrorMessage(error) ?? "Unknown error";
      this.onError?.(message, this.currentRunId ?? "");
      throw error;
    }
  }

  /**
   * Reset stream state (e.g. when switching threads).
   */
  reset() {
    this.currentRunId = null;
    this.hiddenToolCallIds.clear();
    this.toolNameCache.clear();
  }

  /**
   * Permanently stop delivering events to this stream instance.
   */
  dispose() {
    this.disposed = true;
    this.reset();
    this.onMessage = null;
    this.onToolEvent = null;
    this.onApproval = null;
    this.onRunStateChange = null;
    this.onContextCompressing = null;
    this.onPlan = null;
    this.onReasoning = null;
    this.onQueue = null;
    this.onHelperEvent = null;
    this.onThreadTitle = null;
    this.onUsage = null;
    this.onError = null;
    this.onRawEvent = null;
  }

  // -----------------------------------------------------------------------
  // Event routing — maps ThreadStreamEvent to typed callbacks
  // -----------------------------------------------------------------------

  private handleEvent(event: ThreadStreamEvent) {
    if (this.disposed) {
      return;
    }

    // Forward raw event if anyone is listening
    this.onRawEvent?.(event);

    switch (event.type) {
      case "run_started":
        this.currentRunId = event.runId;
        this.onRunStateChange?.("running", event.runId);
        break;

      case "stream_resync_required":
        break;

      case "run_retrying":
        this.currentRunId = event.runId;
        this.onRunStateChange?.("running", event.runId);
        break;

      case "message_delta":
        this.onMessage?.({
          kind: "delta",
          messageId: event.messageId,
          runId: event.runId,
          delta: event.delta,
        });
        break;

      case "message_completed":
        this.onMessage?.({
          kind: "completed",
          messageId: event.messageId,
          runId: event.runId,
          content: event.content,
        });
        break;

      case "message_discarded":
        break;

      case "plan_updated":
        this.onPlan?.({ runId: event.runId, plan: event.plan });
        break;

      case "reasoning_updated":
        this.onReasoning?.({
          runId: event.runId,
          messageId: event.messageId,
          reasoning: event.reasoning,
        });
        break;

      case "queue_updated":
        this.onQueue?.({
          runId: event.runId,
          queue: event.queue,
        });
        break;

      case "task_board_updated":
        this.onTaskBoard?.({
          taskBoard: event.taskBoard,
        });
        break;

      case "subagent_started":
        this.onHelperEvent?.({
          kind: "started",
          runId: event.runId,
          subtaskId: event.subtaskId,
          helperKind: event.helperKind,
          startedAt: event.startedAt,
          snapshot: event.snapshot,
        });
        break;

      case "subagent_progress":
        this.onHelperEvent?.({
          kind: "progress",
          runId: event.runId,
          subtaskId: event.subtaskId,
          helperKind: event.helperKind,
          startedAt: event.startedAt,
          activity: event.activity,
          message: event.message,
          snapshot: event.snapshot,
        });
        break;

      case "subagent_completed":
        this.onHelperEvent?.({
          kind: "completed",
          runId: event.runId,
          subtaskId: event.subtaskId,
          helperKind: event.helperKind,
          startedAt: event.startedAt,
          summary: event.summary,
          snapshot: event.snapshot,
        });
        break;

      case "subagent_failed":
        this.onHelperEvent?.({
          kind: "failed",
          runId: event.runId,
          subtaskId: event.subtaskId,
          helperKind: event.helperKind,
          startedAt: event.startedAt,
          error: event.error,
          snapshot: event.snapshot,
        });
        break;

      case "tool_requested":
        this.toolNameCache.set(event.toolCallId, event.toolName);
        if (isRuntimeOrchestrationToolName(event.toolName)) {
          this.hiddenToolCallIds.add(event.toolCallId);
          break;
        }
        this.onToolEvent?.({
          kind: "requested",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: event.toolName,
          toolInput: event.toolInput,
        });
        break;

      case "approval_required":
        this.toolNameCache.set(event.toolCallId, event.toolName);
        this.onRunStateChange?.("waiting_approval", event.runId);
        this.onApproval?.({
          kind: "required",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: event.toolName,
          toolInput: event.toolInput,
          reason: event.reason,
        });
        break;

      case "clarify_required":
        this.toolNameCache.set(event.toolCallId, event.toolName);
        this.onRunStateChange?.("needs_reply", event.runId);
        this.onToolEvent?.({
          kind: "clarify-required",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: event.toolName,
          toolInput: event.toolInput,
        });
        break;

      case "approval_resolved":
        this.onRunStateChange?.("running", event.runId);
        this.onApproval?.({
          kind: "resolved",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: this.toolNameCache.get(event.toolCallId),
          approved: event.approved,
        });
        break;

      case "clarify_resolved":
        this.onRunStateChange?.("running", event.runId);
        this.onToolEvent?.({
          kind: "clarify-resolved",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: this.toolNameCache.get(event.toolCallId),
          response: event.response,
          result: event.response,
        });
        break;

      case "tool_running":
        if (this.hiddenToolCallIds.has(event.toolCallId)) {
          break;
        }
        this.onToolEvent?.({
          kind: "running",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: this.toolNameCache.get(event.toolCallId),
        });
        break;

      case "tool_completed":
        if (this.hiddenToolCallIds.has(event.toolCallId)) {
          this.hiddenToolCallIds.delete(event.toolCallId);
          this.toolNameCache.delete(event.toolCallId);
          break;
        }
        this.onToolEvent?.({
          kind: "completed",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: this.toolNameCache.get(event.toolCallId),
          result: event.result,
        });
        this.toolNameCache.delete(event.toolCallId);
        break;

      case "tool_failed":
        if (this.hiddenToolCallIds.has(event.toolCallId)) {
          this.hiddenToolCallIds.delete(event.toolCallId);
          this.toolNameCache.delete(event.toolCallId);
          break;
        }
        this.onToolEvent?.({
          kind: "failed",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: this.toolNameCache.get(event.toolCallId),
          error: event.error,
        });
        this.toolNameCache.delete(event.toolCallId);
        break;

      case "thread_title_updated":
        this.onThreadTitle?.({
          runId: event.runId,
          threadId: event.threadId,
          title: event.title,
        });
        break;

      case "thread_usage_updated":
        this.onUsage?.({
          runId: event.runId,
          modelDisplayName: event.modelDisplayName,
          contextWindow: event.contextWindow,
          usage: event.usage,
        });
        break;

      case "run_checkpointed":
        this.currentRunId = null;
        this.onRunStateChange?.("waiting_approval", event.runId);
        break;

      case "context_compressing":
        // Context compression in progress — frontend shows placeholder
        this.onContextCompressing?.(event.runId);
        break;

      case "run_completed":
        this.currentRunId = null;
        this.onRunStateChange?.("completed", event.runId);
        break;

      case "run_limit_reached":
        this.currentRunId = null;
        this.onRunStateChange?.("limit_reached", event.runId);
        this.onError?.(event.error, event.runId);
        break;

      case "run_failed":
        this.currentRunId = null;
        this.onRunStateChange?.("failed", event.runId);
        this.onError?.(event.error, event.runId);
        break;

      case "run_cancelled":
        this.currentRunId = null;
        this.onRunStateChange?.("cancelled", event.runId);
        break;

      case "run_interrupted":
        this.currentRunId = null;
        this.onRunStateChange?.("interrupted", event.runId);
        break;
    }
  }
}

function isRuntimeOrchestrationToolName(toolName: string) {
  return (
    toolName === "agent_explore"
    || toolName === "agent_review"
  );
}
