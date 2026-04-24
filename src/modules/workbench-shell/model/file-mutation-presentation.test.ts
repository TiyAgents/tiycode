import { describe, it, expect } from "vitest";
import {
  countDiffLineChanges,
  getFileMutationPresentation,
} from "@/modules/workbench-shell/model/file-mutation-presentation";
import type { SurfaceToolEntryLike } from "@/modules/workbench-shell/model/file-mutation-presentation";

// ---------------------------------------------------------------------------
// countDiffLineChanges
// ---------------------------------------------------------------------------

describe("countDiffLineChanges", () => {
  it("counts simple additions and removals", () => {
    const diff = [
      "--- a/file.ts",
      "+++ b/file.ts",
      "@@ -1,3 +1,4 @@",
      " unchanged",
      "-removed line",
      "+added line 1",
      "+added line 2",
    ].join("\n");

    const result = countDiffLineChanges(diff);
    expect(result.linesAdded).toBe(2);
    expect(result.linesRemoved).toBe(1);
  });

  it("returns zeros for empty diff", () => {
    expect(countDiffLineChanges("")).toEqual({ linesAdded: 0, linesRemoved: 0 });
  });

  it("ignores diff headers (+++ --- @@)", () => {
    const diff = "--- a/old\n+++ b/new\n@@ -1 +1 @@\n+added";
    const result = countDiffLineChanges(diff);
    expect(result.linesAdded).toBe(1);
    expect(result.linesRemoved).toBe(0);
  });

  it("handles only additions", () => {
    const diff = "+line1\n+line2\n+line3";
    const result = countDiffLineChanges(diff);
    expect(result.linesAdded).toBe(3);
    expect(result.linesRemoved).toBe(0);
  });

  it("handles only removals", () => {
    const diff = "-line1\n-line2";
    const result = countDiffLineChanges(diff);
    expect(result.linesAdded).toBe(0);
    expect(result.linesRemoved).toBe(2);
  });

  it("handles context lines (no prefix)", () => {
    const diff = " context\n+added\n context2";
    const result = countDiffLineChanges(diff);
    expect(result.linesAdded).toBe(1);
    expect(result.linesRemoved).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// getFileMutationPresentation
// ---------------------------------------------------------------------------

describe("getFileMutationPresentation", () => {
  it("returns null for non-file-mutation tool", () => {
    const tool: SurfaceToolEntryLike = {
      name: "search",
      state: "output-available",
      input: { query: "foo" },
      result: {},
    };
    expect(getFileMutationPresentation(tool)).toBeNull();
  });

  it("returns null when no path is available", () => {
    const tool: SurfaceToolEntryLike = {
      name: "edit",
      state: "output-available",
      input: {},
      result: {},
    };
    expect(getFileMutationPresentation(tool)).toBeNull();
  });

  it("handles edit tool with complete data", () => {
    const tool: SurfaceToolEntryLike = {
      name: "edit",
      state: "output-available",
      input: { path: "/src/app.ts", old_string: "old", new_string: "new" },
      result: { path: "/src/app.ts", diff: "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new", linesAdded: 1, linesRemoved: 1 },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result).not.toBeNull();
    expect(result.path).toBe("/src/app.ts");
    expect(result.fileName).toBe("app.ts");
    expect(result.linesAdded).toBe(1);
    expect(result.linesRemoved).toBe(1);
    expect(result.actionLabel).toBe("Edited");
  });

  it("detects created file via result.created", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "output-available",
      input: { path: "/new.ts", content: "hello\nworld" },
      result: { path: "/new.ts", created: true },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.actionLabel).toBe("Created");
  });

  it("detects created file via empty old_string for edit", () => {
    const tool: SurfaceToolEntryLike = {
      name: "edit",
      state: "output-available",
      input: { path: "/new.ts", old_string: "", new_string: "content" },
      result: { path: "/new.ts" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.actionLabel).toBe("Created");
  });

  it("returns 'Editing' for in-progress edit", () => {
    const tool: SurfaceToolEntryLike = {
      name: "edit",
      state: "input-streaming",
      input: { path: "/src/app.ts", old_string: "a", new_string: "b" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.actionLabel).toBe("Editing");
  });

  it("returns 'Writing' for in-progress write", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "input-streaming",
      input: { path: "/src/app.ts", content: "hello" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.actionLabel).toBe("Writing");
  });

  it("falls back to input-based diff when result has no diff", () => {
    const tool: SurfaceToolEntryLike = {
      name: "edit",
      state: "output-available",
      input: { path: "/f.ts", old_string: "old", new_string: "new" },
      result: { path: "/f.ts" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.diff).not.toBeNull();
    expect(result.diff).toContain("-old");
    expect(result.diff).toContain("+new");
  });

  it("computes line counts from diff when result counts absent", () => {
    const tool: SurfaceToolEntryLike = {
      name: "edit",
      state: "output-available",
      input: { path: "/f.ts", old_string: "a\nb", new_string: "c" },
      result: { path: "/f.ts" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.linesAdded).toBe(1);
    expect(result.linesRemoved).toBe(2);
  });

  it("extracts fileName from path", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "output-available",
      input: { path: "/a/b/c/deep.rs", content: "fn main() {}" },
      result: { path: "/a/b/c/deep.rs" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.fileName).toBe("deep.rs");
  });

  it("uses path as fileName when no separator", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "output-available",
      input: { path: "single", content: "hello" },
      result: { path: "single" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.fileName).toBe("single");
  });

  it("handles write tool with content", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "output-available",
      input: { path: "/f.ts", content: "line1\nline2" },
      result: { path: "/f.ts" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.contentPreview).toBe("line1\nline2");
    expect(result.linesAdded).toBe(2);
  });

  it("handles patch tool the same as edit", () => {
    const tool: SurfaceToolEntryLike = {
      name: "patch",
      state: "output-available",
      input: { path: "/f.ts", old_string: "x", new_string: "y" },
      result: { path: "/f.ts" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.diff).toContain("-x");
    expect(result.diff).toContain("+y");
  });

  it("reads path from input when result has no path", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "output-available",
      input: { path: "/from-input.ts", content: "ok" },
      result: {},
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.path).toBe("/from-input.ts");
  });

  it("returns null diff for write tool with no result diff", () => {
    const tool: SurfaceToolEntryLike = {
      name: "write",
      state: "output-available",
      input: { path: "/f.ts", content: "abc" },
      result: { path: "/f.ts" },
    };
    const result = getFileMutationPresentation(tool)!;
    expect(result.diff).toBeNull();
  });
});
