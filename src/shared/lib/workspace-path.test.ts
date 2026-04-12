import { describe, expect, it } from "vitest";
import {
  buildWorkspacePathKeys,
  isSameWorkspacePath,
  normalizeWorkspacePath,
} from "@/shared/lib/workspace-path";

describe("normalizeWorkspacePath", () => {
  it("normalizes POSIX paths and resolves dot segments", () => {
    expect(normalizeWorkspacePath("/Users/Buddy//repo/./src/../README.md"))
      .toBe("/Users/Buddy/repo/README.md");
    expect(normalizeWorkspacePath("../../repo/./src/..")).toBe("../../repo");
    expect(normalizeWorkspacePath("/../../repo")).toBe("/repo");
  });

  it("normalizes Windows drive-letter paths case-insensitively", () => {
    expect(normalizeWorkspacePath("C:\\Users\\Buddy\\Repo\\..\\Docs\\"))
      .toBe("c:/users/buddy/docs");
    expect(normalizeWorkspacePath("c:/Users/BUDDY/docs/./guide.md"))
      .toBe("c:/users/buddy/docs/guide.md");
  });

  it("normalizes UNC paths without escaping the protected server/share segments", () => {
    expect(normalizeWorkspacePath("///SERVER/Share//Folder/../File.txt"))
      .toBe("//server/share/file.txt");
    expect(normalizeWorkspacePath("//SERVER/Share/path/../../.."))
      .toBe("//server/share");
  });
});

describe("workspace path helpers", () => {
  it("treats equivalent paths as the same workspace", () => {
    expect(isSameWorkspacePath(
      "C:\\Users\\Buddy\\Repo",
      "c:/users/buddy/repo/./",
    )).toBe(true);
    expect(isSameWorkspacePath(
      "//SERVER/Share/Repo",
      "///server/share/repo",
    )).toBe(true);
  });

  it("deduplicates normalized workspace path keys", () => {
    expect(buildWorkspacePathKeys(
      "/tmp/workspace",
      "/tmp//workspace/",
      "/tmp/workspace/./",
      null,
      undefined,
      "",
    )).toEqual(["/tmp/workspace"]);
  });
});
