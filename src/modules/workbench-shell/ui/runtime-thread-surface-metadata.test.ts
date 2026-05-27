import { describe, expect, it } from "vitest";

import { parseRuntimeQueueComposerMetadata } from "./runtime-thread-surface-metadata";

describe("parseRuntimeQueueComposerMetadata", () => {
  it("parses display text, effective prompt, and referenced files", () => {
    expect(parseRuntimeQueueComposerMetadata({
      composer: {
        kind: "command",
        displayText: "/init docs",
        effectivePrompt: "Generate docs",
        referencedFiles: [
          { name: "app.ts", path: "src/app.ts", parentPath: "src" },
          { name: "bad" },
        ],
      },
    })).toEqual({
      kind: "command",
      displayText: "/init docs",
      effectivePrompt: "Generate docs",
      referencedFiles: [{ name: "app.ts", path: "src/app.ts", parentPath: "src" }],
    });
  });

  it("returns null for invalid composer metadata", () => {
    expect(parseRuntimeQueueComposerMetadata(null)).toBeNull();
    expect(parseRuntimeQueueComposerMetadata({ composer: { kind: "unknown" } })).toBeNull();
  });
});
