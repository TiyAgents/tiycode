import { describe, expect, it } from "vitest";
import type { ThreadSummaryDto } from "@/shared/types/api";
import {
  buildGitDiffPreview,
  buildGitSplitDiffRows,
  formatThreadTimeLabel,
  buildWorkspaceItemsFromDtos,
  clearActiveThreads,
  getActiveThread,
  activateThread,
  buildThreadTitle,
  mergeRecentProjects,
  buildProjectOptionFromPath,
  formatProjectPathLabel,
  buildInitialWorkspaces,
  buildWorkspaceThreadItem,
  buildProjectOptionFromWorkspace,
  isHelperOwnedTool,
  buildSnapshotHelperToolSummary,
  sortWorkspacesWithWorktrees,
} from "./helpers";
import type { GitChangeFile, WorkspaceItem, ProjectOption } from "./types";

function thread(overrides: Partial<ThreadSummaryDto> = {}): ThreadSummaryDto {
  return {
    id: "thread-1",
    workspaceId: "workspace-1",
    profileId: null,
    title: "Test thread",
    status: "idle",
    lastActiveAt: "2026-04-25T10:00:00Z",
    createdAt: "2026-04-25T10:00:00Z",
    ...overrides,
  };
}

function project(id: string, name: string, path: string): ProjectOption {
  return { id, name, path, lastOpenedLabel: "" };
}

function threadItem(id: string, name: string, active = false): { id: string; name: string; profileId?: string | null; time: string; active: boolean; status: "running" | "completed" | "needs-reply" | "failed" | "interrupted" } {
  return { id, name, profileId: null, time: "1m", active, status: "completed" };
}

function workspaceItem(id: string, threads: ReturnType<typeof threadItem>[]): WorkspaceItem {
  return {
    id,
    name: `Workspace ${id}`,
    defaultOpen: false,
    threads,
    kind: "standalone",
    path: "/path",
    createdAt: "2026-04-25T00:00:00Z",
  };
}

function gitFile(status: "M" | "A" | "D"): GitChangeFile {
  return {
    id: "file-1",
    path: "src/app.ts",
    status,
    icon: "ts",
    summary: "Update app",
    initialStaged: false,
  };
}

describe("helpers: git diff", () => {
  it("builds unified diff preview for added files", () => {
    const preview = buildGitDiffPreview(gitFile("A"));
    expect(preview.meta[1]).toContain("new file mode");
    expect(preview.meta[2]).toContain("--- /dev/null");
    expect(preview.lines.every((l) => l.kind === "add")).toBe(true);
  });

  it("builds unified diff preview for deleted files", () => {
    const preview = buildGitDiffPreview(gitFile("D"));
    expect(preview.meta[1]).toContain("deleted file mode");
    expect(preview.lines.every((l) => l.kind === "remove")).toBe(true);
  });

  it("builds split diff rows for added, deleted, and modified files", () => {
    expect(buildGitSplitDiffRows(gitFile("A")).every((r) => r.kind === "add")).toBe(true);
    expect(buildGitSplitDiffRows(gitFile("D")).every((r) => r.kind === "remove")).toBe(true);
    const rows = buildGitSplitDiffRows(gitFile("M"));
    const kinds = new Set(rows.map((r) => r.kind));
    expect(kinds.has("modified") || kinds.has("add") || kinds.has("remove") || kinds.has("context")).toBe(true);
  });
});

describe("helpers: thread & workspace", () => {
  it("maps API workspace DTOs to workspace items", () => {
    const threads = [thread()];
    const items = buildWorkspaceItemsFromDtos(
      [{ id: "ws-1", name: "Ws", kind: "standalone", threads, createdAt: "2026-04-25T00:00:00Z" }] as any,
      { "ws-1": threads } as any,
      null,
      "en",
    );

    expect(items[0].id).toBe("ws-1");
    expect(items[0].threads).toHaveLength(1);
    expect(items[0].threads[0].status).toBe("completed");
  });

  it("clears active flag from all threads", () => {
    const ws = workspaceItem("ws-1", [
      threadItem("t-1", "Thread 1", true),
      threadItem("t-2", "Thread 2"),
    ]);
    const cleared = clearActiveThreads([ws]);

    expect(cleared[0].threads.every((t) => !t.active)).toBe(true);
  });

  it("returns null when no thread is active", () => {
    const ws = workspaceItem("ws-1", [threadItem("t-1", "Thread 1")]);
    expect(getActiveThread([ws])).toBeNull();
  });

  it("activates the target thread by id", () => {
    const ws = workspaceItem("ws-1", [
      threadItem("t-1", "Thread 1"),
      threadItem("t-2", "Thread 2"),
    ]);
    const updated = activateThread([ws], "t-2");

    expect(updated[0].threads.find((t) => t.id === "t-2")?.active).toBe(true);
    expect(updated[0].threads.find((t) => t.id === "t-1")?.active).toBe(false);
  });
});

