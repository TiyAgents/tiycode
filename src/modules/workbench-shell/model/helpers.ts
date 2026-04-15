import {
  DEFAULT_PANEL_VISIBILITY_STATE,
  PANEL_VISIBILITY_STORAGE_KEY,
  WORKSPACE_ITEMS,
} from "@/modules/workbench-shell/model/fixtures";
import { translate } from "@/i18n";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type {
  ThreadStatus as ApiThreadStatus,
  ThreadSummaryDto,
  WorkspaceDto,
} from "@/shared/types/api";
import type {
  GitChangeFile,
  GitDiffPreview,
  GitDiffLine,
  GitSplitDiffRow,
  PanelVisibilityState,
  ProjectOption,
  ThreadStatus as WorkbenchThreadStatus,
  WorkspaceItem,
  WorkspaceThreadItem,
} from "@/modules/workbench-shell/model/types";

function getDiffTemplate(file: GitChangeFile) {
  const fileName = file.path.split("/").pop() ?? file.path;

  if (file.path.endsWith(".tsx") || file.path.endsWith(".ts")) {
    return {
      before: [
        'const panelDensity = "comfortable";',
        "const enablePreview = false;",
        `return <section data-file="${fileName}">{panelDensity}</section>;`,
      ],
      after: [
        'const panelDensity = "compact";',
        "const enablePreview = true;",
        'const previewMode = "diff";',
        `return <section data-file="${fileName}" data-preview="diff">{panelDensity}</section>;`,
      ],
    };
  }

  if (file.path.endsWith(".css")) {
    return {
      before: [
        ".tracked-row {",
        "  gap: 10px;",
        "  padding: 8px 10px;",
        "}",
      ],
      after: [
        ".tracked-row {",
        "  gap: 8px;",
        "  padding: 6px 8px;",
        "}",
        ".tracked-row:hover { background: var(--app-surface-hover); }",
      ],
    };
  }

  if (file.path.endsWith(".json")) {
    return {
      before: [
        "{",
        '  "beforeDevCommand": "npm run dev:web",',
        '  "beforeBuildCommand": "npm run build:web"',
        "}",
      ],
      after: [
        "{",
        '  "beforeDevCommand": "npm run dev:web",',
        '  "beforeBuildCommand": "npm run build:web",',
        '  "sourceControlPreview": true',
        "}",
      ],
    };
  }

  if (file.path.endsWith(".md")) {
    return {
      before: [
        "# Tiy Desktop",
        "",
        "- Project Panel",
        "- Git Panel",
      ],
      after: [
        "# Tiy Desktop",
        "",
        "- Project Panel",
        "- Git Panel",
        "- Diff preview overlay",
      ],
    };
  }

  return {
    before: [
      `// ${fileName}`,
      "export const panelState = {",
      '  density: "comfortable",',
      "};",
    ],
    after: [
      `// ${fileName}`,
      "export const panelState = {",
      '  density: "compact",',
      '  preview: "diff",',
      "};",
    ],
  };
}

export function buildGitDiffPreview(file: GitChangeFile): GitDiffPreview {
  const template = getDiffTemplate(file);
  const startLine = 18;

  if (file.status === "A") {
    return {
      meta: [
        `diff --git a/${file.path} b/${file.path}`,
        "new file mode 100644",
        "--- /dev/null",
        `+++ b/${file.path}`,
        `@@ -0,0 +1,${template.after.length} @@`,
      ],
      lines: template.after.map((text, index) => ({
        kind: "add",
        oldNumber: null,
        newNumber: index + 1,
        text,
      })),
    };
  }

  if (file.status === "D") {
    return {
      meta: [
        `diff --git a/${file.path} b/${file.path}`,
        "deleted file mode 100644",
        `--- a/${file.path}`,
        "+++ /dev/null",
        `@@ -1,${template.before.length} +0,0 @@`,
      ],
      lines: template.before.map((text, index) => ({
        kind: "remove",
        oldNumber: index + 1,
        newNumber: null,
        text,
      })),
    };
  }

  const lines: GitDiffLine[] = [];
  let oldLine = startLine;
  let newLine = startLine;
  const maxLength = Math.max(template.before.length, template.after.length);

  for (let index = 0; index < maxLength; index += 1) {
    const previous = template.before[index];
    const next = template.after[index];

    if (previous !== undefined && next !== undefined && previous === next) {
      lines.push({
        kind: "context",
        oldNumber: oldLine,
        newNumber: newLine,
        text: previous,
      });
      oldLine += 1;
      newLine += 1;
      continue;
    }

    if (previous !== undefined) {
      lines.push({
        kind: "remove",
        oldNumber: oldLine,
        newNumber: null,
        text: previous,
      });
      oldLine += 1;
    }

    if (next !== undefined) {
      lines.push({
        kind: "add",
        oldNumber: null,
        newNumber: newLine,
        text: next,
      });
      newLine += 1;
    }
  }

  return {
    meta: [
      `diff --git a/${file.path} b/${file.path}`,
      `--- a/${file.path}`,
      `+++ b/${file.path}`,
      `@@ -${startLine},${template.before.length} +${startLine},${template.after.length} @@`,
    ],
    lines,
  };
}

