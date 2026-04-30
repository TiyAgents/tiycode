import { createStore, useStore as useStoreBase, shallowEqual } from "@/shared/lib/create-store";
import type { RunMode } from "@/shared/types/api";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ComposerStoreState {
  [key: string]: unknown;
  /** Input value for new-thread mode. */
  newThreadValue: string;
  /** Run mode for new threads (default / plan). */
  newThreadRunMode: RunMode;
  /** Per-thread drafts keyed by thread ID. */
  drafts: Record<string, string>;
  /** Composer-level error message. */
  error: string | null;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const composerStore = createStore<ComposerStoreState>({
  newThreadValue: "",
  newThreadRunMode: "default",
  drafts: {},
  error: null,
});

// ---------------------------------------------------------------------------
// React hook (re-export for convenience)
// ---------------------------------------------------------------------------

export { useStoreBase as useStore, shallowEqual };

// ---------------------------------------------------------------------------
// Actions — New thread composer
// ---------------------------------------------------------------------------

export function setNewThreadValue(value: string): void {
  composerStore.setState({ newThreadValue: value });
}

export function setNewThreadRunMode(mode: RunMode): void {
  composerStore.setState({ newThreadRunMode: mode });
}

/** Clear new-thread composer state (used after submission or thread switch). */
export function clearNewThreadComposer(): void {
  composerStore.setState({
    newThreadValue: "",
    error: null,
  });
}

// ---------------------------------------------------------------------------
// Actions — Drafts
// ---------------------------------------------------------------------------

export function setDraft(threadId: string, value: string): void {
  composerStore.setState((prev) => ({
    drafts: { ...prev.drafts, [threadId]: value },
  }));
}

export function getDraft(threadId: string): string {
  return composerStore.getState().drafts[threadId] ?? "";
}

export function removeDraft(threadId: string): void {
  composerStore.setState((prev) => {
    if (!(threadId in prev.drafts)) {
      return {};
    }
    const next = { ...prev.drafts };
    delete next[threadId];
    return { drafts: next };
  });
}

// ---------------------------------------------------------------------------
// Actions — Error
// ---------------------------------------------------------------------------

export function setComposerError(error: string | null): void {
  composerStore.setState({ error });
}

export function clearComposerError(): void {
  composerStore.setState({ error: null });
}
