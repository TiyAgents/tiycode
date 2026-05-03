/**
 * Coalesced async runner — single-flight + coalescing + throttling.
 *
 * Replaces the 6-ref pattern previously used for `syncWorkspaceSidebar`.
 *
 * Behaviour:
 * 1. **Single-flight**: concurrent callers share the same in-flight promise.
 * 2. **Coalescing**: when a request arrives during an in-flight run, the
 *    caller's options are merged into a pending payload; a single trailing
 *    run executes when the current one finishes (after `minGapMs`).
 * 3. **Throttling**: enforces a minimum gap between independent executions
 *    via `minGapMs`.  If the gap is too short, the request is delayed.
 */

export interface CoalescedRunnerConfig<T> {
  /** Minimum gap in milliseconds between independent executions. */
  minGapMs: number;
  /** The actual work to perform with merged options. Must be async-safe. */
  executeFn: (options: T) => Promise<void>;
}

export interface CoalescedAsyncRunner<T> {
  /** Request an execution, merging options with any pending request. */
  request: (options: T) => Promise<void>;
  /** Whether a run is currently in-flight or a trailing run is queued. */
  isRunning: () => boolean;
}

export function createCoalescedAsyncRunner<T extends object>(
  config: CoalescedRunnerConfig<T>,
): CoalescedAsyncRunner<T> {
  const { minGapMs, executeFn } = config;

  let inFlightPromise: Promise<void> | null = null;
  let pendingPromise: Promise<void> | null = null;
  let pendingOptions: T | null = null;
  let lastFinishedAt = 0;

  const request = (options: T): Promise<void> => {
    // Merge new options into any already-pending options.
    pendingOptions = {
      ...(pendingOptions ?? ({} as unknown as T)),
      ...options,
    };

    // If a trailing run is already queued, return its promise.
    if (pendingPromise) {
      return pendingPromise;
    }

    // If a run is currently in-flight, schedule a trailing run.
    if (inFlightPromise) {
      const trailing = inFlightPromise.then(async () => {
        const elapsed = Date.now() - lastFinishedAt;
        const wait = Math.max(0, minGapMs - elapsed);
        if (wait > 0) {
          await new Promise<void>((resolve) => setTimeout(resolve, wait));
        }
        const opts = pendingOptions!;
        pendingOptions = null;
        pendingPromise = null;
        const run = executeFn(opts);
        inFlightPromise = run.finally(() => {
          lastFinishedAt = Date.now();
          inFlightPromise = null;
        });
        return inFlightPromise;
      });
      pendingPromise = trailing;
      return trailing;
    }

    // No run in-flight: honour minimum gap before starting.
    const elapsed = Date.now() - lastFinishedAt;
    const wait = Math.max(0, minGapMs - elapsed);

    const start = async (): Promise<void> => {
      const opts = pendingOptions!;
      pendingOptions = null;
      const run = executeFn(opts);
      inFlightPromise = run.finally(() => {
        lastFinishedAt = Date.now();
        inFlightPromise = null;
      });
      return inFlightPromise;
    };

    if (wait > 0) {
      const delayed = new Promise<void>((resolve) =>
        setTimeout(resolve, wait),
      ).then(start);
      pendingPromise = delayed.finally(() => {
        pendingPromise = null;
      });
      return pendingPromise;
    }

    return start();
  };

  const isRunning = (): boolean => {
    return inFlightPromise !== null || pendingPromise !== null;
  };

  return { request, isRunning };
}
