/**
 * Delete confirmation discriminated union.
 *
 * Replaces the two-useState pattern (pendingDeleteThreadId + deletingThreadId)
 * with a single discriminated union that TypeScript can narrow automatically.
 */
export type DeletePhase =
  | { kind: "idle" }
  | { kind: "confirming"; threadId: string }
  | { kind: "deleting"; threadId: string };