describe("helpers: time formatting", () => {
  const now = new Date("2026-04-25T12:00:00Z").getTime();

  it("returns empty string for null or undefined", () => {
    expect(formatThreadTimeLabel(null, "en", now)).toBe("");
    expect(formatThreadTimeLabel(undefined, "en", now)).toBe("");
    expect(formatThreadTimeLabel("", "en", now)).toBe("");
  });

  it('returns "Just now" for timestamps within 1 minute', () => {
    const recent = new Date(now - 30_000).toISOString();
    expect(formatThreadTimeLabel(recent, "en", now)).toBe("Just now");
  });

  it("formats minutes, hours, days, and weeks", () => {
    expect(formatThreadTimeLabel(new Date(now - 5 * 60_000).toISOString(), "en", now)).toBe("5m");
    expect(formatThreadTimeLabel(new Date(now - 3 * 3600_000).toISOString(), "en", now)).toBe("3h");
    expect(formatThreadTimeLabel(new Date(now - 2 * 86400_000).toISOString(), "en", now)).toBe("2d");
    expect(formatThreadTimeLabel(new Date(now - 14 * 86400_000).toISOString(), "en", now)).toBe("2w");
  });

  it("returns empty string for invalid timestamps", () => {
    expect(formatThreadTimeLabel("not-a-date", "en", now)).toBe("");
  });
});

describe("helpers: thread title", () => {
  it("returns trimmed prompt under 30 chars", () => {
    expect(buildThreadTitle("  hello world  ")).toBe("hello world");
  });

  it("truncates prompts over 30 chars with ellipsis", () => {
    const long = "a".repeat(35);
    const result = buildThreadTitle(long);
    expect(result.length).toBe(33);
    expect(result.endsWith("...")).toBe(true);
  });
});

describe("helpers: mergeRecentProjects", () => {
  it("places the new project at the front and deduplicates by id and name+path", () => {
    const current = [project("1", "A", "/a"), project("2", "B", "/b")];
    const next = project("3", "C", "/c");

    const merged = mergeRecentProjects(current, next);
    expect(merged[0].id).toBe("3");
    expect(merged).toHaveLength(3);

    const duplicate = project("1", "A", "/a");
    const deduped = mergeRecentProjects(current, duplicate);
    expect(deduped).toHaveLength(2);
    expect(deduped[0].id).toBe("1");
    expect(deduped.find((p) => p.id === "1")?.name).toBe("A");
  });

  it("caps the merged list at 6 entries", () => {
    const current = Array.from({ length: 6 }, (_, i) => project(`p${i}`, `Project ${i}`, `/p${i}`));
    const next = project("new", "New", "/new");
    const merged = mergeRecentProjects(current, next);
    expect(merged).toHaveLength(6);
  });
});

describe("helpers: buildProjectOptionFromPath", () => {
  it("returns null for null path", () => {
    expect(buildProjectOptionFromPath(null)).toBeNull();
  });

  it("normalizes backslashes and trailing slashes", () => {
    const result = buildProjectOptionFromPath("C:\\Users\\John\\project\\");
    expect(result).not.toBeNull();
    expect(result!.path).toBe("C:/Users/John/project");
    expect(result!.name).toBe("project");
  });

  it("normalizes id from path and folder name", () => {
    const result = buildProjectOptionFromPath("/home/user/my-repo");
    expect(result).not.toBeNull();
    expect(result!.id).toMatch(/^my-repo-/);
  });
});

