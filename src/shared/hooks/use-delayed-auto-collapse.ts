import { useEffect, useRef } from "react";

export interface DelayedAutoCollapseEntry {
  /** Stable identifier, for example a tool id, helper id, or reasoning message id. */
  id: string;
  /** Whether the entry has finished executing and is eligible for delayed collapse. */
  completed: boolean;
  /** Whether the entry's collapsible is currently open. */
  currentOpen: boolean;
}

export interface UseDelayedAutoCollapseOptions {
  /** Delay after an entry becomes completed and open before it is auto-collapsed. */
  delayMs?: number;
  /** Current state of all collapsible timeline entries. */
  entries: ReadonlyArray<DelayedAutoCollapseEntry>;
  /** Entries manually opened by the user should not be auto-collapsed again. */
  userManuallyOpenedIds: ReadonlySet<string>;
  /** Called when the hook decides to collapse an entry. */
  onCollapse: (id: string) => void;
}

type TimerHandle = ReturnType<typeof setTimeout>;

export interface DelayedAutoCollapseTimerState {
  autoCollapsedIds: Set<string>;
  timers: Map<string, TimerHandle>;
}

export interface SyncDelayedAutoCollapseTimersOptions {
  clearTimer?: (timer: TimerHandle) => void;
  delayMs: number;
  entries: ReadonlyArray<DelayedAutoCollapseEntry>;
  getLatestEntries: () => ReadonlyArray<DelayedAutoCollapseEntry>;
  getLatestUserManuallyOpenedIds: () => ReadonlySet<string>;
  onCollapse: (id: string) => void;
  scheduleTimer?: (callback: () => void, delayMs: number) => TimerHandle;
  state: DelayedAutoCollapseTimerState;
  userManuallyOpenedIds: ReadonlySet<string>;
}

const DEFAULT_DELAY_MS = 8000;

export function syncDelayedAutoCollapseTimers({
  clearTimer = clearTimeout,
  delayMs,
  entries,
  getLatestEntries,
  getLatestUserManuallyOpenedIds,
  onCollapse,
  scheduleTimer = setTimeout,
  state,
  userManuallyOpenedIds,
}: SyncDelayedAutoCollapseTimersOptions): void {
  const clearPendingTimer = (id: string) => {
    const timer = state.timers.get(id);
    if (timer) {
      clearTimer(timer);
      state.timers.delete(id);
    }
  };

  const entryById = new Map(entries.map((entry) => [entry.id, entry]));

  for (const id of Array.from(state.timers.keys())) {
    const entry = entryById.get(id);
    if (
      !entry
      || !entry.completed
      || !entry.currentOpen
      || userManuallyOpenedIds.has(id)
      || state.autoCollapsedIds.has(id)
    ) {
      clearPendingTimer(id);
    }
  }

  for (const id of Array.from(state.autoCollapsedIds)) {
    if (!entryById.has(id)) {
      state.autoCollapsedIds.delete(id);
    }
  }

  for (const entry of entries) {
    if (
      !entry.completed
      || !entry.currentOpen
      || userManuallyOpenedIds.has(entry.id)
      || state.timers.has(entry.id)
      || state.autoCollapsedIds.has(entry.id)
    ) {
      continue;
    }

    const timer = scheduleTimer(() => {
      state.timers.delete(entry.id);

      if (
        getLatestUserManuallyOpenedIds().has(entry.id)
        || state.autoCollapsedIds.has(entry.id)
      ) {
        return;
      }

      const latestEntry = getLatestEntries().find((candidate) => candidate.id === entry.id);
      if (!latestEntry || !latestEntry.completed || !latestEntry.currentOpen) {
        return;
      }

      state.autoCollapsedIds.add(entry.id);
      onCollapse(entry.id);
    }, delayMs);

    state.timers.set(entry.id, timer);
  }
}

export function clearDelayedAutoCollapseTimers(
  state: DelayedAutoCollapseTimerState,
  clearTimer: (timer: TimerHandle) => void = clearTimeout,
): void {
  for (const timer of state.timers.values()) {
    clearTimer(timer);
  }
  state.timers.clear();
}

export function useDelayedAutoCollapse({
  delayMs = DEFAULT_DELAY_MS,
  entries,
  userManuallyOpenedIds,
  onCollapse,
}: UseDelayedAutoCollapseOptions): void {
  const entriesRef = useRef(entries);
  entriesRef.current = entries;

  const userManualRef = useRef(userManuallyOpenedIds);
  userManualRef.current = userManuallyOpenedIds;

  const onCollapseRef = useRef(onCollapse);
  onCollapseRef.current = onCollapse;

  const stateRef = useRef<DelayedAutoCollapseTimerState>({
    autoCollapsedIds: new Set(),
    timers: new Map(),
  });

  useEffect(() => {
    syncDelayedAutoCollapseTimers({
      delayMs,
      entries,
      getLatestEntries: () => entriesRef.current,
      getLatestUserManuallyOpenedIds: () => userManualRef.current,
      onCollapse: (id) => onCollapseRef.current(id),
      state: stateRef.current,
      userManuallyOpenedIds,
    });
  }, [delayMs, entries, userManuallyOpenedIds]);

  useEffect(
    () => () => clearDelayedAutoCollapseTimers(stateRef.current),
    [],
  );
}
