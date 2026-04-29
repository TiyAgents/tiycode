import { describe, expect, it, vi } from "vitest";

import { createMachine } from "./create-machine";

// ---------------------------------------------------------------------------
// Types for test machines
// ---------------------------------------------------------------------------

type LightState = "green" | "yellow" | "red";
type LightEvent = "TIMER" | "EMERGENCY" | "RESET";

interface CounterCtx {
  count: number;
}

type CounterState = "idle" | "counting";
type CounterEvent = "INC" | "RESET" | "DOUBLE";

// ---------------------------------------------------------------------------
// createMachine
// ---------------------------------------------------------------------------

describe("createMachine", () => {
  // -- basic transitions -----------------------------------------------------

  it("starts at the initial state", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });
    expect(fsm.getState()).toBe("green");
  });

  it("performs a legal transition via string target", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    fsm.send("TIMER");
    expect(fsm.getState()).toBe("yellow");
  });

  it("performs a legal transition with object target", () => {
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: {
          on: {
            INC: { target: "counting", action: (ctx) => ({ count: ctx.count + 1 }) },
          },
        },
        counting: { on: {} },
      },
    });

    fsm.send("INC");
    expect(fsm.getState()).toBe("counting");
    expect(fsm.getContext()).toEqual({ count: 1 });
  });

  // -- illegal transitions ---------------------------------------------------

  it("silently ignores illegal transitions", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    fsm.send("EMERGENCY"); // no handler for EMERGENCY in green
    expect(fsm.getState()).toBe("green");
  });

  it("silently ignores transitions from states with no 'on' map", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: {},
        red: { on: { TIMER: "green" } },
      },
    });

    fsm.send("TIMER"); // green → yellow
    fsm.send("TIMER"); // yellow has no on — ignored
    expect(fsm.getState()).toBe("yellow");
  });

  // -- action callbacks ------------------------------------------------------

  it("executes action callback during transition", () => {
    const onInc = vi.fn((ctx: CounterCtx) => ({ count: ctx.count + 1 }));
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: { on: { INC: { target: "counting", action: onInc } } },
        counting: {},
      },
    });

    fsm.send("INC");
    expect(onInc).toHaveBeenCalledTimes(1);
    expect(onInc).toHaveBeenCalledWith({ count: 0 }, undefined);
  });

  it("passes payload to action callback", () => {
    const onDouble = vi.fn(
      (_ctx: CounterCtx, payload: unknown) =>
        ({ count: (payload as number) * 2 }) as CounterCtx,
    );
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: { on: { DOUBLE: { target: "counting", action: onDouble } } },
        counting: {},
      },
    });

    fsm.send("DOUBLE", 5);
    expect(onDouble).toHaveBeenCalledWith({ count: 0 }, 5);
    expect(fsm.getContext()).toEqual({ count: 10 });
  });

  it("does not mutate context in-place (returns new object)", () => {
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: {
          on: { INC: { target: "counting", action: (ctx) => ({ count: ctx.count + 1 }) } },
        },
        counting: {},
      },
    });

    const ctxBefore = fsm.getContext();
    fsm.send("INC");
    const ctxAfter = fsm.getContext();

    // Context should be a new object
    expect(ctxAfter).not.toBe(ctxBefore);
    // Original context should be unchanged (defensive)
    expect(ctxBefore).toEqual({ count: 0 });
  });

  it("keeps context unchanged when action returns void", () => {
    const sideEffect = vi.fn();
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: {
          on: {
            INC: {
              target: "counting",
              action: (_ctx) => {
                sideEffect();
              },
            },
          },
        },
        counting: {},
      },
    });

    fsm.send("INC");
    expect(sideEffect).toHaveBeenCalledTimes(1);
    expect(fsm.getContext()).toEqual({ count: 0 });
  });

  // -- self-transition with context change -----------------------------------

  it("notifies subscribers on self-transition when context changes", () => {
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: {
          on: {
            INC: {
              target: "idle", // self-transition
              action: (ctx) => ({ count: ctx.count + 1 }),
            },
          },
        },
        counting: {},
      },
    });

    const listener = vi.fn();
    fsm.subscribe(listener);

    fsm.send("INC");
    expect(fsm.getState()).toBe("idle");
    expect(fsm.getContext()).toEqual({ count: 1 });
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("does not notify subscribers on self-transition when both state and context are unchanged", () => {
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: {
          on: {
            INC: {
              target: "idle",
              action: (ctx) => ctx, // returns same object
            },
          },
        },
        counting: {},
      },
    });

    const listener = vi.fn();
    fsm.subscribe(listener);

    fsm.send("INC");
    // State unchanged, context unchanged (same ref) → no notification
    expect(listener).not.toHaveBeenCalled();
  });

  // -- subscribe / unsubscribe -----------------------------------------------

  it("notifies multiple subscribers", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    const a = vi.fn();
    const b = vi.fn();
    fsm.subscribe(a);
    fsm.subscribe(b);

    fsm.send("TIMER");
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
  });

  it("unsubscribe stops notification", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    const listener = vi.fn();
    const unsub = fsm.subscribe(listener);

    fsm.send("TIMER");
    expect(listener).toHaveBeenCalledTimes(1);

    unsub();
    fsm.send("TIMER"); // yellow → red
    expect(listener).toHaveBeenCalledTimes(1); // no new call
  });

  // -- destroy ---------------------------------------------------------------

  it("destroy clears all subscribers and prevents further sends", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    const listener = vi.fn();
    fsm.subscribe(listener);

    fsm.destroy();

    // Subsequent send should be ignored
    fsm.send("TIMER");
    expect(listener).not.toHaveBeenCalled();
    expect(fsm.getState()).toBe("green"); // unchanged
  });

  it("destroy prevents reset", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    fsm.send("TIMER"); // green → yellow
    fsm.destroy();
    fsm.reset("red");

    // Destroy should block reset — state stays at yellow
    expect(fsm.getState()).toBe("yellow");
  });

  // -- reset -----------------------------------------------------------------

  it("resets to initial state and context", () => {
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: {
          on: { INC: { target: "counting", action: (ctx) => ({ count: ctx.count + 1 }) } },
        },
        counting: {},
      },
    });

    fsm.send("INC");
    expect(fsm.getState()).toBe("counting");
    expect(fsm.getContext()).toEqual({ count: 1 });

    fsm.reset();
    expect(fsm.getState()).toBe("idle");
    expect(fsm.getContext()).toEqual({ count: 0 });
  });

  it("reset accepts explicit state and context", () => {
    const fsm = createMachine<CounterState, CounterEvent, CounterCtx>({
      initial: "idle",
      context: { count: 0 },
      states: {
        idle: { on: {} },
        counting: { on: {} },
      },
    });

    fsm.reset("counting", { count: 99 });
    expect(fsm.getState()).toBe("counting");
    expect(fsm.getContext()).toEqual({ count: 99 });
  });

  it("reset notifies subscribers", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    const listener = vi.fn();
    fsm.subscribe(listener);

    fsm.reset("red");
    expect(listener).toHaveBeenCalledTimes(1);
  });

  // -- listener exception isolation ------------------------------------------

  it("isolates listener exceptions — one throw does not block others", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    const bad = vi.fn(() => {
      throw new Error("boom");
    });
    const good = vi.fn();
    fsm.subscribe(bad);
    fsm.subscribe(good);

    fsm.send("TIMER");
    expect(bad).toHaveBeenCalledTimes(1);
    expect(good).toHaveBeenCalledTimes(1);
  });

  it("does not re-throw listener exceptions from send", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    fsm.subscribe(() => {
      throw new Error("boom");
    });
    expect(() => fsm.send("TIMER")).not.toThrow();
  });

  // -- default context (void) ------------------------------------------------

  it("returns undefined context when no context is configured", () => {
    const fsm = createMachine<LightState, LightEvent>({
      initial: "green",
      states: {
        green: { on: { TIMER: "yellow" } },
        yellow: { on: { TIMER: "red" } },
        red: { on: { TIMER: "green" } },
      },
    });

    expect(fsm.getContext()).toBeUndefined();
  });
});