describe("helpers: formatProjectPathLabel", () => {
  it("returns normalized path when 4 or fewer segments", () => {
    expect(formatProjectPathLabel("/a/b/c")).toBe("/a/b/c");
    expect(formatProjectPathLabel("/a/b/c/d")).toBe("/a/b/c/d");
  });

  it("truncates to last 4 segments with .../", () => {
    const result = formatProjectPathLabel("/a/b/c/d/e/f/g");
    expect(result).toBe(".../d/e/f/g");
  });

  it("normalizes backslashes before counting segments", () => {
    const result = formatProjectPathLabel("C:\\a\\b\\c\\d\\e");
    expect(result).toBe(".../b/c/d/e");
  });
});

describe("helpers: getDiffTemplate (via buildGitDiffPreview)", () => {
  it("returns .css template for .css files", () => {
    const preview = buildGitDiffPreview({ id: "f1", path: "styles.css", status: "M", icon: "css", summary: "", initialStaged: false });
    expect(preview.lines.some((l) => l.text.includes("tracked-row"))).toBe(true);
  });

  it("returns .json template for .json files", () => {
    const preview = buildGitDiffPreview({ id: "f1", path: "config.json", status: "M", icon: "json", summary: "", initialStaged: false });
    expect(preview.lines.some((l) => l.text.includes("beforeDevCommand"))).toBe(true);
    expect(preview.lines.some((l) => l.text.includes("sourceControlPreview"))).toBe(true);
  });

  it("returns .md template for .md files", () => {
    const preview = buildGitDiffPreview({ id: "f1", path: "README.md", status: "M", icon: "readme", summary: "", initialStaged: false });
    expect(preview.lines.some((l) => l.text.includes("Tiy Desktop"))).toBe(true);
    expect(preview.lines.some((l) => l.text.includes("Diff preview overlay"))).toBe(true);
  });

  it("returns default template for unknown extensions", () => {
    const preview = buildGitDiffPreview({ id: "f1", path: "some.py", status: "M", icon: "file", summary: "", initialStaged: false });
    expect(preview.lines.some((l) => l.text.includes("panelState"))).toBe(true);
  });
});

describe("helpers: sortWorkspacesWithWorktrees", () => {
  it("sorts by defaultOpen first, then name, then kind, then createdAt", () => {
    const items = [
      { id: "b", name: "a", kind: "repo" as const, createdAt: "2026-01-01T00:00:00Z" },
      { id: "a", name: "a", kind: "repo" as const, defaultOpen: true, createdAt: "2026-01-01T00:00:00Z" },
      { id: "c", name: "b", kind: "repo" as const, createdAt: "2026-01-01T00:00:00Z" },
    ];
    const result = sortWorkspacesWithWorktrees(items);
    expect(result.map((i) => i.id)).toEqual(["a", "b", "c"]);
  });

  it("groups worktrees under their parent repo", () => {
    const items = [
      { id: "repo-1", name: "alpha", kind: "repo" as const, createdAt: "2026-01-01T00:00:00Z" },
      { id: "wt-1", name: "alpha", kind: "worktree" as const, parentWorkspaceId: "repo-1", createdAt: "2026-01-01T00:00:00Z" },
      { id: "repo-2", name: "beta", kind: "repo" as const, createdAt: "2026-01-01T00:00:00Z" },
    ];
    const result = sortWorkspacesWithWorktrees(items);
    // alpha (repo-1) first, then its worktree, then beta (repo-2)
    const ids = result.map((i) => i.id);
    expect(ids[0]).toBe("repo-1");
    expect(ids[1]).toBe("wt-1");
    expect(ids[2]).toBe("repo-2");
  });

  it("handles orphan worktrees at the end", () => {
    const items = [
      { id: "wt-orphan", name: "z", kind: "worktree" as const, parentWorkspaceId: "missing-parent", createdAt: "2026-01-01T00:00:00Z" },
      { id: "repo-1", name: "a", kind: "repo" as const, createdAt: "2026-01-01T00:00:00Z" },
    ];
    const result = sortWorkspacesWithWorktrees(items);
    expect(result.map((i) => i.id)).toEqual(["repo-1", "wt-orphan"]);
  });

  it("handles empty array", () => {
    expect(sortWorkspacesWithWorktrees([])).toEqual([]);
  });

  it("sorts by createdAt descending as tiebreaker", () => {
    const items = [
      { id: "older", name: "same", kind: "repo" as const, createdAt: "2026-01-01T00:00:00Z" },
      { id: "newer", name: "same", kind: "repo" as const, createdAt: "2026-04-01T00:00:00Z" },
    ];
    const result = sortWorkspacesWithWorktrees(items);
    expect(result[0].id).toBe("newer");
  });
});

