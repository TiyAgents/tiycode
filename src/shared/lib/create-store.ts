import { useSyncExternalStore, useRef, useCallback } from "react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface Store<S extends object> {
  /** Snapshot of the current state. */
  getState: () => S;
  /**
   * Update state via a partial object or an updater function.
   *
   * @remarks
   * State is merged shallowly (`{ ...prev, ...next }`). Nested objects are
   * replaced wholesale, not deep-merged — the caller is responsible for
   * constructing complete sub-objects.
   */
  setState: (next: Partial<S> | ((prev: S) => Partial<S>)) => void;
  /** Subscribe to every state change. Returns an unsubscribe function. */
  subscribe: (listener: () => void) => () => void;
  /** Reset state to the initial value and notify subscribers (primarily for test isolation). */
  reset: () => void;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/**
 * Create a lightweight external store backed by a module-level closure.
 *
 * @example
 * ```ts
 * const counterStore = createStore({ count: 0 });
 * counterStore.setState({ count: 1 });
 * counterStore.setState((prev) => ({ count: prev.count + 1 }));
 * ```
 */
export function createStore<S extends object>(
  initialState: S,
): Store<S> {
  let state: S = initialState;
  const listeners = new Set<() => void>();

  const emitSafe = (): void => {
    listeners.forEach((l) => {
      try {
        l();
      } catch (e) {
        console.error("[Store] listener threw:", e);
      }
    });
  };

  const getState = (): S => state;

  const setState = (next: Partial<S> | ((prev: S) => Partial<S>)): void => {
    const partial = typeof next === "function" ? next(state) : next;

    // Skip if no key actually changed (Object.is equality).
    let changed = false;
    for (const key of Object.keys(partial)) {
      if (!Object.is((state as Record<string, unknown>)[key], (partial as Record<string, unknown>)[key])) {
        changed = true;
        break;
      }
    }
    if (!changed) return;

    const nextState: S = { ...state, ...partial } as S;
    state = nextState;
    emitSafe();
  };

  const subscribe = (listener: () => void): (() => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };

  const reset = (): void => {
    state = initialState;
    emitSafe();
  };

  return { getState, setState, subscribe, reset };
}

// ---------------------------------------------------------------------------
// React Hook
// ---------------------------------------------------------------------------

/**
 * Subscribe to a slice of a store from a React component.
 *
 * @remarks
 * ⚠️ This is a React Hook — call it at the top level of a component or
 * custom Hook, never inside conditions or loops.
 *
 * @param store - A store created with {@link createStore}.
 * @param selector - Returns the slice of state the component needs.
 * @param isEqual - Optional equality function for the selector result.
 *   Defaults to `Object.is`. Pass {@link shallowEqual} when the selector
 *   returns a new object/array on every call (e.g. `.filter()`, object picks)
 *   to avoid unnecessary re-renders.
 *
 * @important The store's `setState` uses shallow merge (spread). Nested
 * objects are replaced entirely, not deep-merged.
 */
export function useStore<S extends object, T>(
  store: Store<S>,
  selector: (s: S) => T,
  isEqual: (a: T, b: T) => boolean = Object.is,
): T {
  // Cache the last selector result so getSnapshot can return a stable
  // reference when the value hasn't changed.
  const prevRef = useRef<{ value: T } | null>(null);

  const getSnapshot = useCallback((): T => {
    const next = selector(store.getState());
    if (prevRef.current !== null && isEqual(prevRef.current.value, next)) {
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
      return prevRef.current.value;
    }
    prevRef.current = { value: next };
    return next;
  }, [store, selector, isEqual]);

  return useSyncExternalStore(store.subscribe, getSnapshot);
}

// ---------------------------------------------------------------------------
// Equality helpers
// ---------------------------------------------------------------------------

/**
 * Shallow equality comparison for objects and arrays.
 *
 * Use as the `isEqual` parameter of {@link useStore} when the selector
 * returns a new object or array on every invocation.
 */
export function shallowEqual<T>(a: T, b: T): boolean {
  if (Object.is(a, b)) return true;
  if (
    typeof a !== "object" ||
    a === null ||
    typeof b !== "object" ||
    b === null
  ) {
    return false;
  }
  // Both must be the same structural type (both arrays or both plain objects).
  const aIsArray = Array.isArray(a);
  const bIsArray = Array.isArray(b);
  if (aIsArray !== bIsArray) return false;

  // Compare arrays element-by-element
  if (aIsArray && bIsArray) {
    const arrA = a as unknown[];
    const arrB = b as unknown[];
    if (arrA.length !== arrB.length) return false;
    for (let i = 0; i < arrA.length; i++) {
      if (!Object.is(arrA[i], arrB[i])) return false;
    }
    return true;
  }
  // Compare plain-object keys
  const keysA = Object.keys(a as object);
  const keysB = Object.keys(b as object);
  if (keysA.length !== keysB.length) return false;
  for (const key of keysA) {
    if (
      !Object.prototype.hasOwnProperty.call(b, key) ||
      !Object.is(
        (a as Record<string, unknown>)[key],
        (b as Record<string, unknown>)[key],
      )
    ) {
      return false;
    }
  }
  return true;
}
