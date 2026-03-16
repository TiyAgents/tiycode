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

import { threadStartRun, threadCancelRun, toolApprovalRespond } from "@/services/bridge";
import type { ThreadStreamEvent } from "./types";

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
  kind: "requested" | "running" | "completed" | "failed";
  runId: string;
  toolCallId: string;
  toolName?: string;
  toolInput?: unknown;
  result?: unknown;
  error?: string;
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

export type RunState = "idle" | "running" | "waiting_approval" | "completed" | "failed" | "interrupted";

export type PlanEvent = {
  runId: string;
  plan: unknown;
};

export type ReasoningEvent = {
  runId: string;
  reasoning: string;
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
  onPlan: ((event: PlanEvent) => void) | null = null;
  onReasoning: ((event: ReasoningEvent) => void) | null = null;
  onError: ((error: string, runId: string) => void) | null = null;
  onRawEvent: ((event: ThreadStreamEvent) => void) | null = null;

  private currentRunId: string | null = null;

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
    prompt: string,
    runMode?: string,
  ): Promise<string> {
    const runId = await threadStartRun(threadId, prompt, (event) => {
      this.handleEvent(event);
    }, runMode);

    this.currentRunId = runId;
    return runId;
  }

  /**
   * Cancel the currently active run.
   */
  async cancelRun(threadId: string): Promise<void> {
    await threadCancelRun(threadId);
  }

  /**
   * Respond to a tool approval request.
   */
  async respondToApproval(
    toolCallId: string,
    runId: string,
    approved: boolean,
  ): Promise<void> {
    await toolApprovalRespond(toolCallId, runId, approved);
  }

  /**
   * Reset stream state (e.g. when switching threads).
   */
  reset() {
    this.currentRunId = null;
  }

  // -----------------------------------------------------------------------
  // Event routing — maps ThreadStreamEvent to typed callbacks
  // -----------------------------------------------------------------------

  private handleEvent(event: ThreadStreamEvent) {
    // Forward raw event if anyone is listening
    this.onRawEvent?.(event);

    switch (event.type) {
      case "run_started":
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

      case "plan_updated":
        this.onPlan?.({ runId: event.runId, plan: event.plan });
        break;

      case "reasoning_updated":
        this.onReasoning?.({
          runId: event.runId,
          reasoning: event.reasoning,
        });
        break;

      case "tool_requested":
        this.onToolEvent?.({
          kind: "requested",
          runId: event.runId,
          toolCallId: event.toolCallId,
          toolName: event.toolName,
          toolInput: event.toolInput,
        });
        break;

      case "approval_required":
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

      case "approval_resolved":
        this.onRunStateChange?.("running", event.runId);
        this.onApproval?.({
          kind: "resolved",
          runId: event.runId,
          toolCallId: event.toolCallId,
          approved: event.approved,
        });
        break;

      case "tool_running":
        this.onToolEvent?.({
          kind: "running",
          runId: event.runId,
          toolCallId: event.toolCallId,
        });
        break;

      case "tool_completed":
        this.onToolEvent?.({
          kind: "completed",
          runId: event.runId,
          toolCallId: event.toolCallId,
          result: event.result,
        });
        break;

      case "tool_failed":
        this.onToolEvent?.({
          kind: "failed",
          runId: event.runId,
          toolCallId: event.toolCallId,
          error: event.error,
        });
        break;

      case "run_completed":
        this.currentRunId = null;
        this.onRunStateChange?.("completed", event.runId);
        break;

      case "run_failed":
        this.currentRunId = null;
        this.onRunStateChange?.("failed", event.runId);
        this.onError?.(event.error, event.runId);
        break;

      case "run_interrupted":
        this.currentRunId = null;
        this.onRunStateChange?.("interrupted", event.runId);
        break;
    }
  }
}