export function buildGitSplitDiffRows(file: GitChangeFile): ReadonlyArray<GitSplitDiffRow> {
  const template = getDiffTemplate(file);

  if (file.status === "A") {
    return template.after.map((text, index) => ({
      kind: "add",
      leftNumber: null,
      rightNumber: index + 1,
      leftText: "",
      rightText: text,
    }));
  }

  if (file.status === "D") {
    return template.before.map((text, index) => ({
      kind: "remove",
      leftNumber: index + 1,
      rightNumber: null,
      leftText: text,
      rightText: "",
    }));
  }

  const rows: GitSplitDiffRow[] = [];
  let leftNumber = 18;
  let rightNumber = 18;
  const maxLength = Math.max(template.before.length, template.after.length);

  for (let index = 0; index < maxLength; index += 1) {
    const previous = template.before[index];
    const next = template.after[index];

    if (previous !== undefined && next !== undefined && previous === next) {
      rows.push({
        kind: "context",
        leftNumber,
        rightNumber,
        leftText: previous,
        rightText: next,
      });
      leftNumber += 1;
      rightNumber += 1;
      continue;
    }

    if (previous !== undefined && next !== undefined) {
      rows.push({
        kind: "modified",
        leftNumber,
        rightNumber,
        leftText: previous,
        rightText: next,
      });
      leftNumber += 1;
      rightNumber += 1;
      continue;
    }

    if (previous !== undefined) {
      rows.push({
        kind: "remove",
        leftNumber,
        rightNumber: null,
        leftText: previous,
        rightText: "",
      });
      leftNumber += 1;
    }

    if (next !== undefined) {
      rows.push({
        kind: "add",
        leftNumber: null,
        rightNumber,
        leftText: "",
        rightText: next,
      });
      rightNumber += 1;
    }
  }

  return rows;
}

export function readPanelVisibilityState(): PanelVisibilityState {
  if (typeof window === "undefined") {
    return DEFAULT_PANEL_VISIBILITY_STATE;
  }

  const rawValue = window.localStorage.getItem(PANEL_VISIBILITY_STORAGE_KEY);

  if (!rawValue) {
    return DEFAULT_PANEL_VISIBILITY_STATE;
  }

  try {
    const parsed = JSON.parse(rawValue) as Partial<PanelVisibilityState>;

    return {
      isSidebarOpen:
        typeof parsed.isSidebarOpen === "boolean"
          ? parsed.isSidebarOpen
          : DEFAULT_PANEL_VISIBILITY_STATE.isSidebarOpen,
      isDrawerOpen:
        typeof parsed.isDrawerOpen === "boolean"
          ? parsed.isDrawerOpen
          : DEFAULT_PANEL_VISIBILITY_STATE.isDrawerOpen,
    };
  } catch {
    return DEFAULT_PANEL_VISIBILITY_STATE;
  }
}

export function buildInitialWorkspaces(): Array<WorkspaceItem> {
  return WORKSPACE_ITEMS.map((workspace) => ({
    ...workspace,
    threads: workspace.threads.map((thread, index) => ({
      ...thread,
      id: `${workspace.id}-thread-${index + 1}`,
      active: false,
    })),
  }));
}

function mapThreadStatus(status: ApiThreadStatus): WorkbenchThreadStatus {
  switch (status) {
    case "running":
      return "running";
    case "waiting_approval":
    case "needs_reply":
      return "needs-reply";
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
    default:
      return "completed";
  }
}

export function formatThreadTimeLabel(value: string | null | undefined, language: LanguagePreference = "zh-CN", now = Date.now()) {
  if (!value) {
    return "";
  }

  const timestamp = new Date(value).getTime();
  if (Number.isNaN(timestamp)) {
    return "";
  }

  const diffMs = Math.max(0, now - timestamp);
  const diffMinutes = Math.floor(diffMs / 60_000);

  if (diffMinutes < 1) {
    return translate(language, "time.justNow");
  }

  if (diffMinutes < 60) {
    return `${diffMinutes}m`;
  }

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 24) {
    return `${diffHours}h`;
  }

  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 7) {
    return `${diffDays}d`;
  }

  return `${Math.floor(diffDays / 7)}w`;
}

