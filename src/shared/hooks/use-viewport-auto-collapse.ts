import { useCallback, useEffect, useRef } from "react";

/**
 * Describes a collapsible timeline entry that this hook may auto-collapse
 * once it scrolls fully above the visible viewport.
 */
export interface ViewportAutoCollapseEntry {
  /** Stable identifier (e.g. tool id, helper id, reasoning message id). */
  id: string;
  /** Whether the entry has finished executing (tool completed, helper completed, reasoning done streaming). */
  completed: boolean;
  /** Whether the entry's collapsible is currently open. */
  currentOpen: boolean;
}

export interface UseViewportAutoCollapseOptions {
  /**
   * The scroll container element (the element with `overflow-y: auto/scroll`).
   * Pass `null` until the element is available.
   */
  scrollContainer: HTMLElement | null;
  /**
   * A ref-like getter that returns the current stuck-to-bottom state.
   * Using a getter (rather than a reactive boolean) avoids re-creating the
   * IntersectionObserver on every scroll position change.
   */
  getIsStuckToBottom: () => boolean;
  /** Current state of all collapsible timeline entries. */
  entries: ReadonlyArray<ViewportAutoCollapseEntry>;
  /**
   * Map from entry id → the DOM wrapper element.
   * The hook observes these elements with IntersectionObserver.
   */
  wrapperRefs: Map<string, HTMLElement>;
  /**
   * Set of entry ids that the user has manually re-opened after an
   * auto-collapse.  These entries will never be auto-collapsed again.
   */
  userManuallyOpenedIds: ReadonlySet<string>;
  /**
   * Called when the hook decides to collapse an entry.
   * The consumer should set the entry's open state to false.
   */
  onCollapse: (id: string) => void;
}

/**
 * Automatically collapses completed collapsible timeline entries once they
 * scroll fully above the visible viewport, but only when the scroll container
 * is stuck to bottom.
 *
 * To avoid visible layout shifts, the hook compensates scrollTop in the same
 * animation frame as the collapse so the viewport content does not move.
 */
export function useViewportAutoCollapse({
  scrollContainer,
  getIsStuckToBottom,
  entries,
  wrapperRefs,
  userManuallyOpenedIds,
  onCollapse,
}: UseViewportAutoCollapseOptions): void {
  // Keep mutable refs so the IO callback always sees the latest values without
  // needing to tear down / recreate the observer on every render.
  const getIsStuckRef = useRef(getIsStuckToBottom);
  getIsStuckRef.current = getIsStuckToBottom;

  const entriesRef = useRef(entries);
  entriesRef.current = entries;

  const userManualRef = useRef(userManuallyOpenedIds);
  userManualRef.current = userManuallyOpenedIds;

  const onCollapseRef = useRef(onCollapse);
  onCollapseRef.current = onCollapse;

  const scrollContainerRef = useRef(scrollContainer);
  scrollContainerRef.current = scrollContainer;

  // We keep a single long-lived IntersectionObserver.  The `wrapperRefs` map
  // is the source of truth for which elements are currently observed.
  const observerRef = useRef<IntersectionObserver | null>(null);
  const observedElementsRef = useRef<Map<string, HTMLElement>>(new Map());

  // Set of ids that have already been auto-collapsed this session to avoid
  // redundant work if the IO fires again for the same element.
  const autoCollapsedRef = useRef<Set<string>>(new Set());

  /**
   * The IO callback.  For each entry reported as not intersecting *above*
   * the viewport, trigger a collapse + scrollTop compensation.
   */
  const handleIntersection = useCallback(
    (ioEntries: IntersectionObserverEntry[]) => {
      if (!getIsStuckRef.current()) return;

      for (const ioEntry of ioEntries) {
        if (ioEntry.isIntersecting) continue;

        // Ensure the element is above the viewport, not below.
        const rootTop = ioEntry.rootBounds?.top ?? 0;
        if (ioEntry.boundingClientRect.bottom >= rootTop) continue;

        // Find the id for this element.
        const el = ioEntry.target as HTMLElement;
        const id = el.dataset.timelineEntryId;
        if (!id) continue;

        // Guard: only collapse completed + open entries that the user hasn't
        // manually re-opened and that we haven't already collapsed.
        if (userManualRef.current.has(id)) continue;
        if (autoCollapsedRef.current.has(id)) continue;

        const entry = entriesRef.current.find((e) => e.id === id);
        if (!entry || !entry.completed || !entry.currentOpen) continue;

        // --- Collapse with scrollTop compensation ---
        const oldHeight = el.offsetHeight;

        // Mark first so we don't re-enter.
        autoCollapsedRef.current.add(id);

        // Notify consumer to set open=false (this triggers a React state
        // update; the DOM will re-render shortly).
        onCollapseRef.current(id);

        // Compensate scrollTop in the next animation frame, after React has
        // committed the DOM change.  We capture `scrollContainerRef` here
        // because the IO root is the same element.
        const scrollEl = scrollContainerRef.current;
        requestAnimationFrame(() => {
          const newHeight = el.offsetHeight;
          const delta = oldHeight - newHeight;
          if (delta > 0 && scrollEl && scrollEl.scrollTop >= delta) {
            scrollEl.scrollTop -= delta;
          }
        });
      }
    },
    [],
  );

  // Create / recreate the IO when the scroll container element changes.
  useEffect(() => {
    if (!scrollContainer) {
      observerRef.current?.disconnect();
      observerRef.current = null;
      return;
    }

    // Disconnect previous observer if root changed.
    observerRef.current?.disconnect();
    observedElementsRef.current.clear();
    autoCollapsedRef.current.clear();

    const observer = new IntersectionObserver(handleIntersection, {
      root: scrollContainer,
      threshold: 0,
    });
    observerRef.current = observer;

    return () => {
      observer.disconnect();
      observerRef.current = null;
      observedElementsRef.current.clear();
    };
  }, [scrollContainer, handleIntersection]);

  // Sync observed elements with `wrapperRefs` on every render.
  useEffect(() => {
    const observer = observerRef.current;
    if (!observer) return;

    const prevObserved = observedElementsRef.current;
    const nextObserved = new Map<string, HTMLElement>();

    for (const entry of entries) {
      // Only observe completed + currently open entries (candidates for
      // auto-collapse).  Skip user-manually-opened entries.
      if (
        !entry.completed ||
        !entry.currentOpen ||
        userManuallyOpenedIds.has(entry.id) ||
        autoCollapsedRef.current.has(entry.id)
      ) {
        continue;
      }

      const el = wrapperRefs.get(entry.id);
      if (!el) continue;

      nextObserved.set(entry.id, el);

      if (!prevObserved.has(entry.id) || prevObserved.get(entry.id) !== el) {
        observer.observe(el);
      }
    }

    // Unobserve elements that are no longer candidates.
    for (const [id, el] of prevObserved) {
      if (!nextObserved.has(id) || nextObserved.get(id) !== el) {
        observer.unobserve(el);
      }
    }

    observedElementsRef.current = nextObserved;
  }, [entries, wrapperRefs, userManuallyOpenedIds]);
}

/**
 * Find the nearest scrollable ancestor of the given element.
 * Useful for locating the scroll container managed by `use-stick-to-bottom`.
 */
export function findScrollParent(el: HTMLElement | null): HTMLElement | null {
  let current = el?.parentElement ?? null;
  while (current) {
    const style = getComputedStyle(current);
    if (
      style.overflowY === "auto" ||
      style.overflowY === "scroll" ||
      style.overflow === "auto" ||
      style.overflow === "scroll"
    ) {
      return current;
    }
    current = current.parentElement;
  }
  return null;
}
