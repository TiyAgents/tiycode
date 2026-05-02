import type { Machine } from "@/shared/lib/create-machine";
import { createMachine } from "@/shared/lib/create-machine";
import { setThreadStatus } from "./thread-store";
import type { ThreadRunStatus } from "./types";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Valid states for an individual thread's run lifecycle. */
export type RunMachineState =
  | "idle"
  | "running"
  | "waiting_approval"
  | "needs_reply"
  | "completed"
  | "failed"
  | "cancelled"
  | "interrupted"
  | "limit_reached";

/** Events that drive run-lifecycle state transitions. */
export type RunMachineEvent =
  | "RUN_STARTED"
  | "APPROVAL_REQUIRED"
  | "CLARIFY_REQUIRED"
  | "APPROVAL_RESOLVED"
  | "CLARIFY_RESOLVED"
  | "RUN_RETRYING"
  | "RUN_COMPLETED"
  | "RUN_FAILED"
  | "RUN_CANCELLED"
  | "RUN_INTERRUPTED"
  | "LIMIT_REACHED";

/** Context data carried alongside the run-lifecycle state. */
export interface RunMachineContext {
  runId: string | null;
  errorMessage: string | null;
  retryCount: number;
}

/** Payload shape for run-machine events. */
export interface RunMachinePayload {
  runId?: string | null;
  message?: string;
  newRunId?: string;
}

// ---------------------------------------------------------------------------
// Machine factory
// ---------------------------------------------------------------------------

/**
 * Create a per-thread run-lifecycle state machine.
 *
 * The machine is the authoritative source for the thread's run status.
 * Every state change is automatically synchronised to `threadStore` via
 * the `subscribe` callback, so sidebar and other consumers stay in sync.
 *
 * Thread-stream events drive the machine via {@link mapStreamEventToMachineEvent}
 * + `machine.send()`.  Snapshot restores use `machine.reset()`.
 */
export function createRunLifecycleMachine(
  threadId: string,
): Machine<RunMachineState, RunMachineEvent, RunMachineContext> {
  const machine = createMachine<
    RunMachineState,
    RunMachineEvent,
    RunMachineContext
  >({
    initial: "idle",
    context: { runId: null, errorMessage: null, retryCount: 0 },
    states: {
      idle: {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return {
                ...ctx,
                runId: p?.runId ?? null,
                retryCount: 0,
                errorMessage: null,
              };
            },
          },
        },
      },
      running: {
        on: {
          APPROVAL_REQUIRED: "waiting_approval",
          CLARIFY_REQUIRED: "needs_reply",
          RUN_RETRYING: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return {
                ...ctx,
                runId: p?.newRunId ?? ctx.runId,
                retryCount: ctx.retryCount + 1,
              };
            },
          },
          RUN_COMPLETED: "completed",
          RUN_FAILED: {
            target: "failed",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, errorMessage: p?.message ?? null };
            },
          },
          RUN_CANCELLED: "cancelled",
          RUN_INTERRUPTED: "interrupted",
          LIMIT_REACHED: "limit_reached",
        },
      },
      waiting_approval: {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, runId: p?.runId ?? null, retryCount: 0, errorMessage: null };
            },
          },
          APPROVAL_RESOLVED: "running",
          RUN_COMPLETED: "completed",
          RUN_FAILED: {
            target: "failed",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, errorMessage: p?.message ?? null };
            },
          },
          RUN_CANCELLED: "cancelled",
          RUN_INTERRUPTED: "interrupted",
          LIMIT_REACHED: "limit_reached",
        },
      },
      needs_reply: {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, runId: p?.runId ?? null, retryCount: 0, errorMessage: null };
            },
          },
          CLARIFY_RESOLVED: "running",
          RUN_COMPLETED: "completed",
          RUN_FAILED: {
            target: "failed",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, errorMessage: p?.message ?? null };
            },
          },
          RUN_CANCELLED: "cancelled",
          RUN_INTERRUPTED: "interrupted",
          LIMIT_REACHED: "limit_reached",
        },
      },
      completed:   {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, runId: p?.runId ?? null, retryCount: 0, errorMessage: null };
            },
          },
        },
      },
      failed:      {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, runId: p?.runId ?? null, retryCount: 0, errorMessage: null };
            },
          },
        },
      },
      cancelled:   {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, runId: p?.runId ?? null, retryCount: 0, errorMessage: null };
            },
          },
        },
      },
      interrupted: {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, runId: p?.runId ?? null, retryCount: 0, errorMessage: null };
            },
          },
        },
      },
      limit_reached: {
        on: {
          RUN_STARTED: {
            target: "running",
            action: (ctx, payload) => {
              const p = payload as RunMachinePayload | undefined;
              return { ...ctx, runId: p?.runId ?? null, retryCount: 0, errorMessage: null };
            },
          },
        },
      },
    },
  });

  // Auto-sync every state or context change to the threadStore.
  machine.subscribe(() => {
    if (!threadId) return;
    const currentState = machine.getState();
    const runId = machine.getContext().runId;
    setThreadStatus(threadId, currentState as ThreadRunStatus, {
      runId,
      source: "stream",
    });
  });

  return machine;
}

// ---------------------------------------------------------------------------
// Stream event mapping
// ---------------------------------------------------------------------------

/**
 * Map a raw ThreadStream event type string to the corresponding
 * run-lifecycle machine event.  Returns `null` when the event is
 * not relevant to run lifecycle state (e.g. message deltas, tool events).
 */
export function mapStreamEventToMachineEvent(
  eventType: string,
): RunMachineEvent | null {
  const mapping: Record<string, RunMachineEvent> = {
    run_started: "RUN_STARTED",
    approval_required: "APPROVAL_REQUIRED",
    clarify_required: "CLARIFY_REQUIRED",
    approval_resolved: "APPROVAL_RESOLVED",
    clarify_resolved: "CLARIFY_RESOLVED",
    run_checkpointed: "APPROVAL_REQUIRED", // plan checkpoint ≡ approval
    run_retrying: "RUN_RETRYING",
    run_completed: "RUN_COMPLETED",
    run_failed: "RUN_FAILED",
    run_cancelled: "RUN_CANCELLED",
    run_interrupted: "RUN_INTERRUPTED",
    run_limit_reached: "LIMIT_REACHED",
  };
  return mapping[eventType] ?? null;
}
