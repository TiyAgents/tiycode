import type { Machine } from "@/shared/lib/create-machine";
import type {
  RunMachineContext,
  RunMachineEvent,
  RunMachineState,
} from "./run-lifecycle-machine";
import { setThreadStatus } from "./thread-store";
import {
  backendStatusToMachineEvent,
  backendToThreadRunStatus,
  machineEventToThreadRunStatus,
} from "./status-mappings";

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
    const status = machineEventToThreadRunStatus(event);
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
    const event = backendStatusToMachineEvent(backendStatus);
    if (event) {
      machine.send(event, { runId, message: undefined });
    }
  } else {
    setThreadStatus(
      threadId,
      backendToThreadRunStatus(backendStatus),
      { runId, source: "tauri_event" },
    );
  }
}

/**
 * Dispatch a unified `thread-run-status-changed` event.
 *
 * This covers all sidebar-visible status transitions — including intermediate
 * states like `waiting_approval` and `needs_reply` — so background threads
 * without an active `RuntimeThreadSurface` can update their sidebar indicator.
 *
 * If the thread has an active machine, the status is mapped to a machine event
 * and routed there; otherwise it writes directly to `threadStore`.
 */
export function dispatchRunStatusChangedEvent(
  threadId: string,
  runId: string,
  backendStatus: string,
): void {
  const machine = activeMachines.get(threadId);
  if (machine) {
    const event = backendStatusToMachineEvent(backendStatus);
    if (event) {
      machine.send(event, { runId, message: undefined });
    }
  } else {
    setThreadStatus(
      threadId,
      backendToThreadRunStatus(backendStatus),
      { runId, source: "tauri_event" },
    );
  }
}
