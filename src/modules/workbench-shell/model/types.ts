export type ThreadStatus = "running" | "completed" | "needs-reply" | "failed";

export type ThreadItem = {
  name: string;
  time: string;
  active: boolean;
  status: ThreadStatus;
};

export type WorkspaceThreadItem = ThreadItem & {
  id: string;
};

export type WorkspaceItem = {
  id: string;
  name: string;
  defaultOpen: boolean;
  threads: Array<WorkspaceThreadItem>;
  path?: string;
};

export type ProjectOption = {
  id: string;
  name: string;
  path: string;
  lastOpenedLabel: string;
};

export type WorkspaceOpenApp = {
  id: string;
  name: string;
  openWith: string | null;
  iconDataUrl: string | null;
};

export type DrawerPanel = "project" | "git";

export type ProjectTreeItem = {
  id: string;
  name: string;
  kind: "folder" | "file";
  icon: "folder" | "git" | "json" | "html" | "css" | "license" | "readme" | "ts";
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
  isTerminalCollapsed: boolean;
};
