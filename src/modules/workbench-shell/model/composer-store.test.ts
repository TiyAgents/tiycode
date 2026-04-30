import { describe, it, expect, beforeEach } from "vitest";
import {
  composerStore,
  setNewThreadValue,
  setNewThreadRunMode,
  setDraft,
  getDraft,
  removeDraft,
  setComposerError,
  clearComposerError,
  clearNewThreadComposer,
} from "./composer-store";

beforeEach(() => {
  composerStore.reset();
});

describe("composerStore", () => {
  describe("new thread composer", () => {
    it("should set new thread value", () => {
      setNewThreadValue("hello world");
      expect(composerStore.getState().newThreadValue).toBe("hello world");
    });

    it("should set new thread run mode", () => {
      setNewThreadRunMode("plan");
      expect(composerStore.getState().newThreadRunMode).toBe("plan");
      setNewThreadRunMode("default");
      expect(composerStore.getState().newThreadRunMode).toBe("default");
    });

    it("should clear new thread composer", () => {
      setNewThreadValue("some value");
      setComposerError("an error");
      clearNewThreadComposer();
      const state = composerStore.getState();
      expect(state.newThreadValue).toBe("");
      expect(state.error).toBeNull();
      // Run mode should NOT be cleared (it's not part of clearNewThreadComposer)
    });
  });

  describe("drafts", () => {
    it("should set and get drafts", () => {
      setDraft("thread-1", "draft content");
      expect(getDraft("thread-1")).toBe("draft content");
      expect(getDraft("thread-2")).toBe(""); // non-existent → empty string
    });

    it("should remove drafts", () => {
      setDraft("thread-1", "content");
      removeDraft("thread-1");
      expect(getDraft("thread-1")).toBe("");
    });

    it("should handle removing non-existent draft silently", () => {
      removeDraft("non-existent");
      // Should not throw
    });

    it("should support multiple thread drafts", () => {
      setDraft("t1", "a");
      setDraft("t2", "b");
      expect(getDraft("t1")).toBe("a");
      expect(getDraft("t2")).toBe("b");
    });

    it("setDraft should update existing draft", () => {
      setDraft("t1", "first");
      setDraft("t1", "second");
      expect(getDraft("t1")).toBe("second");
    });
  });

  describe("error", () => {
    it("should set and clear composer error", () => {
      setComposerError("something went wrong");
      expect(composerStore.getState().error).toBe("something went wrong");
      clearComposerError();
      expect(composerStore.getState().error).toBeNull();
    });

    it("should set error to null", () => {
      setComposerError("error");
      setComposerError(null);
      expect(composerStore.getState().error).toBeNull();
    });
  });
});
