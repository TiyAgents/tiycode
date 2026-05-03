import { createStore, useStore as useStoreBase, shallowEqual } from "@/shared/lib/create-store";
import type { RunMode } from "@/shared/types/api";
import type { ComposerReferencedFile } from "@/modules/workbench-shell/model/composer-commands";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Serializable attachment metadata stored in composer drafts. */
export interface SerializableAttachment {
  id: string;
  name: string;
  mediaType: string;
  /** Base64 data URL of the file content (blob URLs are ephemeral). */
  dataUrl: string;
}

/** Structured draft data keyed by thread ID. */
export interface ComposerDraftData {
  text: string;
  referencedFiles: ComposerReferencedFile[];
}

export interface ComposerStoreState {
  // Keep index-signature for store compatibility; it must cover all value types.
  [key: string]: unknown;
  /** Input value for new-thread mode. */
  newThreadValue: string;
  /** Run mode for new threads (default / plan). */
  newThreadRunMode: RunMode;
  /** @file references for new-thread composer. */
  newThreadReferencedFiles: ComposerReferencedFile[];
  /** Serialized attachment data for new-thread composer. */
  newThreadAttachmentData: SerializableAttachment[];
  /** Per-thread drafts keyed by thread ID. */
  drafts: Record<string, ComposerDraftData>;
  /** Composer-level error message. */
  error: string | null;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const composerStore = createStore<ComposerStoreState>({
  newThreadValue: "",
  newThreadRunMode: "default",
  newThreadReferencedFiles: [],
  newThreadAttachmentData: [],
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

export function setNewThreadReferencedFiles(files: ReadonlyArray<ComposerReferencedFile>): void {
  composerStore.setState({ newThreadReferencedFiles: files as ComposerReferencedFile[] });
}

export function setNewThreadAttachmentData(data: ReadonlyArray<SerializableAttachment>): void {
  composerStore.setState({ newThreadAttachmentData: data as SerializableAttachment[] });
}

/** Clear new-thread composer state (used after submission or thread switch). */
export function clearNewThreadComposer(): void {
  composerStore.setState({
    newThreadValue: "",
    newThreadReferencedFiles: [],
    newThreadAttachmentData: [],
    error: null,
  });
}

// ---------------------------------------------------------------------------
// Actions — Drafts
// ---------------------------------------------------------------------------

export function setDraft(threadId: string, data: ComposerDraftData): void {
  composerStore.setState((prev) => ({
    drafts: { ...prev.drafts, [threadId]: data },
  }));
}

/**
 * Read the draft for a thread.
 * Compatible with legacy string drafts: if the stored value is a plain string
 * (from older code), wraps it into a ComposerDraftData with empty referencedFiles.
 */
export function getDraft(threadId: string): ComposerDraftData {
  const raw = composerStore.getState().drafts[threadId];
  if (!raw) {
    return { text: "", referencedFiles: [] };
  }
  // Backward compat: old drafts were plain strings.
  if (typeof raw === "string") {
    return { text: raw, referencedFiles: [] };
  }
  return raw as ComposerDraftData;
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
