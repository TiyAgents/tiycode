import { describe, it, expect, beforeEach } from "vitest";
import {
  composerStore,
  setNewThreadValue,
  setNewThreadRunMode,
  setNewThreadReferencedFiles,
  setNewThreadAttachmentData,
  setDraft,
  getDraft,
  removeDraft,
  setComposerError,
  clearComposerError,
  clearNewThreadComposer,
  type SerializableAttachment,
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
      setNewThreadReferencedFiles([{ name: "test.ts", path: "/test.ts", parentPath: "/" }]);
      setNewThreadAttachmentData([{ id: "1", name: "test.png", mediaType: "image/png", dataUrl: "data:image/png;base64," }]);
      setComposerError("an error");
      clearNewThreadComposer();
      const state = composerStore.getState();
      expect(state.newThreadValue).toBe("");
      expect(state.newThreadReferencedFiles).toEqual([]);
      expect(state.newThreadAttachmentData).toEqual([]);
      expect(state.error).toBeNull();
    });
  });

  describe("drafts", () => {
    it("should set and get drafts", () => {
      setDraft("thread-1", { text: "draft content", referencedFiles: [], attachmentData: [] });
      expect(getDraft("thread-1").text).toBe("draft content");
      expect(getDraft("thread-2").text).toBe(""); // non-existent → empty
    });

    it("should remove drafts", () => {
      setDraft("thread-1", { text: "content", referencedFiles: [], attachmentData: [] });
      removeDraft("thread-1");
      expect(getDraft("thread-1").text).toBe("");
    });

    it("should handle removing non-existent draft silently", () => {
      removeDraft("non-existent");
      // Should not throw
    });

    it("should support multiple thread drafts", () => {
      setDraft("t1", { text: "a", referencedFiles: [], attachmentData: [] });
      setDraft("t2", { text: "b", referencedFiles: [], attachmentData: [] });
      expect(getDraft("t1").text).toBe("a");
      expect(getDraft("t2").text).toBe("b");
    });

    it("setDraft should update existing draft", () => {
      setDraft("t1", { text: "first", referencedFiles: [], attachmentData: [] });
      setDraft("t1", { text: "second", referencedFiles: [], attachmentData: [] });
      expect(getDraft("t1").text).toBe("second");
    });

    it("should store and retrieve referencedFiles", () => {
      const files = [{ name: "app.ts", path: "src/app.ts", parentPath: "src" }];
      setDraft("t1", { text: "hello", referencedFiles: files, attachmentData: [] });
      expect(getDraft("t1").referencedFiles).toEqual(files);
    });

    it("should store and retrieve attachmentData", () => {
      const attachments: SerializableAttachment[] = [
        { id: "1", name: "photo.png", mediaType: "image/png", dataUrl: "data:image/png;base64,abc" },
      ];
      setDraft("t1", { text: "check this out", referencedFiles: [], attachmentData: attachments });
      expect(getDraft("t1").attachmentData).toEqual(attachments);
    });

    it("should update attachmentData independently of other draft fields", () => {
      setDraft("t1", { text: "hello", referencedFiles: [], attachmentData: [] });
      const existing = getDraft("t1");
      const attachments: SerializableAttachment[] = [
        { id: "2", name: "doc.pdf", mediaType: "application/pdf", dataUrl: "data:application/pdf;base64,xyz" },
      ];
      setDraft("t1", { ...existing, attachmentData: attachments });
      expect(getDraft("t1").text).toBe("hello");
      expect(getDraft("t1").attachmentData).toEqual(attachments);
    });

    it("getDraft should handle legacy string drafts", () => {
      // Simulate a legacy string stored in drafts (force-set via store API)
      composerStore.setState({ drafts: { legacy: "old text" as any } });
      const draft = getDraft("legacy");
      expect(draft.text).toBe("old text");
      expect(draft.referencedFiles).toEqual([]);
      expect(draft.attachmentData).toEqual([]);
    });

    it("getDraft should handle drafts missing attachmentData (backward compat)", () => {
      // Simulate an old object draft that lacks attachmentData
      composerStore.setState({
        drafts: {
          oldObj: { text: "old draft", referencedFiles: [] } as any,
        },
      });
      const draft = getDraft("oldObj");
      expect(draft.text).toBe("old draft");
      expect(draft.referencedFiles).toEqual([]);
      expect(draft.attachmentData).toEqual([]);
    });

    it("getDraft should return empty attachmentData for non-existent thread", () => {
      const draft = getDraft("nonexistent");
      expect(draft.attachmentData).toEqual([]);
    });
  });

  describe("new thread referenced files", () => {
    it("should set and retrieve new thread referenced files", () => {
      const files = [{ name: "app.ts", path: "src/app.ts", parentPath: "src" }];
      setNewThreadReferencedFiles(files);
      expect(composerStore.getState().newThreadReferencedFiles).toEqual(files);
    });
  });

  describe("new thread attachment data", () => {
    it("should set and retrieve new thread attachment data", () => {
      const data = [{ id: "1", name: "test.png", mediaType: "image/png", dataUrl: "data:image/png;base64," }];
      setNewThreadAttachmentData(data);
      expect(composerStore.getState().newThreadAttachmentData).toEqual(data);
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