describe("helpers: buildInitialWorkspaces", () => {
  it("maps fixture workspace seeds to workspace items with thread ids", () => {
    const workspaces = buildInitialWorkspaces();
    expect(workspaces.length).toBeGreaterThan(0);
    for (const ws of workspaces) {
      expect(ws.threads.every((t) => t.id.startsWith(ws.id))).toBe(true);
      expect(ws.threads.every((t) => !t.active)).toBe(true);
    }
  });
});

describe("helpers: buildWorkspaceThreadItem", () => {
  it("builds a workspace thread item from DTO", () => {
    const dto: ThreadSummaryDto = {
      id: "t1",
      workspaceId: "ws1",
      profileId: "p1",
      title: "Hello world",
      status: "running",
      lastActiveAt: "2026-04-25T11:59:00Z",
      createdAt: "2026-04-25T10:00:00Z",
    };
    const item = buildWorkspaceThreadItem(dto, null, "en");
    expect(item.id).toBe("t1");
    expect(item.profileId).toBe("p1");
    expect(item.name).toBe("Hello world");
    expect(item.active).toBe(false);
    expect(item.status).toBe("running");
  });

  it("uses fallback title for empty thread title", () => {
    const dto: ThreadSummaryDto = {
      id: "t1",
      workspaceId: "ws1",
      profileId: null,
      title: "",
      status: "idle",
      lastActiveAt: "2026-04-25T11:59:00Z",
      createdAt: "2026-04-25T10:00:00Z",
    };
    const item = buildWorkspaceThreadItem(dto, null, "en");
    expect(item.name).toBe("New thread");
  });

  it("marks thread active when id matches", () => {
    const dto: ThreadSummaryDto = {
      id: "t1",
      workspaceId: "ws1",
      profileId: null,
      title: "Test",
      status: "idle",
      lastActiveAt: "2026-04-25T11:59:00Z",
      createdAt: "2026-04-25T10:00:00Z",
    };
    const item = buildWorkspaceThreadItem(dto, "t1", "en");
    expect(item.active).toBe(true);
  });

  it("falls back to createdAt when lastActiveAt is empty", () => {
    const dto: ThreadSummaryDto = {
      id: "t1",
      workspaceId: "ws1",
      profileId: null,
      title: "Test",
      status: "idle",
      lastActiveAt: "",
      createdAt: "2026-04-25T10:00:00Z",
    };
    const item = buildWorkspaceThreadItem(dto, null, "en");
    // Should use createdAt (a valid date) rather than empty lastActiveAt
    expect(item.time).toBeTruthy();
    expect(item.time).not.toBe("");
  });
});

describe("helpers: buildProjectOptionFromWorkspace", () => {
  it("builds ProjectOption from workspace DTO", () => {
    const ws: any = {
      id: "ws-1",
      name: "My Project",
      path: "/home/user/project",
      canonicalPath: "/home/user/project",
      kind: "repo",
      parentWorkspaceId: null,
      worktreeName: null,
      branch: "main",
    };
    const result = buildProjectOptionFromWorkspace(ws);
    expect(result.id).toBe("ws-1");
    expect(result.name).toBe("My Project");
    expect(result.path).toBe("/home/user/project");
    expect(result.kind).toBe("repo");
    expect(result.branch).toBe("main");
  });

  it("uses canonicalPath over path", () => {
    const ws: any = {
      id: "ws-1",
      name: "P",
      path: "/old",
      canonicalPath: "/new",
      kind: "standalone",
    };
    const result = buildProjectOptionFromWorkspace(ws);
    expect(result.path).toBe("/new");
  });

  it("generates worktreeHash from worktreeName", () => {
    const ws: any = {
      id: "ws-1",
      name: "P",
      path: "/p",
      canonicalPath: "/p",
      kind: "worktree",
      worktreeName: "abcdef123456",
    };
    const result = buildProjectOptionFromWorkspace(ws);
    expect(result.worktreeHash).toBe("abcdef");
  });

  it("uses custom lastOpenedLabel when provided", () => {
    const ws: any = {
      id: "ws-1",
      name: "P",
      path: "/p",
      canonicalPath: "/p",
      kind: "standalone",
    };
    const result = buildProjectOptionFromWorkspace(ws, "2d ago");
    expect(result.lastOpenedLabel).toBe("2d ago");
  });
});

