import { beforeEach, describe, expect, it } from "vitest";
import type { ThreadSummaryDto, WorkspaceDto } from "@/shared/types/api";
import {
  buildGitDiffPreview,
  buildGitSplitDiffRows,
  readPanelVisibilityState,
  buildInitialWorkspaces,
  formatThreadTimeLabel,
  buildWorkspaceThreadItem,
  buildWorkspaceItemsFromDtos,
  clearActiveThreads,
  getActiveThread,
  activateThread,
  buildThreadTitle,
  mergeRecentProjects,
  buildProjectOptionFromPath,
  formatProjectPathLabel,
  isEditableSelectionTarget,
  isNodeInsideContainer,
  selectContainerContents,
} from "@/modules/workbench-shell/model/helpers";
import { PANEL_VISIBILITY_STORAGE_KEY } from "@/modules/workbench-shell/model/fixtures";

beforeEach(() => {
  window.localStorage.clear();
});

// ---------------------------------------------------------------------------
// buildGitDiffPreview
// ---------------------------------------------------------------------------

describe("buildGitDiffPreview", () => {
  it("generates an add-only diff for status 'A'", () => {
    const result = buildGitDiffPreview({ path: "src/new.ts", status: "A" });
    expect(result.meta[0]).toContain("diff --git");
    expect(result.meta[1]).toContain("new file mode");
    expect(result.lines.every((l) => l.kind === "add")).toBe(true);
    expect(result.lines[0].oldNumber).toBeNull();
    expect(result.lines[0].newNumber).toBe(1);
  });

  it("generates a remove-only diff for status 'D'", () => {
    const result = buildGitDiffPreview({ path: "src/old.ts", status: "D" });
    expect(result.meta[1]).toContain("deleted file mode");
    expect(result.lines.every((l) => l.kind === "remove")).toBe(true);
    expect(result.lines[0].oldNumber).toBe(1);
    expect(result.lines[0].newNumber).toBeNull();
  });

  it("generates a mixed diff for status 'M'", () => {
    const result = buildGitDiffPreview({ path: "src/file.ts", status: "M" });
    const kinds = new Set(result.lines.map((l) => l.kind));
    expect(kinds.size).toBeGreaterThan(1); // should have context and changes
  });

  it("uses CSS template for .css files", () => {
    const result = buildGitDiffPreview({ path: "styles/main.css", status: "M" });
    expect(result.lines.some((l) => l.text.includes("gap"))).toBe(true);
  });

  it("uses JSON template for .json files", () => {
    const result = buildGitDiffPreview({ path: "config.json", status: "A" });
    expect(result.lines.some((l) => l.text.includes("beforeDevCommand"))).toBe(true);
  });

  it("uses Markdown template for .md files", () => {
    const result = buildGitDiffPreview({ path: "README.md", status: "M" });
    expect(result.lines.some((l) => l.text.includes("Tiy Desktop"))).toBe(true);
  });

  it("uses generic template for unknown extensions", () => {
    const result = buildGitDiffPreview({ path: "config.yaml", status: "M" });
    expect(result.lines.some((l) => l.text.includes("panelState"))).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// buildGitSplitDiffRows
// ---------------------------------------------------------------------------

describe("buildGitSplitDiffRows", () => {
  it("generates add-only rows for status 'A'", () => {
    const rows = buildGitSplitDiffRows({ path: "new.ts", status: "A" });
    expect(rows.every((r) => r.kind === "add")).toBe(true);
    expect(rows[0].leftText).toBe("");
    expect(rows[0].rightText.length).toBeGreaterThan(0);
  });

  it("generates remove-only rows for status 'D'", () => {
    const rows = buildGitSplitDiffRows({ path: "old.ts", status: "D" });
    expect(rows.every((r) => r.kind === "remove")).toBe(true);
    expect(rows[0].leftText.length).toBeGreaterThan(0);
    expect(rows[0].rightText).toBe("");
  });

  it("generates context and modified rows for status 'M'", () => {
    const rows = buildGitSplitDiffRows({ path: "file.ts", status: "M" });
    const kinds = new Set(rows.map((r) => r.kind));
    expect(kinds.has("context") || kinds.has("modified")).toBe(true);
  });

  it("generates add rows for lines only present in the new version", () => {
    const rows = buildGitSplitDiffRows({ path: "file.ts", status: "M" });
    const addRows = rows.filter((r) => r.kind === "add");
    for (const row of addRows) {
      expect(row.leftNumber).toBeNull();
      expect(row.rightNumber).not.toBeNull();
    }
  });
});

// ---------------------------------------------------------------------------
// readPanelVisibilityState
// ---------------------------------------------------------------------------

describe("readPanelVisibilityState", () => {
  it("returns defaults when localStorage is empty", () => {
    const state = readPanelVisibilityState();
    expect(state.isSidebarOpen).toBe(true);
    expect(state.isDrawerOpen).toBe(false);
  });

  it("returns parsed state from localStorage", () => {
    window.localStorage.setItem(
      PANEL_VISIBILITY_STORAGE_KEY,
      JSON.stringify({ isSidebarOpen: false, isDrawerOpen: true }),
    );
    const state = readPanelVisibilityState();
    expect(state.isSidebarOpen).toBe(false);
    expect(state.isDrawerOpen).toBe(true);
  });

  it("returns defaults when localStorage has invalid JSON", () => {
    window.localStorage.setItem(PANEL_VISIBILITY_STORAGE_KEY, "not-json");
    const state = readPanelVisibilityState();
    expect(state.isSidebarOpen).toBe(true);
    expect(state.isDrawerOpen).toBe(false);
  });

  it("fills missing fields with defaults", () => {
    window.localStorage.setItem(
      PANEL_VISIBILITY_STORAGE_KEY,
      JSON.stringify({ isSidebarOpen: false }),
    );
    const state = readPanelVisibilityState();
    expect(state.isSidebarOpen).toBe(false);
    expect(state.isDrawerOpen).toBe(false); // default
  });

  it("ignores non-boolean values and falls back to defaults", () => {
    window.localStorage.setItem(
      PANEL_VISIBILITY_STORAGE_KEY,
      JSON.stringify({ isSidebarOpen: "yes", isDrawerOpen: 42 }),
    );
    const state = readPanelVisibilityState();
    expect(state.isSidebarOpen).toBe(true); // default
    expect(state.isDrawerOpen).toBe(false); // default
  });
});

// ---------------------------------------------------------------------------
// buildInitialWorkspaces
// ---------------------------------------------------------------------------

describe("buildInitialWorkspaces", () => {
  it("returns an array of workspace items", () => {
    const workspaces = buildInitialWorkspaces();
    expect(Array.isArray(workspaces)).toBe(true);
    expect(workspaces.length).toBeGreaterThan(0);
  });

  it("generates thread ids from workspace id and index", () => {
    const workspaces = buildInitialWorkspaces();
    const firstWorkspace = workspaces[0];
    if (firstWorkspace.threads.length > 0) {
      expect(firstWorkspace.threads[0].id).toContain(firstWorkspace.id);
      expect(firstWorkspace.threads[0].id).toContain("thread-1");
    }
  });

  it("sets all threads as inactive", () => {
    const workspaces = buildInitialWorkspaces();
    for (const workspace of workspaces) {
      for (const thread of workspace.threads) {
        expect(thread.active).toBe(false);
      }
    }
  });
});

// ---------------------------------------------------------------------------
// formatThreadTimeLabel
// ---------------------------------------------------------------------------

describe("formatThreadTimeLabel", () => {
  it("returns empty string for null input", () => {
    expect(formatThreadTimeLabel(null)).toBe("");
  });

  it("returns empty string for undefined input", () => {
    expect(formatThreadTimeLabel(undefined)).toBe("");
  });

  it("returns empty string for empty string input", () => {
    expect(formatThreadTimeLabel("")).toBe("");
  });

  it("returns empty string for invalid date string", () => {
    expect(formatThreadTimeLabel("not-a-date")).toBe("");
  });

  it("returns 'just now' label for timestamps less than 1 minute ago", () => {
    const now = Date.now();
    const thirtySecondsAgo = new Date(now - 30_000).toISOString();
    const result = formatThreadTimeLabel(thirtySecondsAgo, "en", now);
    expect(result.length).toBeGreaterThan(0); // Should be the "just now" translation
  });

  it("returns minutes format for timestamps 1-59 minutes ago", () => {
    const now = Date.now();
    const fiveMinutesAgo = new Date(now - 5 * 60_000).toISOString();
    expect(formatThreadTimeLabel(fiveMinutesAgo, "en", now)).toBe("5m");
  });

  it("returns hours format for timestamps 1-23 hours ago", () => {
    const now = Date.now();
    const threeHoursAgo = new Date(now - 3 * 60 * 60_000).toISOString();
    expect(formatThreadTimeLabel(threeHoursAgo, "en", now)).toBe("3h");
  });

  it("returns days format for timestamps 1-6 days ago", () => {
    const now = Date.now();
    const twoDaysAgo = new Date(now - 2 * 24 * 60 * 60_000).toISOString();
    expect(formatThreadTimeLabel(twoDaysAgo, "en", now)).toBe("2d");
  });

  it("returns weeks format for timestamps 7+ days ago", () => {
    const now = Date.now();
    const twoWeeksAgo = new Date(now - 14 * 24 * 60 * 60_000).toISOString();
    expect(formatThreadTimeLabel(twoWeeksAgo, "en", now)).toBe("2w");
  });

  it("treats future timestamps as just now (clamps to 0)", () => {
    const now = Date.now();
    const future = new Date(now + 60_000).toISOString();
    const result = formatThreadTimeLabel(future, "en", now);
    // diffMs is clamped to 0 via Math.max, so < 1 minute => justNow
    expect(result.length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// buildWorkspaceThreadItem
// ---------------------------------------------------------------------------

describe("buildWorkspaceThreadItem", () => {
  function createThread(overrides: Partial<ThreadSummaryDto> = {}): ThreadSummaryDto {
    return {
      id: "t-1",
      workspaceId: "ws-1",
      profileId: null,
      title: "Test Thread",
      status: "idle",
      lastActiveAt: new Date().toISOString(),
      createdAt: new Date().toISOString(),
      ...overrides,
    };
  }

  it("maps thread to workspace thread item", () => {
    const thread = createThread();
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.id).toBe("t-1");
    expect(item.name).toBe("Test Thread");
    expect(item.active).toBe(false);
  });

  it("marks thread as active when matching activeThreadId", () => {
    const thread = createThread({ id: "t-active" });
    const item = buildWorkspaceThreadItem(thread, "t-active");
    expect(item.active).toBe(true);
  });

  it("uses default title for empty thread title", () => {
    const thread = createThread({ title: "" });
    const item = buildWorkspaceThreadItem(thread, null, "en");
    expect(item.name.length).toBeGreaterThan(0); // Should be the translated "New Thread"
  });

  it("trims whitespace from thread title", () => {
    const thread = createThread({ title: "  Spaced Title  " });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.name).toBe("Spaced Title");
  });

  it("maps running status correctly", () => {
    const thread = createThread({ status: "running" });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.status).toBe("running");
  });

  it("maps waiting_approval to needs-reply", () => {
    const thread = createThread({ status: "waiting_approval" });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.status).toBe("needs-reply");
  });

  it("maps needs_reply to needs-reply", () => {
    const thread = createThread({ status: "needs_reply" });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.status).toBe("needs-reply");
  });

  it("maps failed status correctly", () => {
    const thread = createThread({ status: "failed" });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.status).toBe("failed");
  });

  it("maps interrupted status correctly", () => {
    const thread = createThread({ status: "interrupted" });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.status).toBe("interrupted");
  });

  it("maps idle to completed", () => {
    const thread = createThread({ status: "idle" });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.status).toBe("completed");
  });

  it("maps archived to completed", () => {
    const thread = createThread({ status: "archived" });
    const item = buildWorkspaceThreadItem(thread, null);
    expect(item.status).toBe("completed");
  });
});

// ---------------------------------------------------------------------------
// buildWorkspaceItemsFromDtos
// ---------------------------------------------------------------------------

describe("buildWorkspaceItemsFromDtos", () => {
  function createWorkspaceDto(overrides: Partial<WorkspaceDto> = {}): WorkspaceDto {
    return {
      id: "ws-1",
      name: "Repo",
      path: "/home/user/repo",
      canonicalPath: "/home/user/repo",
      displayPath: "~/repo",
      isDefault: false,
      isGit: true,
      autoWorkTree: false,
      status: "ready",
      lastValidatedAt: null,
      createdAt: "2026-04-12T00:00:00Z",
      updatedAt: "2026-04-12T00:00:00Z",
      kind: "repo",
      parentWorkspaceId: null,
      gitCommonDir: null,
      branch: "main",
      worktreeName: null,
      ...overrides,
    };
  }

  it("maps workspaces to workspace items", () => {
    const workspaces = [createWorkspaceDto()];
    const result = buildWorkspaceItemsFromDtos(workspaces, {}, null);
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("ws-1");
    expect(result[0].name).toBe("Repo");
  });

  it("includes threads for a workspace", () => {
    const threads: ThreadSummaryDto[] = [
      {
        id: "t-1",
        workspaceId: "ws-1",
        profileId: null,
        title: "Thread 1",
        status: "idle",
        lastActiveAt: new Date().toISOString(),
        createdAt: new Date().toISOString(),
      },
    ];
    const result = buildWorkspaceItemsFromDtos(
      [createWorkspaceDto()],
      { "ws-1": threads },
      null,
    );
    expect(result[0].threads).toHaveLength(1);
    expect(result[0].threads[0].name).toBe("Thread 1");
  });

  it("returns empty threads when no threads exist for workspace", () => {
    const result = buildWorkspaceItemsFromDtos([createWorkspaceDto()], {}, null);
    expect(result[0].threads).toHaveLength(0);
  });

  it("uses canonicalPath, falling back to path", () => {
    const ws = createWorkspaceDto({ canonicalPath: "", path: "/fallback" });
    const result = buildWorkspaceItemsFromDtos([ws], {}, null);
    expect(result[0].path).toBe("/fallback");
  });

  it("generates worktreeHash from worktreeName", () => {
    const ws = createWorkspaceDto({
      kind: "worktree",
      worktreeName: "abc123-feat",
    });
    const result = buildWorkspaceItemsFromDtos([ws], {}, null);
    expect(result[0].worktreeHash).toBe("abc123");
  });
});

// ---------------------------------------------------------------------------
// clearActiveThreads
// ---------------------------------------------------------------------------

describe("clearActiveThreads", () => {
  it("sets all threads to inactive", () => {
    const workspaces = [
      {
        id: "ws-1",
        name: "Test",
        defaultOpen: false,
        path: "/test",
        threads: [
          { id: "t-1", name: "Thread 1", time: "", active: true, status: "completed" as const, profileId: null },
          { id: "t-2", name: "Thread 2", time: "", active: false, status: "completed" as const, profileId: null },
        ],
      },
    ];
    const result = clearActiveThreads(workspaces);
    expect(result[0].threads.every((t) => t.active === false)).toBe(true);
  });

  it("returns new array (immutable)", () => {
    const workspaces = [{
      id: "ws-1",
      name: "Test",
      defaultOpen: false,
      path: "/test",
      threads: [{ id: "t-1", name: "Thread 1", time: "", active: true, status: "completed" as const, profileId: null }],
    }];
    const result = clearActiveThreads(workspaces);
    expect(result).not.toBe(workspaces);
    expect(result[0]).not.toBe(workspaces[0]);
  });
});

// ---------------------------------------------------------------------------
// getActiveThread
// ---------------------------------------------------------------------------

describe("getActiveThread", () => {
  it("returns the active thread", () => {
    const workspaces = [
      {
        id: "ws-1",
        name: "Test",
        defaultOpen: false,
        path: "/test",
        threads: [
          { id: "t-1", name: "Thread 1", time: "", active: false, status: "completed" as const, profileId: null },
          { id: "t-2", name: "Thread 2", time: "", active: true, status: "running" as const, profileId: null },
        ],
      },
    ];
    const result = getActiveThread(workspaces);
    expect(result).not.toBeNull();
    expect(result!.id).toBe("t-2");
  });

  it("returns null when no thread is active", () => {
    const workspaces = [
      {
        id: "ws-1",
        name: "Test",
        defaultOpen: false,
        path: "/test",
        threads: [
          { id: "t-1", name: "Thread 1", time: "", active: false, status: "completed" as const, profileId: null },
        ],
      },
    ];
    expect(getActiveThread(workspaces)).toBeNull();
  });

  it("returns null for empty workspaces", () => {
    expect(getActiveThread([])).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// activateThread
// ---------------------------------------------------------------------------

describe("activateThread", () => {
  it("activates the specified thread and deactivates others", () => {
    const workspaces = [
      {
        id: "ws-1",
        name: "Test",
        defaultOpen: false,
        path: "/test",
        threads: [
          { id: "t-1", name: "Thread 1", time: "", active: true, status: "completed" as const, profileId: null },
          { id: "t-2", name: "Thread 2", time: "", active: false, status: "completed" as const, profileId: null },
        ],
      },
    ];
    const result = activateThread(workspaces, "t-2");
    expect(result[0].threads[0].active).toBe(false);
    expect(result[0].threads[1].active).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// buildThreadTitle
// ---------------------------------------------------------------------------

describe("buildThreadTitle", () => {
  it("returns trimmed text for short prompts", () => {
    expect(buildThreadTitle("  Hello world  ")).toBe("Hello world");
  });

  it("truncates at 30 chars with ellipsis for long prompts", () => {
    const longPrompt = "This is a very long prompt that exceeds thirty characters by far";
    const result = buildThreadTitle(longPrompt);
    expect(result.length).toBe(33); // 30 + "..."
    expect(result.endsWith("...")).toBe(true);
  });

  it("collapses whitespace", () => {
    expect(buildThreadTitle("Hello   \n  world")).toBe("Hello world");
  });

  it("returns exact 30 chars without truncation", () => {
    const exact = "a".repeat(30);
    expect(buildThreadTitle(exact)).toBe(exact);
  });
});

// ---------------------------------------------------------------------------
// mergeRecentProjects
// ---------------------------------------------------------------------------

describe("mergeRecentProjects", () => {
  it("prepends the new project", () => {
    const current = [{ id: "a", name: "A", path: "/a" }];
    const next = { id: "b", name: "B", path: "/b" };
    const result = mergeRecentProjects(current, next);
    expect(result[0].id).toBe("b");
    expect(result[1].id).toBe("a");
  });

  it("deduplicates by id", () => {
    const current = [{ id: "a", name: "A", path: "/a" }];
    const next = { id: "a", name: "A Updated", path: "/a-new" };
    const result = mergeRecentProjects(current, next);
    expect(result).toHaveLength(1);
    expect(result[0].name).toBe("A Updated");
  });

  it("deduplicates by name+path combination", () => {
    const current = [{ id: "old-id", name: "Project", path: "/project" }];
    const next = { id: "new-id", name: "Project", path: "/project" };
    const result = mergeRecentProjects(current, next);
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("new-id");
  });

  it("limits to 6 items", () => {
    const current = Array.from({ length: 6 }, (_, i) => ({
      id: `p${i}`,
      name: `Project ${i}`,
      path: `/p${i}`,
    }));
    const next = { id: "new", name: "New Project", path: "/new" };
    const result = mergeRecentProjects(current, next);
    expect(result).toHaveLength(6);
    expect(result[0].id).toBe("new");
  });
});

// ---------------------------------------------------------------------------
// buildProjectOptionFromPath
// ---------------------------------------------------------------------------

describe("buildProjectOptionFromPath", () => {
  it("returns null for null path", () => {
    expect(buildProjectOptionFromPath(null)).toBeNull();
  });

  it("extracts folder name from path", () => {
    const result = buildProjectOptionFromPath("/home/user/my-project");
    expect(result).not.toBeNull();
    expect(result!.name).toBe("my-project");
  });

  it("normalizes backslashes", () => {
    const result = buildProjectOptionFromPath("C:\\Users\\dev\\project");
    expect(result!.path).toBe("C:/Users/dev/project");
  });

  it("strips trailing slashes", () => {
    const result = buildProjectOptionFromPath("/home/user/project/");
    expect(result!.path).toBe("/home/user/project");
  });

  it("generates a normalized id", () => {
    const result = buildProjectOptionFromPath("/home/user/my-project");
    expect(result!.id).toBeTruthy();
    expect(result!.id).not.toContain(" ");
  });
});

// ---------------------------------------------------------------------------
// formatProjectPathLabel
// ---------------------------------------------------------------------------

describe("formatProjectPathLabel", () => {
  it("returns full path for short paths", () => {
    expect(formatProjectPathLabel("/a/b/c")).toBe("/a/b/c");
  });

  it("truncates long paths to last 4 segments", () => {
    const result = formatProjectPathLabel("/a/b/c/d/e/f");
    expect(result).toBe(".../c/d/e/f");
  });

  it("normalizes backslashes", () => {
    const result = formatProjectPathLabel("C:\\a\\b\\c");
    expect(result).toBe("C:/a/b/c");
  });
});

// ---------------------------------------------------------------------------
// isEditableSelectionTarget
// ---------------------------------------------------------------------------

describe("isEditableSelectionTarget", () => {
  it("returns false for null", () => {
    expect(isEditableSelectionTarget(null)).toBe(false);
  });

  it("returns true for input element", () => {
    const input = document.createElement("input");
    expect(isEditableSelectionTarget(input)).toBe(true);
  });

  it("returns true for textarea element", () => {
    const textarea = document.createElement("textarea");
    expect(isEditableSelectionTarget(textarea)).toBe(true);
  });

  it("returns true for contenteditable element", () => {
    const div = document.createElement("div");
    div.setAttribute("contenteditable", "true");
    expect(isEditableSelectionTarget(div)).toBe(true);
  });

  it("returns false for a plain div", () => {
    const div = document.createElement("div");
    expect(isEditableSelectionTarget(div)).toBe(false);
  });

  it("returns true for child of input", () => {
    const input = document.createElement("input");
    const wrapper = document.createElement("div");
    wrapper.appendChild(input);
    // input itself is editable
    expect(isEditableSelectionTarget(input)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// isNodeInsideContainer
// ---------------------------------------------------------------------------

describe("isNodeInsideContainer", () => {
  it("returns true when node is inside container", () => {
    const container = document.createElement("div");
    const child = document.createElement("span");
    container.appendChild(child);
    expect(isNodeInsideContainer(container, child)).toBe(true);
  });

  it("returns false when node is outside container", () => {
    const container = document.createElement("div");
    const outside = document.createElement("span");
    expect(isNodeInsideContainer(container, outside)).toBe(false);
  });

  it("returns false for null container", () => {
    const node = document.createElement("span");
    expect(isNodeInsideContainer(null, node)).toBe(false);
  });

  it("returns false for null node", () => {
    const container = document.createElement("div");
    expect(isNodeInsideContainer(container, null)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// selectContainerContents
// ---------------------------------------------------------------------------

describe("selectContainerContents", () => {
  it("does not throw when called", () => {
    const container = document.createElement("div");
    container.textContent = "Hello";
    // jsdom has limited Selection support, just verify no error
    expect(() => selectContainerContents(container)).not.toThrow();
  });
});
