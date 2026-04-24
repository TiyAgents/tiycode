import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));

// mapComposerAttachments is the only exported function from workbench-prompt-composer
// It maps FileUIPart array to internal ComposerAttachment format
import { mapComposerAttachments } from "@/modules/workbench-shell/ui/workbench-prompt-composer";

describe("mapComposerAttachments", () => {
  it("returns empty array for empty input", () => {
    const result = mapComposerAttachments([]);
    // Should return an array
    expect(Array.isArray(result)).toBe(true);
    expect(result).toHaveLength(0);
  });

  it("returns array with same length as input", () => {
    const files = [
      { type: "file" as const, data: new Uint8Array([1]), mimeType: "image/png", filename: "a.png" },
      { type: "file" as const, data: new Uint8Array([2]), mimeType: "text/plain", filename: "b.txt" },
    ];
    const result = mapComposerAttachments(files);
    expect(result).toHaveLength(2);
  });
});
