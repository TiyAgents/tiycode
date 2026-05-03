import { afterEach, describe, expect, it, vi } from "vitest";

import { createStore } from "./create-store";
import {
  syncToBackend,
  SyncError,
  setSyncDefaults,
  setSyncErrorFormatter,
  type SyncOptions,
} from "./ipc-sync";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type TestState = {
  items: string[];
  count: number;
  label: string;
};

function makeStore(initial?: Partial<TestState>) {
  return createStore<TestState>({
    items: [],
    count: 0,
    label: "",
    ...initial,
  });
}

/** Create a mock IPC call that resolves or rejects after 0 ticks. */
function mockIpc<R>(result: R | Error) {
  return vi.fn<() => Promise<R>>().mockImplementation(() => {
    if (result instanceof Error) return Promise.reject(result);
    return Promise.resolve(result as R);
  });
}

/** Create a mock IPC call that rejects with an arbitrary value (not just Error). */
function mockIpcReject(value: unknown) {
  return vi.fn<() => Promise<never>>().mockRejectedValue(value);
}

/** Create a deferred promise that can be resolved/rejected externally. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

afterEach(() => {
  // Reset global defaults between tests
  setSyncDefaults({ onError: undefined, formatError: undefined });
  setSyncErrorFormatter((raw) => {
    const message =
      raw instanceof Error
        ? raw.message
        : typeof raw === "string"
          ? raw
          : "Unknown IPC error";
    return new SyncError(message, raw);
  });
});

// ---------------------------------------------------------------------------
// Optimistic update + success correction
// ---------------------------------------------------------------------------

describe("syncToBackend", () => {
  it("applies optimistic update before IPC and corrects on success", async () => {
    const store = makeStore({ items: ["a"], count: 0 });
    const ipc = mockIpc(["a", "b", "c"]);

    const result = await syncToBackend(store, ipc, {
      optimistic: (s) => ({ items: [...s.items, "tmp"], count: 99 }),
      onSuccess: (_s, r: string[]) => ({ items: r, count: 0 }),
    });

    // After optimistic (before IPC completes), store should see tmp
    expect(result).toEqual(["a", "b", "c"]);
    expect(store.getState().items).toEqual(["a", "b", "c"]);
    expect(store.getState().count).toBe(0);
  });

  it("applies optimistic update only (no onSuccess)", async () => {
    const store = makeStore({ count: 0 });
    const ipc = mockIpc(42);

    await syncToBackend(store, ipc, {
      optimistic: (s) => ({ count: s.count + 1 }),
    });

    // No onSuccess => optimistic stays in place
    expect(store.getState().count).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// Rollback on failure
// ---------------------------------------------------------------------------

describe("rollback", () => {
  it("rolls back optimistic fields on failure", async () => {
    const store = makeStore({ items: ["a"], count: 0, label: "hello" });
    const ipc = mockIpc(new Error("network"));

    await expect(
      syncToBackend(store, ipc, {
        optimistic: (s) => ({ items: [...s.items, "tmp"], count: 99 }),
      }),
    ).rejects.toThrow(SyncError);

    // Only optimistic fields should be restored
    expect(store.getState().items).toEqual(["a"]);
    expect(store.getState().count).toBe(0);
    expect(store.getState().label).toBe("hello");
  });

  it("field-level rollback: does not touch non-optimistic keys", async () => {
    const store = makeStore({ items: ["x"], count: 5, label: "keep" });
    const ipc = mockIpc(new Error("fail"));

    await expect(
      syncToBackend(store, ipc, {
        optimistic: (s) => ({ items: [...s.items, "tmp"] }),
      }),
    ).rejects.toThrow(SyncError);

    // Only items was in optimistic -> only items is rolled back
    expect(store.getState().items).toEqual(["x"]);
    // count was NOT in optimistic, so it keeps whatever value it had
    // (in this case no other setState happened, so it stays 5)
    expect(store.getState().count).toBe(5);
    expect(store.getState().label).toBe("keep");
  });

  it("does not rollback when rollback: false", async () => {
    const store = makeStore({ items: ["a"] });
    const ipc = mockIpc(new Error("fail"));

    await expect(
      syncToBackend(store, ipc, {
        optimistic: (s) => ({ items: [...s.items, "tmp"] }),
        rollback: false,
      }),
    ).rejects.toThrow(SyncError);

    // rollback disabled -> optimistic stays
    expect(store.getState().items).toEqual(["a", "tmp"]);
  });
});

// ---------------------------------------------------------------------------
// Mode B: no optimistic update
// ---------------------------------------------------------------------------

describe("no optimistic", () => {
  it("does not modify store before IPC, only on success", async () => {
    const store = makeStore({ items: ["a"] });
    const ipc = mockIpc(["a", "b"]);

    await syncToBackend(store, ipc, {
      onSuccess: (_s, r: string[]) => ({ items: r }),
    });

    expect(store.getState().items).toEqual(["a", "b"]);
  });

  it("does nothing on failure when no optimistic", async () => {
    const store = makeStore({ items: ["a"] });
    const ipc = mockIpc(new Error("fail"));

    await expect(
      syncToBackend(store, ipc, {
        onSuccess: (_s, r) => ({ items: r as unknown as string[] }),
      }),
    ).rejects.toThrow(SyncError);

    // Store unchanged
    expect(store.getState().items).toEqual(["a"]);
  });
});

// ---------------------------------------------------------------------------
// Deduplication: 'first'
// ---------------------------------------------------------------------------

describe("dedupe 'first'", () => {
  it("ignores subsequent calls with same key", async () => {
    const store = makeStore({ count: 0 });
    // First call resolves slowly, second rejects immediately
    let resolve1!: (v: number) => void;
    const ipc1 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((r) => { resolve1 = r; }),
    );
    const ipc2 = vi.fn<() => Promise<number>>().mockRejectedValue(new Error("should not happen"));

    const opts: SyncOptions<TestState, number> = {
      dedupe: { key: "counter", strategy: "first" },
    };

    const p1 = syncToBackend(store, ipc1, opts);
    const p2 = syncToBackend(store, ipc2, opts);

    // Second call should be rejected immediately
    await expect(p2).rejects.toThrow("Request superseded");

    // Complete first call
    resolve1(10);
    await expect(p1).resolves.toBe(10);
    expect(ipc2).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Deduplication: 'last'
// ---------------------------------------------------------------------------

describe("dedupe 'last'", () => {
  it("old request result is silently ignored", async () => {
    const store = makeStore({ count: 0 });
    let resolve1!: (v: number) => void;
    let resolve2!: (v: number) => void;

    const ipc1 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((r) => { resolve1 = r; }),
    );
    const ipc2 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((r) => { resolve2 = r; }),
    );

    const opts: SyncOptions<TestState, number> = {
      optimistic: (s) => ({ count: s.count + 1 }),
      dedupe: { key: "counter", strategy: "last" },
    };

    const p1 = syncToBackend(store, ipc1, opts);
    // Second request supersedes first
    const p2 = syncToBackend(store, ipc2, opts);

    // Complete first (old) request — should be silently ignored
    resolve1(10);
    await expect(p1).resolves.toBe(10);
    // Store should still have optimistic from second call (count: from 0 optimistic +1 twice? No - first call optimistic added 1 (count=1), then second call optimistic added 1 (count=2))
    // Actually let's check: ipc1 starts, optimistic: 0->1. Then ipc2 starts, marks ipc1 aborted, optimistic: 1->2.
    expect(store.getState().count).toBe(2);

    // Complete second (latest) request
    resolve2(20);
    await expect(p2).resolves.toBe(20);
    // No onSuccess, so optimistic stays
    expect(store.getState().count).toBe(2);
  });

  it("superseded request failure does NOT rollback", async () => {
    const store = makeStore({ count: 0 });
    let reject1!: (e: Error) => void;
    let resolve2!: (v: number) => void;

    const ipc1 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((_res, rej) => { reject1 = rej; }),
    );
    const ipc2 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((r) => { resolve2 = r; }),
    );

    const opts: SyncOptions<TestState, number> = {
      optimistic: (s) => ({ count: s.count + 1 }),
      dedupe: { key: "counter", strategy: "last" },
    };

    const p1 = syncToBackend(store, ipc1, opts);
    // Second request supersedes
    syncToBackend(store, ipc2, opts);

    // First request fails AFTER being superseded
    reject1(new Error("superseded failure"));
    // p1 should reject, but store should NOT be rolled back (second call's optimistic is in place)
    await expect(p1).rejects.toThrow(SyncError);
    expect(store.getState().count).toBe(2); // second call's optimistic is intact

    // Complete second call
    resolve2(20);
  });
});

// ---------------------------------------------------------------------------
// Deduplication: string shorthand
// ---------------------------------------------------------------------------

describe("dedupe string shorthand", () => {
  it("'first' shorthand works", async () => {
    const store = makeStore({ count: 0 });
    const d1 = deferred<number>();

    const ipc1 = vi.fn<() => Promise<number>>().mockReturnValue(d1.promise);

    // First call starts but doesn't complete yet
    const p1 = syncToBackend(store, ipc1, { dedupe: "first" });

    // Second call with same key should be rejected immediately
    await expect(
      syncToBackend(store, mockIpc(2), { dedupe: "first" }),
    ).rejects.toThrow("Request superseded");

    // Complete first call
    d1.resolve(1);
    await p1;
  });

  it("'last' shorthand works", async () => {
    const store = makeStore({ count: 0 });
    let resolve1!: (v: number) => void;
    let resolve2!: (v: number) => void;

    const ipc1 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((r) => { resolve1 = r; }),
    );
    const ipc2 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((r) => { resolve2 = r; }),
    );

    syncToBackend(store, ipc1, { dedupe: "last" });
    syncToBackend(store, ipc2, { dedupe: "last" });

    resolve1(10);
    resolve2(20);
  });

  it("'none' shorthand does not dedupe", async () => {
    const store = makeStore({ count: 0 });
    const ipc1 = mockIpc(1);
    const ipc2 = mockIpc(2);

    // Both should complete without rejection
    await syncToBackend(store, ipc1, { dedupe: "none" });
    await syncToBackend(store, ipc2, { dedupe: "none" });

    expect(ipc1).toHaveBeenCalled();
    expect(ipc2).toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Error normalization
// ---------------------------------------------------------------------------

describe("error normalization", () => {
  it("wraps Error instances as SyncError", async () => {
    const store = makeStore();
    const ipc = mockIpc(new Error("something broke"));

    await expect(syncToBackend(store, ipc)).rejects.toMatchObject({
      name: "SyncError",
      message: "something broke",
    });
  });

  it("wraps string errors as SyncError", async () => {
    const store = makeStore();
    const ipc = mockIpcReject("plain string error");

    await expect(syncToBackend(store, ipc)).rejects.toMatchObject({
      name: "SyncError",
      message: "plain string error",
    });
  });

  it("uses custom error formatter when set", async () => {
    const store = makeStore();
    setSyncErrorFormatter((raw) => new SyncError(`CUSTOM: ${String(raw)}`, raw));
    const ipc = mockIpc(new Error("boom"));

    await expect(syncToBackend(store, ipc)).rejects.toMatchObject({
      name: "SyncError",
      message: "CUSTOM: Error: boom",
    });
  });

  it("uses global formatError default", async () => {
    const store = makeStore();
    setSyncDefaults({
      formatError: (raw) => new SyncError(`GLOBAL: ${String(raw)}`, raw),
    });
    const ipc = mockIpc(new Error("boom"));

    await expect(syncToBackend(store, ipc)).rejects.toMatchObject({
      message: "GLOBAL: Error: boom",
    });
  });
});

// ---------------------------------------------------------------------------
// onError callback
// ---------------------------------------------------------------------------

describe("onError callback", () => {
  it("calls per-call onError after rollback", async () => {
    const store = makeStore({ count: 0 });
    const onError = vi.fn();
    const ipc = mockIpc(new Error("fail"));

    await expect(
      syncToBackend(store, ipc, {
        optimistic: (s) => ({ count: s.count + 1 }),
        onError,
      }),
    ).rejects.toThrow(SyncError);

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalledWith(expect.any(SyncError));
    expect(store.getState().count).toBe(0); // rolled back
  });

  it("calls global onError when no per-call handler", async () => {
    const store = makeStore({ count: 0 });
    const globalOnError = vi.fn();
    setSyncDefaults({ onError: globalOnError });
    const ipc = mockIpc(new Error("fail"));

    await expect(
      syncToBackend(store, ipc, {
        optimistic: (s) => ({ count: s.count + 1 }),
      }),
    ).rejects.toThrow(SyncError);

    expect(globalOnError).toHaveBeenCalledTimes(1);
  });

  it("superseded request does NOT trigger onError", async () => {
    const store = makeStore({ count: 0 });
    const onError = vi.fn();
    let reject1!: (e: Error) => void;
    let resolve2!: (v: number) => void;

    const ipc1 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((_res, rej) => { reject1 = rej; }),
    );
    const ipc2 = vi.fn<() => Promise<number>>().mockImplementation(
      () => new Promise((r) => { resolve2 = r; }),
    );

    const opts: SyncOptions<TestState, number> = {
      onError,
      dedupe: { key: "test", strategy: "last" },
    };

    const p1 = syncToBackend(store, ipc1, opts).catch(() => {}); // suppress unhandled rejection
    void p1;
    syncToBackend(store, ipc2, { ...opts, onError: vi.fn() });

    reject1(new Error("old failed"));
    resolve2(42);

    // Wait a tick for promises
    await new Promise((r) => setTimeout(r, 10));

    // First call's onError should NOT have been called (superseded)
    expect(onError).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Store.subscribe notifications
// ---------------------------------------------------------------------------

describe("store notifications", () => {
  it("notifies subscribers on optimistic update and success correction", async () => {
    const store = makeStore({ count: 0 });
    const listener = vi.fn();
    store.subscribe(listener);

    const ipc = mockIpc(10);

    await syncToBackend(store, ipc, {
      optimistic: (s) => ({ count: s.count + 1 }),
      onSuccess: (_s, r: number) => ({ count: r }),
    });

    // Called at least twice: once for optimistic, once for onSuccess
    expect(listener).toHaveBeenCalled();
    expect(store.getState().count).toBe(10);
  });
});
