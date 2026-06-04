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
  /**
   * Wall-clock start time (Unix ms) of the current run. Mirrors the
   * `startedAtMs` field on `ThreadStreamEvent::RunStarted` and is
   * propagated to `ThreadStatusRecord.startedAtMs` for metadata /
   * debugging.  The workbench header elapsed timer is driven by a
   * separate frontend `TimerSlot` in `use-thread-elapsed-timer.ts`
   * that only accumulates active running time.
   */
  startedAtMs: number | null;
  errorMessage: string | null;
  retryCount: number;
}

/** Payload shape for run-machine events. */
export interface RunMachinePayload {
  runId?: string | null;
  /** Wall-clock start time (Unix ms) attached to a `RUN_STARTED` event. */
  startedAtMs?: number | null;
  message?: string;
  newRunId?: string;
}

// ---------------------------------------------------------------------------
// Shared transitions — DRY helpers for actions reused across multiple states
// ---------------------------------------------------------------------------

/** Transition for RUN_STARTED: reused in idle, waiting_approval, needs_reply, and all 5 terminal states. */
const RUN_STARTED_TRANSITION = {
  target: "running" as const,
  action: (ctx: RunMachineContext, payload?: unknown): RunMachineContext => {
    const p = payload as RunMachinePayload | undefined;
    return {
      ...ctx,
      runId: p?.runId ?? null,
      startedAtMs: p?.startedAtMs ?? null,
      retryCount: 0,
      errorMessage: null,
    };
  },
};

/** Transition for RUN_FAILED: reused in running, waiting_approval, and needs_reply. */
const RUN_FAILED_TRANSITION = {
  target: "failed" as const,
  action: (ctx: RunMachineContext, payload?: unknown): RunMachineContext => {
    const p = payload as RunMachinePayload | undefined;
    return { ...ctx, errorMessage: p?.message ?? null };
  },
};

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
    context: { runId: null, startedAtMs: null, errorMessage: null, retryCount: 0 },
    states: {
      idle: {
        on: {
          RUN_STARTED: RUN_STARTED_TRANSITION,
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
          RUN_FAILED: RUN_FAILED_TRANSITION,
          RUN_CANCELLED: "cancelled",
          RUN_INTERRUPTED: "interrupted",
          LIMIT_REACHED: "limit_reached",
        },
      },
      waiting_approval: {
        on: {
          RUN_STARTED: RUN_STARTED_TRANSITION,
          APPROVAL_RESOLVED: "running",
          RUN_COMPLETED: "completed",
          RUN_FAILED: RUN_FAILED_TRANSITION,
          RUN_CANCELLED: "cancelled",
          RUN_INTERRUPTED: "interrupted",
          LIMIT_REACHED: "limit_reached",
        },
      },
      needs_reply: {
        on: {
          RUN_STARTED: RUN_STARTED_TRANSITION,
          CLARIFY_RESOLVED: "running",
          RUN_COMPLETED: "completed",
          RUN_FAILED: RUN_FAILED_TRANSITION,
          RUN_CANCELLED: "cancelled",
          RUN_INTERRUPTED: "interrupted",
          LIMIT_REACHED: "limit_reached",
        },
      },
      completed:     { on: { RUN_STARTED: RUN_STARTED_TRANSITION } },
      failed:        { on: { RUN_STARTED: RUN_STARTED_TRANSITION } },
      cancelled:     { on: { RUN_STARTED: RUN_STARTED_TRANSITION } },
      interrupted:   { on: { RUN_STARTED: RUN_STARTED_TRANSITION } },
      limit_reached: { on: { RUN_STARTED: RUN_STARTED_TRANSITION } },
    },
  });

  // Auto-sync every state or context change to the threadStore.
  machine.subscribe(() => {
    if (!threadId) return;
    const currentState = machine.getState();
    const context = machine.getContext();
    setThreadStatus(threadId, currentState as ThreadRunStatus, {
      runId: context.runId,
      startedAtMs: context.startedAtMs,
      source: "stream",
    });
  });

  return machine;
}

// ---------------------------------------------------------------------------
// Stream event mapping — re-exported from the unified status-mappings module
// ---------------------------------------------------------------------------

export { streamEventToMachineEvent as mapStreamEventToMachineEvent } from "./status-mappings";
