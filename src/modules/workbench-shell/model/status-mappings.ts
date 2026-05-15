/**
 * Unified status mapping module.
 *
 * STATUS ARCHITECTURE (read top-to-bottom = data flow direction):
 *
 * Layer 1: BackendRunStatus (13 values) — wire format from Tauri IPC
 * Layer 2: RunMachineEvent             — FSM events in run-lifecycle-machine
 * Layer 3: ThreadRunStatus (9 values)  — store-level thread status
 * Layer 4: ThreadStatus (5 values)     — sidebar display
 *
 * Each layer boundary has exactly ONE mapping function:
 *   Layer 1 → Layer 2: backendStatusToMachineEvent()
 *   Layer 1 → Layer 3: backendStatusToThreadRunStatus()
 *   Layer 2 → Layer 3: machineEventToThreadRunStatus()
 *   Layer 3 → Layer 4: threadRunStatusToDisplayStatus()
 *
 * Stream events (string names) also map into Layer 2:
 *   streamEventToMachineEvent()
 */

import type { RunMachineEvent } from "./run-lifecycle-machine";
import type { ThreadRunStatus, ThreadStatus } from "./types";

// ---------------------------------------------------------------------------
// Layer 1 → Layer 2: Backend run status → Machine event
// ---------------------------------------------------------------------------

/**
 * Map a backend run status string (from Tauri global events or snapshot DTOs)
 * to the corresponding run-lifecycle machine event.
 *
 * Returns `null` for statuses that don't map to a machine event (shouldn't
 * happen for known statuses, but guards against unknown future values).
 *
 * Replaces: `mapBackendStatusToMachineEvent`, `mapBackendFinishedStatusToMachineEvent`
 */
export function backendStatusToMachineEvent(
  status: string,
): RunMachineEvent | null {
  switch (status) {
    // Active states → machine should be in "running"
    case "running":
    case "created":
    case "dispatching":
    case "waiting_tool_result":
    case "cancelling": // cancelling is still active — aligned with backend RunStatus::to_thread_status()
      return "RUN_STARTED";
    // User-action states
    case "waiting_approval":
      return "APPROVAL_REQUIRED";
    case "needs_reply":
      return "CLARIFY_REQUIRED";
    // Terminal states
    case "completed":
      return "RUN_COMPLETED";
    case "failed":
    case "denied":
      return "RUN_FAILED";
    case "cancelled":
      return "RUN_CANCELLED";
    case "interrupted":
      return "RUN_INTERRUPTED";
    case "limit_reached":
      return "LIMIT_REACHED";
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// Stream event → Layer 2: Stream event name → Machine event
// ---------------------------------------------------------------------------

/**
 * Map a raw ThreadStream event type string to the corresponding
 * run-lifecycle machine event.  Returns `null` when the event is
 * not relevant to run lifecycle state (e.g. message deltas, tool events).
 *
 * Moved from: `run-lifecycle-machine.ts::mapStreamEventToMachineEvent`
 */
export function streamEventToMachineEvent(
  eventType: string,
): RunMachineEvent | null {
  switch (eventType) {
    case "run_started":
      return "RUN_STARTED";
    case "approval_required":
    case "run_checkpointed": // plan checkpoint ≡ approval
      return "APPROVAL_REQUIRED";
    case "clarify_required":
      return "CLARIFY_REQUIRED";
    case "approval_resolved":
      return "APPROVAL_RESOLVED";
    case "clarify_resolved":
      return "CLARIFY_RESOLVED";
    case "run_retrying":
      return "RUN_RETRYING";
    case "run_completed":
      return "RUN_COMPLETED";
    case "run_failed":
      return "RUN_FAILED";
    case "run_cancelled":
      return "RUN_CANCELLED";
    case "run_interrupted":
      return "RUN_INTERRUPTED";
    case "run_limit_reached":
      return "LIMIT_REACHED";
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// Layer 2 → Layer 3: Machine event → ThreadRunStatus
// ---------------------------------------------------------------------------

/**
 * Map a machine event to the ThreadRunStatus it implies.
 * Used by the dispatcher's fallback path (no active machine → direct store write).
 *
 * Replaces: `mapMachineEventToStatus`
 */
export function machineEventToThreadRunStatus(
  event: RunMachineEvent,
): ThreadRunStatus {
  switch (event) {
    case "RUN_STARTED":
    case "APPROVAL_RESOLVED":
    case "CLARIFY_RESOLVED":
    case "RUN_RETRYING":
      return "running";
    case "APPROVAL_REQUIRED":
      return "waiting_approval";
    case "CLARIFY_REQUIRED":
      return "needs_reply";
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
  }
}

// ---------------------------------------------------------------------------
// Layer 1 → Layer 3: Backend run status → ThreadRunStatus (direct)
// ---------------------------------------------------------------------------

/**
 * Map a backend status string directly to a ThreadRunStatus.
 * Used by the dispatcher's fallback path for background threads.
 *
 * Replaces: `backendStatusToThreadRunStatus`
 */
export function backendToThreadRunStatus(
  status: string,
): ThreadRunStatus {
  switch (status) {
    case "running":
    case "created":
    case "dispatching":
    case "waiting_tool_result":
    case "cancelling":
      return "running";
    case "waiting_approval":
      return "waiting_approval";
    case "needs_reply":
      return "needs_reply";
    case "completed":
      return "completed";
    case "failed":
    case "denied":
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

// ---------------------------------------------------------------------------
// Layer 3 → Layer 4: ThreadRunStatus → Display status
// ---------------------------------------------------------------------------

/**
 * Map a {@link ThreadRunStatus} value to the subset used for sidebar display
 * (matching the legacy {@link ThreadStatus} union).
 *
 * Moved from: `types.ts::threadRunStatusToDisplayStatus`
 */
export function threadRunStatusToDisplayStatus(
  status: ThreadRunStatus,
): ThreadStatus {
  switch (status) {
    case "idle":
    case "completed":
    case "cancelled":
      return "completed";
    case "waiting_approval":
    case "needs_reply":
    case "limit_reached":
      return "needs-reply";
    case "running":
      return "running";
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
  }
}
