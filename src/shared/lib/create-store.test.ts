import { describe, expect, it, vi } from "vitest";

import { createStore, shallowEqual } from "./create-store";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type TestState = {
  count: number;
  label: string;
};

function makeStore(initial?: Partial<TestState>) {
  return createStore<TestState>({ count: 0, label: "", ...initial });
}

// ---------------------------------------------------------------------------
// createStore
// ---------------------------------------------------------------------------

describe("createStore", () => {
  // -- getState / setState ---------------------------------------------------

  it("reads initial state via getState", () => {
    const store = createStore({ x: 1 });
    expect(store.getState()).toEqual({ x: 1 });
  });

  it("updates state with a partial object", () => {
    const store = makeStore();
    store.setState({ count: 5 });
    expect(store.getState().count).toBe(5);
    expect(store.getState().label).toBe(""); // unchanged key preserved
  });

  it("updates state with an updater function", () => {
    const store = makeStore({ count: 2 });
    store.setState((prev) => ({ count: prev.count + 10 }));
    expect(store.getState().count).toBe(12);
  });

  // -- shallow merge ---------------------------------------------------------

  it("shallow-merges partial state (spread)", () => {
    const store = createStore({ a: 1, b: { x: 1 } });
    store.setState({ b: { x: 2 } });
    // b is replaced wholesale — not deep-merged
    expect(store.getState().b).toEqual({ x: 2 });
  });

  it("preserves keys not included in the partial", () => {
    const store = makeStore({ count: 1, label: "hello" });
    store.setState({ count: 99 });
    expect(store.getState()).toEqual({ count: 99, label: "hello" });
  });

  // -- Object.is skip --------------------------------------------------------

  it("skips notification when updater returns the same state reference (Object.is)", () => {
    const store = makeStore({ count: 0 });
    const listener = vi.fn();
    store.subscribe(listener);

    // Function updater that returns the same object — no value changes, no notification.
    store.setState((prev) => prev);
    expect(listener).not.toHaveBeenCalled();
  });

  it("skips notification when partial values match current state", () => {
    const store = makeStore({ count: 0, label: "" });
    const listener = vi.fn();
    store.subscribe(listener);

    // All partial values are already present — no notification.
    store.setState({ count: 0 });
    expect(listener).not.toHaveBeenCalled();
  });

  it("notifies subscribers when state actually changes", () => {
    const store = makeStore({ count: 0 });
    const listener = vi.fn();
    store.subscribe(listener);

    store.setState({ count: 1 });
    expect(listener).toHaveBeenCalledTimes(1);
  });

  // -- subscribe / unsubscribe -----------------------------------------------

  it("calls subscribed listeners on state change", () => {
    const store = makeStore();
    const a = vi.fn();
    const b = vi.fn();
    store.subscribe(a);
    store.subscribe(b);

    store.setState({ count: 42 });
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
  });

  it("stops calling listeners after unsubscribe", () => {
    const store = makeStore();
    const listener = vi.fn();
    const unsub = store.subscribe(listener);

    store.setState({ count: 1 });
    expect(listener).toHaveBeenCalledTimes(1);

    unsub();
    store.setState({ count: 2 });
    expect(listener).toHaveBeenCalledTimes(1); // no new calls
  });

  // -- reset -----------------------------------------------------------------

  it("resets state to initial value", () => {
    const store = makeStore({ count: 0, label: "init" });
    store.setState({ count: 99, label: "changed" });
    store.reset();
    expect(store.getState()).toEqual({ count: 0, label: "init" });
  });

  it("reset notifies subscribers", () => {
    const store = makeStore();
    const listener = vi.fn();
    store.subscribe(listener);

    store.setState({ count: 5 });
    listener.mockClear();

    store.reset();
    expect(listener).toHaveBeenCalledTimes(1);
  });

  // -- listener exception isolation ------------------------------------------

  it("isolates listener exceptions — one throw does not block others", () => {
    const store = makeStore();
    const bad = vi.fn(() => {
      throw new Error("boom");
    });
    const good = vi.fn();
    store.subscribe(bad);
    store.subscribe(good);

    store.setState({ count: 1 });

    expect(bad).toHaveBeenCalledTimes(1);
    expect(good).toHaveBeenCalledTimes(1);
  });

  it("does not re-throw listener exceptions from setState", () => {
    const store = makeStore();
    store.subscribe(() => {
      throw new Error("boom");
    });
    // Should not throw
    expect(() => store.setState({ count: 1 })).not.toThrow();
  });

  it("does not re-throw listener exceptions from reset", () => {
    const store = makeStore();
    store.subscribe(() => {
      throw new Error("boom");
    });
    expect(() => store.reset()).not.toThrow();
  });
});

// ---------------------------------------------------------------------------
// shallowEqual
// ---------------------------------------------------------------------------

describe("shallowEqual", () => {
  it("returns true for identical primitive references", () => {
    expect(shallowEqual(1, 1)).toBe(true);
    expect(shallowEqual("a", "a")).toBe(true);
    expect(shallowEqual(true, true)).toBe(true);
  });

  it("returns false for different primitives", () => {
    expect(shallowEqual(1, 2)).toBe(false);
    expect(shallowEqual("a", "b")).toBe(false);
  });

  it("returns true for the same object reference", () => {
    const obj = { a: 1 };
    expect(shallowEqual(obj, obj)).toBe(true);
  });

  it("returns true for objects with equal shallow keys", () => {
    expect(shallowEqual({ a: 1, b: 2 }, { a: 1, b: 2 })).toBe(true);
  });

  it("returns false for objects with different values", () => {
    expect(shallowEqual({ a: 1 }, { a: 2 })).toBe(false);
  });

  it("returns false for objects with different key counts", () => {
    expect(shallowEqual({ a: 1 }, { a: 1, b: 2 })).toBe(false);
  });

  it("returns true for arrays with equal elements", () => {
    expect(shallowEqual([1, 2, 3], [1, 2, 3])).toBe(true);
  });

  it("returns false for arrays with different elements", () => {
    expect(shallowEqual([1, 2], [1, 3])).toBe(false);
  });

  it("returns false for arrays with different lengths", () => {
    expect(shallowEqual([1, 2], [1, 2, 3])).toBe(false);
  });

  it("returns false when comparing object to array", () => {
    expect(shallowEqual({}, [])).toBe(false);
  });

  it("returns false for null vs object", () => {
    expect(shallowEqual(null, {})).toBe(false);
    expect(shallowEqual({}, null)).toBe(false);
  });

  it("returns true for both null", () => {
    expect(shallowEqual(null, null)).toBe(true);
  });

  it("returns false for nested objects with different refs (shallow only)", () => {
    const a = { x: { deep: 1 } };
    const b = { x: { deep: 1 } };
    // The inner {deep:1} is a new object → Object.is fails at the first level.
    expect(shallowEqual(a, b)).toBe(false);
  });
});