describe("helpers: isHelperOwnedTool", () => {
  it("matches tool ID by 8-char short prefix", () => {
    const helperIds = new Set(["abcdef12-3456-7890-abcd-ef1234567890"]);
    expect(isHelperOwnedTool("abcdef12:call-1", helperIds)).toBe(true);
  });

  it("matches tool ID by full helper ID (legacy)", () => {
    const helperId = "abcdef12-3456-7890-abcd-ef1234567890";
    const helperIds = new Set([helperId]);
    expect(isHelperOwnedTool(`${helperId}:call-1`, helperIds)).toBe(true);
  });

  it("returns false when no helper matches", () => {
    const helperIds = new Set(["aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"]);
    expect(isHelperOwnedTool("zzzzzzzz:call-1", helperIds)).toBe(false);
  });

  it("returns false for empty helper set", () => {
    expect(isHelperOwnedTool("abc:call", new Set())).toBe(false);
  });
});

describe("helpers: buildSnapshotHelperToolSummary", () => {
  const toolCalls = [
    { id: "abc12345:call-1", toolName: "read", status: "completed" },
    { id: "abc12345:call-2", toolName: "read", status: "completed" },
    { id: "abc12345:call-3", toolName: "write", status: "failed" },
    { id: "abc12345:call-4", toolName: "edit", status: "running" },
    { id: "other-id:call-5", toolName: "shell", status: "completed" },
  ];

  it("counts tool calls by helper", () => {
    const summary = buildSnapshotHelperToolSummary("abc12345-6789-abcd-ef01-234567890abc", toolCalls);
    expect(summary.totalToolCalls).toBe(4);
    expect(summary.completedSteps).toBe(3); // completed + completed + failed = 3
    expect(summary.toolCounts["read"]).toBe(2);
    expect(summary.toolCounts["write"]).toBe(1);
    expect(summary.toolCounts["edit"]).toBe(1);
  });

  it("counts denied and cancelled as completed steps", () => {
    const calls = [
      { id: "abc12345:call-1", toolName: "read", status: "denied" },
      { id: "abc12345:call-2", toolName: "read", status: "cancelled" },
    ];
    const summary = buildSnapshotHelperToolSummary("abc12345-6789-abcd-ef01-234567890abc", calls);
    expect(summary.completedSteps).toBe(2);
  });

  it("returns zeros for non-matching helper", () => {
    const summary = buildSnapshotHelperToolSummary("zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz", toolCalls);
    expect(summary.totalToolCalls).toBe(0);
    expect(summary.completedSteps).toBe(0);
    expect(Object.keys(summary.toolCounts)).toHaveLength(0);
  });
});

describe("helpers: formatThreadTimeLabel edge cases", () => {
  const now = new Date("2026-04-25T12:00:00Z").getTime();

  it("handles future timestamps by treating diff as 0", () => {
    const future = new Date(now + 60_000).toISOString();
    expect(formatThreadTimeLabel(future, "en", now)).toBe("Just now");
  });

  it("formats zh-CN just now", () => {
    const recent = new Date(now - 30_000).toISOString();
    expect(formatThreadTimeLabel(recent, "zh-CN", now)).toBe("刚刚");
  });

  it("returns empty string for NaN timestamp", () => {
    expect(formatThreadTimeLabel("not-a-date", "en", now)).toBe("");
  });
});

describe("helpers: buildProjectOptionFromPath edge cases", () => {
  it("returns null for empty string path", () => {
    expect(buildProjectOptionFromPath("")).toBeNull();
  });

  it("falls back to new-project id when segments are empty", () => {
    const result = buildProjectOptionFromPath("/");
    expect(result).not.toBeNull();
    // root path with trailing slash stripped → should still produce option
    expect(result!.name.length).toBeGreaterThan(0);
  });
});
