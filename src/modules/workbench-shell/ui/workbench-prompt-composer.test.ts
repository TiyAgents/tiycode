import { describe, expect, it } from "vitest";

import {
  markAndShouldRestoreInitialAttachmentData,
  type ComposerInitialAttachmentRestoreState,
} from "./workbench-prompt-composer";

const attachment = {
  dataUrl: "data:image/png;base64,AA==",
  id: "attachment-1",
  mediaType: "image/png",
  name: "image.png",
};

describe("markAndShouldRestoreInitialAttachmentData", () => {
  it("marks empty initial data as already restored", () => {
    const state: ComposerInitialAttachmentRestoreState = { hasRestored: false };

    expect(markAndShouldRestoreInitialAttachmentData(state, [])).toBe(false);
    expect(state.hasRestored).toBe(true);
    expect(markAndShouldRestoreInitialAttachmentData(state, [attachment])).toBe(false);
  });

  it("restores non-empty initial data only once", () => {
    const state: ComposerInitialAttachmentRestoreState = { hasRestored: false };

    expect(markAndShouldRestoreInitialAttachmentData(state, [attachment])).toBe(true);
    expect(state.hasRestored).toBe(true);
    expect(markAndShouldRestoreInitialAttachmentData(state, [attachment])).toBe(false);
  });

  it("treats undefined initial data as restored", () => {
    const state: ComposerInitialAttachmentRestoreState = { hasRestored: false };

    expect(markAndShouldRestoreInitialAttachmentData(state, undefined)).toBe(false);
    expect(state.hasRestored).toBe(true);
  });
});
