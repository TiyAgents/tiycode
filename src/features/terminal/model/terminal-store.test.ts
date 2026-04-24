import { afterEach, describe, expect, it, vi } from "vitest";
import { terminalStore } from "@/features/terminal/model/terminal-store";
import type { TerminalSessionDto } from "@/shared/types/api";

function createSession(overrides: Partial<TerminalSessionDto> = {}): TerminalSessionDto {
  return {
    sessionId: "sess-1",
    threadId: "t-1",
    workspaceId: "ws-1",
    shell: "/bin/zsh",
    cwd: "/home/user",
    cols: 80,
    rows: 24,
    status: "running",
    hasUnreadOutput: false,
    lastOutputAt: null,
    exitCode: null,
    createdAt: "2026-04-23T00:00:00Z",
    ...overrides,
  };
}

function resetStore() {
  // Reset by removing all sessions and clearing active thread
  terminalStore.setActiveThread(null);
  const state = terminalStore.getState();
  for (const threadId of Object.keys(state.sessionsByThreadId)) {
    terminalStore.removeSession(threadId);
  }
}

afterEach(() => {
  resetStore();
});

describe("terminalStore", () => {
  describe("getState", () => {
    it("returns initial state with null activeThreadId and empty sessions", () => {
      const state = terminalStore.getState();
      expect(state.activeThreadId).toBeNull();
      expect(Object.keys(state.sessionsByThreadId)).toHaveLength(0);
    });
  });

  describe("setActiveThread", () => {
    it("sets the active thread id", () => {
      terminalStore.setActiveThread("t-1");
      expect(terminalStore.getState().activeThreadId).toBe("t-1");
    });

    it("sets active thread to null", () => {
      terminalStore.setActiveThread("t-1");
      terminalStore.setActiveThread(null);
      expect(terminalStore.getState().activeThreadId).toBeNull();
    });
  });

  describe("upsertSession", () => {
    it("adds a new session keyed by threadId", () => {
      const session = createSession();
      terminalStore.upsertSession(session);

      const state = terminalStore.getState();
      expect(state.sessionsByThreadId["t-1"]).toBeDefined();
      expect(state.sessionsByThreadId["t-1"].sessionId).toBe("sess-1");
    });

    it("overwrites an existing session for the same threadId", () => {
      terminalStore.upsertSession(createSession({ sessionId: "sess-1" }));
      terminalStore.upsertSession(createSession({ sessionId: "sess-2" }));

      const state = terminalStore.getState();
      expect(state.sessionsByThreadId["t-1"].sessionId).toBe("sess-2");
    });

    it("handles multiple sessions for different threads", () => {
      terminalStore.upsertSession(createSession({ threadId: "t-1" }));
      terminalStore.upsertSession(createSession({ threadId: "t-2", sessionId: "sess-2" }));

      const state = terminalStore.getState();
      expect(Object.keys(state.sessionsByThreadId)).toHaveLength(2);
    });
  });

  describe("removeSession", () => {
    it("removes an existing session", () => {
      terminalStore.upsertSession(createSession());
      terminalStore.removeSession("t-1");

      const state = terminalStore.getState();
      expect(state.sessionsByThreadId["t-1"]).toBeUndefined();
    });

    it("clears activeThreadId when removing the active thread", () => {
      terminalStore.upsertSession(createSession());
      terminalStore.setActiveThread("t-1");
      terminalStore.removeSession("t-1");

      const state = terminalStore.getState();
      expect(state.activeThreadId).toBeNull();
    });

    it("does not clear activeThreadId when removing a non-active thread", () => {
      terminalStore.upsertSession(createSession({ threadId: "t-1" }));
      terminalStore.upsertSession(createSession({ threadId: "t-2", sessionId: "sess-2" }));
      terminalStore.setActiveThread("t-1");
      terminalStore.removeSession("t-2");

      expect(terminalStore.getState().activeThreadId).toBe("t-1");
    });

    it("clears activeThreadId when removing a non-existent session that matches active thread", () => {
      terminalStore.setActiveThread("t-orphan");
      terminalStore.removeSession("t-orphan");

      expect(terminalStore.getState().activeThreadId).toBeNull();
    });

    it("is a no-op for non-existent sessions when not the active thread", () => {
      const stateBefore = terminalStore.getState();
      terminalStore.removeSession("nonexistent");
      const stateAfter = terminalStore.getState();

      expect(stateAfter).toBe(stateBefore);
    });
  });

  describe("setSessionMeta", () => {
    it("patches an existing session", () => {
      terminalStore.upsertSession(createSession());
      terminalStore.setSessionMeta("t-1", { hasUnreadOutput: true });

      const session = terminalStore.getState().sessionsByThreadId["t-1"];
      expect(session.hasUnreadOutput).toBe(true);
      expect(session.sessionId).toBe("sess-1"); // unchanged
    });

    it("is a no-op when session does not exist", () => {
      const stateBefore = terminalStore.getState();
      terminalStore.setSessionMeta("nonexistent", { hasUnreadOutput: true });
      const stateAfter = terminalStore.getState();

      expect(stateAfter).toBe(stateBefore);
    });
  });

  describe("subscribe", () => {
    it("calls listener when state changes", () => {
      const listener = vi.fn();
      const unsubscribe = terminalStore.subscribe(listener);

      terminalStore.setActiveThread("t-1");
      expect(listener).toHaveBeenCalledTimes(1);

      unsubscribe();
    });

    it("stops calling listener after unsubscribe", () => {
      const listener = vi.fn();
      const unsubscribe = terminalStore.subscribe(listener);

      terminalStore.setActiveThread("t-1");
      expect(listener).toHaveBeenCalledTimes(1);

      unsubscribe();
      terminalStore.setActiveThread("t-2");
      expect(listener).toHaveBeenCalledTimes(1);
    });

    it("supports multiple listeners", () => {
      const listener1 = vi.fn();
      const listener2 = vi.fn();
      const unsub1 = terminalStore.subscribe(listener1);
      const unsub2 = terminalStore.subscribe(listener2);

      terminalStore.setActiveThread("t-1");
      expect(listener1).toHaveBeenCalledTimes(1);
      expect(listener2).toHaveBeenCalledTimes(1);

      unsub1();
      unsub2();
    });
  });
});
