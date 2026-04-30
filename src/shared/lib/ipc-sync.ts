import type { Store } from "./create-store";
import { formatInvokeErrorMessage } from "./invoke-error";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type DedupeStrategy =
  | "none"
  | "first"
  | "last"
  | { key: string; strategy: "first" | "last" };

export class SyncError extends Error {
  public override readonly name = "SyncError";

  constructor(
    public override readonly message: string,
    public readonly raw: unknown,
    public readonly aborted: boolean = false,
  ) {
    super(message);
  }
}

export interface SyncOptions<S extends Record<string, unknown>, R> {
  /** Optimistic patch applied to the store before the IPC call. */
  optimistic?: (state: S) => Partial<S>;
  /** Store correction applied on success (receives the latest state + IPC result). */
  onSuccess?: (state: S, result: R) => Partial<S>;
  /** Whether to rollback optimistic fields on failure. Default: true. */
  rollback?: boolean;
  /** Custom error handler (called after rollback). */
  onError?: (error: SyncError) => void;
  /** Request deduplication strategy. */
  dedupe?: DedupeStrategy;
}

// ---------------------------------------------------------------------------
// Global defaults
// ---------------------------------------------------------------------------

type GlobalDefaults = {
  onError?: (error: SyncError) => void;
  formatError?: (raw: unknown) => SyncError;
};

const globalDefaults: GlobalDefaults = {};

/**
 * Set global defaults for all `syncToBackend` calls.
 *
 * - `onError`: called after rollback for every failed request (unless
 *   overridden per-call).
 * - `formatError`: custom error normalizer (defaults to a simple
 *   `SyncError` wrapper if not set).
 */
export function setSyncDefaults(defaults: Partial<GlobalDefaults>): void {
  Object.assign(globalDefaults, defaults);
}

// ---------------------------------------------------------------------------
// In-flight tracking
// ---------------------------------------------------------------------------

interface InFlightEntry {
  token: symbol;
  aborted: boolean;
}

const inFlight = new Map<string, InFlightEntry>();

// ---------------------------------------------------------------------------
// Error normalization
// ---------------------------------------------------------------------------

let _formatError: (raw: unknown) => SyncError = (raw) => {
  const message = formatInvokeErrorMessage(raw) ?? "Unknown IPC error";
  return new SyncError(message, raw);
};

/** Override the error formatter used by {@link syncToBackend}. */
export function setSyncErrorFormatter(fn: (raw: unknown) => SyncError): void {
  _formatError = fn;
}

function normalizeIpcError(raw: unknown): SyncError {
  if (globalDefaults.formatError) {
    return globalDefaults.formatError(raw);
  }
  return _formatError(raw);
}

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

/**
 * Execute an IPC call against a store with optimistic update, rollback, and
 * optional deduplication.
 *
 * @param store  - The domain store to update.
 * @param ipcCall - A function that performs the actual IPC (bridge) call.
 * @param options - See {@link SyncOptions}.
 *
 * @returns The IPC result on success.
 * @throws  {SyncError} on failure.
 */
export async function syncToBackend<
  S extends Record<string, unknown>,
  R,
>(
  store: Store<S>,
  ipcCall: () => Promise<R>,
  options: SyncOptions<S, R> = {},
): Promise<R> {
  const {
    optimistic,
    onSuccess,
    rollback = true,
    onError,
    dedupe = "none",
  } = options;

  // ── 1. Resolve dedupe key & strategy ──
  const dedupeKey =
    typeof dedupe === "object" ? dedupe.key : dedupe !== "none" ? dedupe : null;
  const dedupeStrategy =
    typeof dedupe === "object"
      ? dedupe.strategy
      : dedupe !== "none"
        ? dedupe
        : "none";

  let entry: InFlightEntry | null = null;

  if (dedupeKey && dedupeStrategy !== "none") {
    const existing = inFlight.get(dedupeKey);
    if (existing) {
      if (dedupeStrategy === "first") {
        return Promise.reject(
          new SyncError("Request superseded", null, true),
        );
      }
      // 'last': mark the previous as aborted (its return value will be ignored)
      existing.aborted = true;
    }
    entry = { token: Symbol(dedupeKey), aborted: false };
    inFlight.set(dedupeKey, entry);
  }

  // ── 2. Snapshot + optimistic update ──
  const snapshot = store.getState();
  if (optimistic) {
    store.setState(optimistic(snapshot));
  }

  // ── 3. Execute IPC ──
  try {
    const result = await ipcCall();

    // Discard result if this request was superseded (dedupe: 'last')
    if (entry && dedupeKey && entry.aborted) {
      return result; // silently return, don't touch store
    }
    if (entry && dedupeKey && inFlight.get(dedupeKey) !== entry) {
      return result;
    }

    // ── 4. Success correction ──
    if (onSuccess) {
      store.setState(onSuccess(store.getState(), result));
    }

    return result;
  } catch (error) {
    const syncError = normalizeIpcError(error);

    // A superseded request that fails should not rollback (it would overwrite
    // the newer request's optimistic state).
    const superseded =
      entry &&
      (entry.aborted ||
        (dedupeKey ? inFlight.get(dedupeKey) !== entry : false));

    if (!superseded && rollback && optimistic) {
      // Field-level rollback: only restore keys touched by the optimistic patch.
      const optimisticPatch = optimistic(snapshot);
      const rollbackPatch: Partial<S> = {};
      for (const key of Object.keys(optimisticPatch) as (keyof S)[]) {
        rollbackPatch[key] = snapshot[key];
      }
      store.setState(rollbackPatch);
    }

    // ── 5. Error callback ──
    const errorHandler = onError ?? globalDefaults.onError;
    if (!superseded) {
      errorHandler?.(syncError);
    }

    throw syncError;
  } finally {
    // Clean up in-flight entry (but only if our specific entry is still current)
    if (entry && dedupeKey && inFlight.get(dedupeKey) === entry) {
      inFlight.delete(dedupeKey);
    }
  }
}
