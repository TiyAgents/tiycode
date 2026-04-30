import type { Machine } from "@/shared/lib/create-machine";
import type {
  RunMachineContext,
  RunMachineEvent,
  RunMachineState,
} from "./run-lifecycle-machine";
import type { ThreadRunStatus } from "./types";
import { setThreadStatus } from "./thread-store";

// ---------------------------------------------------------------------------
// Module-level registry
// ---------------------------------------------------------------------------

/**
 * Registry of currently-active run-lifecycle machines, keyed by thread ID.
 * A machine is registered when a `RuntimeThreadSurface` mounts and
 * unregistered when it unmounts.
 */
const activeMachines = new Map<
  string,
  Machine<RunMachineState, RunMachineEvent, RunMachineContext>
>();

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Register an active run-lifecycle machine so that global Tauri events
 * can be routed to it.
 */
export function registerRunMachine(
  threadId: string,
  machine: Machine<RunMachineState, RunMachineEvent, RunMachineContext>,
): void {
  activeMachines.set(threadId, machine);
}

/**
 * Unregister a run-lifecycle machine (call on component unmount).
 */
export function unregisterRunMachine(threadId: string): void {
  activeMachines.delete(threadId);
}

/**
 * Dispatch a global Tauri event to the appropriate run-lifecycle machine.
 *
 * - If the thread has an active machine, the event is sent to it.
 * - If there is no active machine (e.g. background thread without a
 *   rendered `RuntimeThreadSurface`), the event falls back to writing
 *   directly to `threadStore` so the sidebar stays in sync.
 */
export function dispatchGlobalEvent(
  threadId: string,
  event: RunMachineEvent,
  payload?: { runId?: string; message?: string; newRunId?: string },
): void {
  const machine = activeMachines.get(threadId);
  if (machine) {
    machine.send(event, payload);
  } else {
    // Fallback: no active surface, write directly to threadStore
    const status = mapMachineEventToStatus(event);
    setThreadStatus(threadId, status, {
      runId: payload?.runId ?? null,
      source: "tauri_event",
    });
  }
}

/**
 * Convenience wrapper for thread-run-finished Tauri events that need to
 * map the backend status string to a ThreadRunStatus first.
 */
export function dispatchRunFinishedEvent(
  threadId: string,
  runId: string,
  backendStatus: string,
): void {
  const machine = activeMachines.get(threadId);
  if (machine) {
    const event = mapBackendFinishedStatusToMachineEvent(backendStatus);
    machine.send(event, { runId, message: undefined });
  } else {
    setThreadStatus(
      threadId,
      backendStatusToThreadRunStatus(backendStatus),
      { runId, source: "tauri_event" },
    );
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function mapMachineEventToStatus(event: RunMachineEvent): ThreadRunStatus {
  switch (event) {
    case "RUN_STARTED":
    case "APPROVAL_RESOLVED":
    case "CLARIFY_RESOLVED":
    case "RUN_RETRYING":
      return "running";
    case "RUN_COMPLETED":
      return "completed";
    case "RUN_FAILED":
      return "failed";
    case "RUN_CANCELLED":
      return "cancelled";
    case "RUN_INTERRUPTED":
      return "interrupted";
    case "LIMIT_REACHED":
      return "limit_reached";
    case "APPROVAL_REQUIRED":
      return "waiting_approval";
    case "CLARIFY_REQUIRED":
      return "needs_reply";
  }
}

function mapBackendFinishedStatusToMachineEvent(
  status: string,
): RunMachineEvent {
  switch (status) {
    case "failed":
      return "RUN_FAILED";
    case "interrupted":
      return "RUN_INTERRUPTED";
    case "cancelled":
      return "RUN_CANCELLED";
    case "limit_reached":
      return "LIMIT_REACHED";
    default:
      return "RUN_COMPLETED";
  }
}

function backendStatusToThreadRunStatus(
  status: string,
): ThreadRunStatus {
  switch (status) {
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
    case "cancelled":
      return "cancelled";
    case "limit_reached":
      return "limit_reached";
    default:
      return "completed";
  }
}
