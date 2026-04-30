import { describe, expect, it, vi } from "vitest";
import { createCoalescedAsyncRunner } from "./sidebar-sync-runner";

interface TestOptions {
  preserveSelectedProjectIfMissing?: boolean;
  threadDisplayCountOverrides: Record<string, number>;
}

describe("createCoalescedAsyncRunner", () => {
  it("executes the function on first request", async () => {
    const executeFn = vi.fn().mockResolvedValue(undefined);
    const runner = createCoalescedAsyncRunner<TestOptions>({
      minGapMs: 0,
      executeFn,
    });

    await runner.request({
      preserveSelectedProjectIfMissing: true,
      threadDisplayCountOverrides: { ws1: 10 },
    });

    expect(executeFn).toHaveBeenCalledTimes(1);
    expect(executeFn).toHaveBeenCalledWith({
      preserveSelectedProjectIfMissing: true,
      threadDisplayCountOverrides: { ws1: 10 },
    });
  });

  it("coalesces concurrent requests into a single execution", async () => {
    // Use a deferred so the first request doesn't complete before the
    // second one arrives.
    let resolveFirst: () => void;
    const firstPromise = new Promise<void>((r) => {
      resolveFirst = r;
    });

    const executeFn = vi
      .fn()
      .mockResolvedValueOnce(firstPromise)
      .mockResolvedValue(undefined);

    const runner = createCoalescedAsyncRunner<TestOptions>({
      minGapMs: 0,
      executeFn,
    });

    // Fire two requests — the first will be in-flight.
    const p1 = runner.request({
      threadDisplayCountOverrides: { ws1: 10 },
    });
    const p2 = runner.request({
      threadDisplayCountOverrides: { ws2: 20 },
    });

    // Resolve the first so the trailing can run.
    resolveFirst!();
    await Promise.all([p1, p2]);

    // Should execute twice: once for the first request, once for trailing.
    expect(executeFn).toHaveBeenCalledTimes(2);
    expect(executeFn).toHaveBeenNthCalledWith(1, {
      threadDisplayCountOverrides: { ws1: 10 },
    });
    expect(executeFn).toHaveBeenNthCalledWith(2, {
      threadDisplayCountOverrides: { ws2: 20 },
    });
  });

  it("coalesces trailing request when in-flight", async () => {
    // Use a deferred promise so we can control when the first execution completes
    let resolveFirst: () => void;
    const firstPromise = new Promise<void>((resolve) => {
      resolveFirst = resolve;
    });

    const executeFn = vi
      .fn()
      .mockResolvedValueOnce(firstPromise) // first call — pending
      .mockResolvedValue(undefined); // trailing call

    const runner = createCoalescedAsyncRunner<TestOptions>({
      minGapMs: 0,
      executeFn,
    });

    // Start the first request (does NOT await)
    const firstReq = runner.request({
      threadDisplayCountOverrides: { ws1: 5 },
    });

    // While first is in-flight, fire a second request
    const secondReq = runner.request({
      threadDisplayCountOverrides: { ws2: 10 },
    });

    // Resolve the first
    resolveFirst!();
    await firstReq;

    // Now the trailing should have been scheduled; wait for it
    await secondReq;

    expect(executeFn).toHaveBeenCalledTimes(2);
    expect(executeFn).toHaveBeenNthCalledWith(1, {
      threadDisplayCountOverrides: { ws1: 5 },
    });
    expect(executeFn).toHaveBeenNthCalledWith(2, {
      // Options are shallow-merged: later overrides replace earlier fields.
      threadDisplayCountOverrides: { ws2: 10 },
    });
  });

  it("reports isRunning correctly", async () => {
    let resolve: () => void;
    const pending = new Promise<void>((r) => {
      resolve = r;
    });

    const executeFn = vi.fn().mockReturnValue(pending);
    const runner = createCoalescedAsyncRunner<TestOptions>({
      minGapMs: 0,
      executeFn,
    });

    expect(runner.isRunning()).toBe(false);

    const req = runner.request({
      threadDisplayCountOverrides: {},
    });

    expect(runner.isRunning()).toBe(true);

    resolve!();
    await req;

    expect(runner.isRunning()).toBe(false);
  });

  it("handles executeFn failure and recovers", async () => {
    const executeFn = vi
      .fn()
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce(undefined);

    const runner = createCoalescedAsyncRunner<TestOptions>({
      minGapMs: 0,
      executeFn,
    });

    // First request fails
    await expect(
      runner.request({ threadDisplayCountOverrides: {} }),
    ).rejects.toThrow("boom");

    // Runner should recover and allow a new request
    expect(runner.isRunning()).toBe(false);

    await runner.request({ threadDisplayCountOverrides: { ws1: 1 } });
    expect(executeFn).toHaveBeenCalledTimes(2);
  });
});
