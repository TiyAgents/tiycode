import { describe, expect, it } from "vitest";
import {
  countDiffLineChanges,
  getFileMutationPresentation,
  type SurfaceToolEntryLike,
} from "./file-mutation-presentation";

describe("countDiffLineChanges", () => {
  it("counts added and removed lines while ignoring diff metadata", () => {
    expect(
      countDiffLineChanges([
        "--- a/src/app.ts",
        "+++ b/src/app.ts",
        "@@ -1,3 +1,4 @@",
        " context",
        "-old line",
        "+new line",
        "+another line",
      ].join("\n")),
    ).toEqual({ linesAdded: 2, linesRemoved: 1 });
  });
});

describe("getFileMutationPresentation", () => {
  it("returns null for non file mutation tools", () => {
    expect(
      getFileMutationPresentation({
        name: "read",
        state: "output-available",
        input: { path: "src/app.ts" },
      }),
    ).toBeNull();
  });

  it("uses result diff and explicit line counts when available", () => {
    const presentation = getFileMutationPresentation({
      name: "edit",
      state: "output-available",
      input: { path: "src/app.ts", old_string: "old", new_string: "new" },
      result: {
        diff: "--- src/app.ts\n+++ src/app.ts\n-old\n+new",
        linesAdded: 10,
        linesRemoved: 4,
        path: "src/app.ts",
      },
    });

    expect(presentation).toMatchObject({
      actionLabel: "Edited",
      fileName: "app.ts",
      linesAdded: 10,
      linesRemoved: 4,
      path: "src/app.ts",
    });
  });

  it("builds a fallback diff for created edit tools", () => {
    const presentation = getFileMutationPresentation({
      name: "edit",
      state: "input-available",
      input: { path: "src/new.ts", old_string: "", new_string: "one\ntwo" },
    });

    expect(presentation?.actionLabel).toBe("Editing");
    expect(presentation?.diff).toContain("--- /dev/null");
    expect(presentation?.diff).toContain("+++ src/new.ts");
    expect(presentation?.linesAdded).toBe(2);
    expect(presentation?.linesRemoved).toBe(0);
    expect(presentation?.contentPreview).toBe("one\ntwo");
  });

  it("summarizes write tools from content input", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "output-available",
      input: { path: "/tmp/report.md", content: "a\nb\nc" },
      result: { created: true },
    };

    expect(getFileMutationPresentation(tool)).toMatchObject({
      actionLabel: "Created",
      contentPreview: "a\nb\nc",
      fileName: "report.md",
      linesAdded: 3,
      linesRemoved: 0,
    });
  });

  it("returns null when mutation path is missing", () => {
    expect(
      getFileMutationPresentation({
        name: "patch",
        state: "input-available",
        input: { new_string: "content" },
      }),
    ).toBeNull();
  });
});
