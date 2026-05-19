import { describe, expect, it, vi } from "vitest";

import {
  clearDelayedAutoCollapseTimers,
  syncDelayedAutoCollapseTimers,
  type DelayedAutoCollapseEntry,
  type DelayedAutoCollapseTimerState,
} from "./use-delayed-auto-collapse";

type TestTimer = {
  callback: () => void;
  delayMs: number;
  id: number;
};

function createTestState() {
  let nextTimerId = 1;
  const timers = new Map<number, TestTimer>();
  const state: DelayedAutoCollapseTimerState = {
    autoCollapsedIds: new Set(),
    timers: new Map(),
  };

  const scheduleTimer = (callback: () => void, delayMs: number) => {
    const timer: TestTimer = { callback, delayMs, id: nextTimerId++ };
    timers.set(timer.id, timer);
    return timer as unknown as ReturnType<typeof setTimeout>;
  };

  const clearTimer = (timer: ReturnType<typeof setTimeout>) => {
    timers.delete((timer as unknown as TestTimer).id);
  };

  const flushTimer = (id: number) => {
    const timer = timers.get(id);
    if (!timer) return;
    timers.delete(id);
    timer.callback();
  };

  const sync = ({
    entries,
    latestEntries = entries,
    latestManualIds,
    manualIds = new Set<string>(),
    onCollapse,
  }: {
    entries: ReadonlyArray<DelayedAutoCollapseEntry>;
    latestEntries?: ReadonlyArray<DelayedAutoCollapseEntry>;
    latestManualIds?: ReadonlySet<string>;
    manualIds?: ReadonlySet<string>;
    onCollapse: (id: string) => void;
  }) => {
    syncDelayedAutoCollapseTimers({
      clearTimer,
      delayMs: 100,
      entries,
      getLatestEntries: () => latestEntries,
      getLatestUserManuallyOpenedIds: () => latestManualIds ?? manualIds,
      onCollapse,
      scheduleTimer,
      state,
      userManuallyOpenedIds: manualIds,
    });
  };

  return { clearTimer, flushTimer, state, sync, timers };
}

describe("syncDelayedAutoCollapseTimers", () => {
  it("collapses a completed open entry after the scheduled delay", () => {
    const onCollapse = vi.fn();
    const test = createTestState();

    test.sync({
      entries: [{ completed: true, currentOpen: true, id: "tool-1" }],
      onCollapse,
    });

    expect(test.timers.size).toBe(1);
    const [timer] = test.timers.values();
    expect(timer.delayMs).toBe(100);
    expect(onCollapse).not.toHaveBeenCalled();

    test.flushTimer(timer.id);
    expect(onCollapse).toHaveBeenCalledTimes(1);
    expect(onCollapse).toHaveBeenCalledWith("tool-1");
  });

  it("does not schedule entries that are incomplete or already closed", () => {
    const onCollapse = vi.fn();
    const test = createTestState();

    test.sync({
      entries: [
        { completed: false, currentOpen: true, id: "running-tool" },
        { completed: true, currentOpen: false, id: "closed-helper" },
      ],
      onCollapse,
    });

    expect(test.timers.size).toBe(0);
    expect(onCollapse).not.toHaveBeenCalled();
  });

  it("cancels a pending collapse when the entry closes before the timer fires", () => {
    const onCollapse = vi.fn();
    const test = createTestState();

    test.sync({
      entries: [{ completed: true, currentOpen: true, id: "helper-1" }],
      onCollapse,
    });
    expect(test.timers.size).toBe(1);

    test.sync({
      entries: [{ completed: true, currentOpen: false, id: "helper-1" }],
      onCollapse,
    });

    expect(test.timers.size).toBe(0);
    expect(onCollapse).not.toHaveBeenCalled();
  });

  it("does not collapse entries that the user manually opened", () => {
    const onCollapse = vi.fn();
    const test = createTestState();

    test.sync({
      entries: [{ completed: true, currentOpen: true, id: "reasoning-1" }],
      manualIds: new Set(["reasoning-1"]),
      onCollapse,
    });

    expect(test.timers.size).toBe(0);
    expect(onCollapse).not.toHaveBeenCalled();
  });

  it("rechecks latest user manual state when the timer fires", () => {
    const onCollapse = vi.fn();
    const test = createTestState();
    const manualIds = new Set<string>();

    test.sync({
      entries: [{ completed: true, currentOpen: true, id: "tool-1" }],
      latestManualIds: manualIds,
      manualIds,
      onCollapse,
    });

    const [timer] = test.timers.values();
    manualIds.add("tool-1");
    test.flushTimer(timer.id);

    expect(onCollapse).not.toHaveBeenCalled();
  });

  it("rechecks latest entry state when the timer fires", () => {
    const onCollapse = vi.fn();
    const test = createTestState();
    const latestEntries = [{ completed: true, currentOpen: true, id: "tool-1" }];

    test.sync({
      entries: latestEntries,
      latestEntries,
      onCollapse,
    });

    const [timer] = test.timers.values();
    latestEntries[0] = { completed: true, currentOpen: false, id: "tool-1" };
    test.flushTimer(timer.id);

    expect(onCollapse).not.toHaveBeenCalled();
  });

  it("does not schedule the same auto-collapsed entry again while it remains present", () => {
    const onCollapse = vi.fn();
    const test = createTestState();
    const entries = [{ completed: true, currentOpen: true, id: "tool-1" }];

    test.sync({ entries, onCollapse });
    const [timer] = test.timers.values();
    expect(timer).toBeDefined();
    test.flushTimer(timer.id);
    expect(onCollapse).toHaveBeenCalledTimes(1);

    test.sync({ entries, onCollapse });
    expect(test.timers.size).toBe(0);
  });

  it("clears timers on cleanup", () => {
    const onCollapse = vi.fn();
    const test = createTestState();

    test.sync({
      entries: [{ completed: true, currentOpen: true, id: "tool-1" }],
      onCollapse,
    });
    expect(test.timers.size).toBe(1);

    clearDelayedAutoCollapseTimers(test.state, test.clearTimer);

    expect(test.timers.size).toBe(0);
    expect(test.state.timers.size).toBe(0);
  });
});
