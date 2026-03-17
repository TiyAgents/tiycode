import { useSyncExternalStore } from "react";
import type { TerminalSessionDto } from "@/shared/types/api";

type TerminalStoreState = {
  activeThreadId: string | null;
  sessionsByThreadId: Record<string, TerminalSessionDto>;
};

type Listener = () => void;

const listeners = new Set<Listener>();

let state: TerminalStoreState = {
  activeThreadId: null,
  sessionsByThreadId: {},
};

const emit = () => {
  listeners.forEach((listener) => listener());
};

const setState = (updater: (current: TerminalStoreState) => TerminalStoreState) => {
  state = updater(state);
  emit();
};

export const terminalStore = {
  getState: () => state,
  subscribe(listener: Listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  setActiveThread(threadId: string | null) {
    setState((current) => ({
      ...current,
      activeThreadId: threadId,
    }));
  },
  setSessionMeta(threadId: string, patch: Partial<TerminalSessionDto>) {
    setState((current) => {
      const existing = current.sessionsByThreadId[threadId];
      if (!existing) {
        return current;
      }

      return {
        ...current,
        sessionsByThreadId: {
          ...current.sessionsByThreadId,
          [threadId]: {
            ...existing,
            ...patch,
          },
        },
      };
    });
  },
  upsertSession(session: TerminalSessionDto) {
    setState((current) => ({
      ...current,
      sessionsByThreadId: {
        ...current.sessionsByThreadId,
        [session.threadId]: session,
      },
    }));
  },
  removeSession(threadId: string) {
    setState((current) => {
      if (!(threadId in current.sessionsByThreadId)) {
        return current;
      }

      const next = { ...current.sessionsByThreadId };
      delete next[threadId];

      return {
        ...current,
        sessionsByThreadId: next,
      };
    });
  },
};

export function useTerminalStore<T>(selector: (value: TerminalStoreState) => T): T {
  return useSyncExternalStore(
    terminalStore.subscribe,
    () => selector(terminalStore.getState()),
    () => selector(terminalStore.getState()),
  );
}