export function buildWorkspaceThreadItem(
  thread: ThreadSummaryDto,
  activeThreadId: string | null,
  language: LanguagePreference = "zh-CN",
): WorkspaceThreadItem {
  const trimmedTitle = thread.title.trim();
  const displayTitle = trimmedTitle || translate(language, "dashboard.newThread");

  return {
    id: thread.id,
    name: displayTitle,
    time: formatThreadTimeLabel(thread.lastActiveAt || thread.createdAt, language),
    active: thread.id === activeThreadId,
    status: mapThreadStatus(thread.status),
  };
}

export function buildWorkspaceItemsFromDtos(
  workspaces: ReadonlyArray<WorkspaceDto>,
  threadsByWorkspaceId: Record<string, ReadonlyArray<ThreadSummaryDto>>,
  activeThreadId: string | null,
  language: LanguagePreference = "zh-CN",
): Array<WorkspaceItem> {
  return workspaces.map((workspace) => ({
    id: workspace.id,
    name: workspace.name,
    defaultOpen: workspace.isDefault,
    path: workspace.canonicalPath || workspace.path,
    threads: (threadsByWorkspaceId[workspace.id] ?? []).map((thread) =>
      buildWorkspaceThreadItem(thread, activeThreadId, language),
    ),
  }));
}

export function clearActiveThreads(workspaces: ReadonlyArray<WorkspaceItem>): Array<WorkspaceItem> {
  return workspaces.map((workspace) => ({
    ...workspace,
    threads: workspace.threads.map((thread) => ({
      ...thread,
      active: false,
    })),
  }));
}

export function isEditableSelectionTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  return Boolean(target.closest("input, textarea, select, [contenteditable=''], [contenteditable='true'], [contenteditable='plaintext-only']"));
}

export function isNodeInsideContainer(container: HTMLElement | null, node: Node | null) {
  return Boolean(container && node && container.contains(node));
}

export function selectContainerContents(container: HTMLElement) {
  const selection = window.getSelection();

  if (!selection) {
    return;
  }

  const range = document.createRange();
  range.selectNodeContents(container);
  selection.removeAllRanges();
  selection.addRange(range);
}

export function getActiveThread(workspaces: ReadonlyArray<WorkspaceItem>): WorkspaceThreadItem | null {
  for (const workspace of workspaces) {
    const activeThread = workspace.threads.find((thread) => thread.active);

    if (activeThread) {
      return activeThread;
    }
  }

  return null;
}

export function activateThread(workspaces: ReadonlyArray<WorkspaceItem>, threadId: string): Array<WorkspaceItem> {
  return workspaces.map((workspace) => ({
    ...workspace,
    threads: workspace.threads.map((thread) => ({
      ...thread,
      active: thread.id === threadId,
    })),
  }));
}

export function buildThreadTitle(prompt: string) {
  const compactPrompt = prompt.trim().replace(/\s+/g, " ");

  if (compactPrompt.length <= 30) {
    return compactPrompt;
  }

  return `${compactPrompt.slice(0, 30)}...`;
}

export function mergeRecentProjects(
  currentProjects: ReadonlyArray<ProjectOption>,
  nextProject: ProjectOption,
): Array<ProjectOption> {
  return [
    nextProject,
    ...currentProjects.filter(
      (project) =>
        !(project.id === nextProject.id || (project.name === nextProject.name && project.path === nextProject.path)),
    ),
  ].slice(0, 6);
}

export function buildProjectOptionFromPath(path: string | null): ProjectOption | null {
  if (!path) {
    return null;
  }

  const normalizedPath = path.replace(/\\/g, "/").replace(/\/+$/g, "");
  const segments = normalizedPath.split("/");
  const folderName = segments[segments.length - 1] || "new-project";
  const normalizedId = `${folderName}-${normalizedPath}`
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

  return {
    id: normalizedId || `project-${Date.now()}`,
    name: folderName,
    path: normalizedPath,
    lastOpenedLabel: translate("zh-CN", "time.justNow"),
  };
}

export function formatProjectPathLabel(path: string) {
  const normalizedPath = path.replace(/\\/g, "/").replace(/\/+$/g, "");
  const segments = normalizedPath.split("/").filter(Boolean);

  if (segments.length <= 4) {
    return normalizedPath;
  }

  return `.../${segments.slice(-4).join("/")}`;
}
