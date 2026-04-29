import { useSyncExternalStore } from "react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Transition target: either a bare state name or an object with target + optional action. */
export type MachineTransition<S extends string, C> =
  | S
  | { target: S; action?: (ctx: C, payload?: unknown) => C | void };

/** Per-state event map. */
type EventMap<S extends string, E extends string, C> = Partial<
  Record<E, MachineTransition<S, C>>
>;

/** Configuration for {@link createMachine}. */
export interface MachineConfig<S extends string, E extends string, C = void> {
  /** Initial state. */
  initial: S;
  /** Optional context data carried alongside the state. */
  context?: C;
  /** State graph: each state defines which events trigger which transitions. */
  states: Record<S, { on?: EventMap<S, E, C> }>;
}

/** A state-machine instance returned by {@link createMachine}. */
export interface Machine<S extends string, E extends string, C = void> {
  /** Current state. */
  getState: () => S;
  /** Current context. */
  getContext: () => C;
  /** Dispatch an event. Illegal transitions are silently ignored. */
  send: (event: E, payload?: unknown) => void;
  /** Subscribe to state & context changes. Returns an unsubscribe function. */
  subscribe: (listener: () => void) => () => void;
  /** Reset to a given state (defaults to initial) and optional context. */
  reset: (state?: S, context?: C) => void;
  /** Remove all subscribers and prevent further sends. Call on unmount. */
  destroy: () => void;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/**
 * Create a lightweight finite-state machine.
 *
 * @typeParam S — Union of state name string literals.
 * @typeParam E — Union of event name string literals.
 * @typeParam C — Context type carried alongside the state (defaults to `void`).
 *
 * @remarks
 * - **Actions are synchronous.** Asynchronous side-effects should be triggered
 *   externally by subscribing to state changes via {@link Machine.subscribe} or
 *   a React `useEffect`.
 * - **Context immutability:** Action callbacks **must return a new context
 *   object** when the context changes. Mutating `ctx` in place will prevent
 *   `useMachineContext` subscribers from detecting the change.
 * - **Self-transitions:** When `send` targets the current state but the action
 *   returns a new context, subscribers are still notified.
 *
 * @example
 * ```ts
 * const fsm = createMachine({
 *   initial: "idle",
 *   context: { retries: 0 },
 *   states: {
 *     idle: { on: { START: "running" } },
 *     running: {
 *       on: {
 *         COMPLETE: "completed",
 *         FAIL: { target: "idle", action: (ctx) => ({ retries: ctx.retries + 1 }) },
 *       },
 *     },
 *     completed: {},
 *   },
 * });
 * ```
 */
export function createMachine<S extends string, E extends string, C = void>(
  config: MachineConfig<S, E, C>,
): Machine<S, E, C> {
  let currentState: S = config.initial;
  let currentContext = config.context as C;
  let destroyed = false;

  const listeners = new Set<() => void>();

  const emit = (): void => {
    if (destroyed) return;
    listeners.forEach((l) => {
      try {
        l();
      } catch (e) {
        console.error("[Machine] listener threw:", e);
      }
    });
  };

  const getState = (): S => currentState;

  const getContext = (): C => currentContext;

  const send = (event: E, payload?: unknown): void => {
    if (destroyed) return;
    const stateDef = config.states[currentState];
    if (!stateDef || !stateDef.on) return;

    const transition = stateDef.on[event];
    if (transition === undefined) return; // illegal — silently ignore

    const prevState = currentState;
    const prevContext = currentContext;

    if (typeof transition === "string") {
      currentState = transition as S;
    } else {
      currentState = transition.target;
      if (transition.action) {
        const result = transition.action(currentContext, payload);
        if (result !== undefined) {
          currentContext = result as C;
        }
      }
    }

    // Notify only when state or context actually changed.
    if (
      !Object.is(prevState, currentState) ||
      !Object.is(prevContext, currentContext)
    ) {
      emit();
    }
  };

  const subscribe = (listener: () => void): (() => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };

  const reset = (state?: S, context?: C): void => {
    if (destroyed) return;
    currentState = state ?? config.initial;
    currentContext = context ?? (config.context as C);
    emit();
  };

  const destroy = (): void => {
    destroyed = true;
    listeners.clear();
  };

  return { getState, getContext, send, subscribe, reset, destroy };
}

// ---------------------------------------------------------------------------
// React Hooks
// ---------------------------------------------------------------------------

/**
 * Subscribe to the current state of a state machine.
 *
 * ⚠️ This is a React Hook — call it at the top level of a component or
 * custom Hook.
 */
export function useMachine<S extends string, E extends string, C>(
  machine: Machine<S, E, C>,
): S {
  return useSyncExternalStore(machine.subscribe, machine.getState);
}

/**
 * Subscribe to the current context of a state machine.
 *
 * ⚠️ This is a React Hook — call it at the top level of a component or
 * custom Hook.
 */
export function useMachineContext<S extends string, E extends string, C>(
  machine: Machine<S, E, C>,
): C {
  return useSyncExternalStore(
    machine.subscribe,
    machine.getContext,
  );
}
