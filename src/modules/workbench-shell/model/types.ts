export type ThreadStatus = "running" | "completed" | "needs-reply" | "failed" | "interrupted";

/**
 * Unified thread run status — superset of {@link ThreadStatus} and {@link RunState}.
 *
 * Used as the authoritative status value in {@link threadStore.threadStatuses}.
 * Sidebar consumers map this down to a display subset via
 * {@link threadRunStatusToDisplayStatus}.
 */
export type ThreadRunStatus =
  | "idle"
  | "running"
  | "waiting_approval"
  | "needs_reply"
  | "completed"
  | "failed"
  | "cancelled"
  | "interrupted"
  | "limit_reached";

/**
 * Map a {@link ThreadRunStatus} value to the subset used for sidebar display
 * (matching the legacy {@link ThreadStatus} union).
 */
export function threadRunStatusToDisplayStatus(
  status: ThreadRunStatus,
): ThreadStatus {
  switch (status) {
    case "idle":
    case "completed":
    case "cancelled":
      return "completed";
    case "waiting_approval":
    case "needs_reply":
    case "limit_reached":
      return "needs-reply";
    case "running":
      return "running";
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
  }
}

export type ThreadItem = {
  name: string;
  time: string;
  active: boolean;
  status: ThreadStatus;
};

export type WorkspaceThreadItem = ThreadItem & {
  id: string;
  profileId?: string | null;
};

export type WorkspaceItem = {
  id: string;
  name: string;
  defaultOpen: boolean;
  threads: Array<WorkspaceThreadItem>;
  path?: string;
  kind?: "standalone" | "repo" | "worktree";
  parentWorkspaceId?: string | null;
  worktreeHash?: string | null;
  branch?: string | null;
  createdAt?: string;
};

export type ProjectOption = {
  id: string;
  name: string;
  path: string;
  lastOpenedLabel: string;
  kind?: "standalone" | "repo" | "worktree";
  parentWorkspaceId?: string | null;
  worktreeHash?: string | null;
  branch?: string | null;
};

export type WorkspaceOpenApp = {
  id: string;
  name: string;
  openWith: string | null;
  iconDataUrl: string | null;
};

export type DrawerPanel = "project" | "git";

export type WorkbenchOverlay = "settings" | "marketplace" | null;

export type ProjectTreeItem = {
  id: string;
  name: string;
  kind: "folder" | "file";
  icon: "folder" | "git" | "json" | "html" | "css" | "license" | "readme" | "ts" | "file";
  ignored?: boolean;
};

export type GitChangeFile = {
  id: string;
  path: string;
  status: "M" | "A" | "D";
  icon: ProjectTreeItem["icon"];
  summary: string;
  initialStaged: boolean;
};

export type GitHistoryItem = {
  id: string;
  subject: string;
  hash: string;
  relativeTime: string;
  author: string;
  refs?: ReadonlyArray<string>;
};

export type GitDiffLine = {
  kind: "context" | "add" | "remove";
  oldNumber: number | null;
  newNumber: number | null;
  text: string;
};

export type GitDiffPreview = {
  meta: ReadonlyArray<string>;
  lines: ReadonlyArray<GitDiffLine>;
};

export type GitSplitDiffRow = {
  kind: "context" | "modified" | "add" | "remove";
  leftNumber: number | null;
  rightNumber: number | null;
  leftText: string;
  rightText: string;
};

export type MockUserSession = {
  name: string;
  avatar: string;
  email: string;
};

export type PanelVisibilityState = {
  isSidebarOpen: boolean;
  isDrawerOpen: boolean;
};
