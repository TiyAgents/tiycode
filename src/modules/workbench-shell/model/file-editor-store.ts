import { createStore, useStore as useStoreBase, shallowEqual } from "@/shared/lib/create-store";
import { fileRead, fileWrite } from "@/services/bridge/file-commands";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface FileTab {
  workspaceId: string;
  path: string;
  content: string | null;
  originalContent: string | null;
  language: string;
  isDirty: boolean;
  isLoading: boolean;
  isPinned: boolean;
  isBinary: boolean;
  previewMode: "editor" | "preview";
  error: string | null;
}

export interface FileEditorState {
  [key: string]: unknown;
  tabs: FileTab[];
  activeTabKey: string | null;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_TABS = 10;
const AUTO_SAVE_DELAY_MS = 10_000;

// ---------------------------------------------------------------------------
// Tab identity
// ---------------------------------------------------------------------------

export function getFileTabKey(workspaceId: string, path: string): string {
  return `${workspaceId}\u0000${path}`;
}

function getTabKey(tab: Pick<FileTab, "workspaceId" | "path">): string {
  return getFileTabKey(tab.workspaceId, tab.path);
}

function matchesTab(tab: FileTab, workspaceId: string, path: string): boolean {
  return tab.workspaceId === workspaceId && tab.path === path;
}

function findTab(tabs: FileTab[], workspaceId: string, path: string): FileTab | undefined {
  return tabs.find((tab) => matchesTab(tab, workspaceId, path));
}

// ---------------------------------------------------------------------------
// Language detection
// ---------------------------------------------------------------------------

const EXTENSION_LANGUAGE_MAP: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  css: "css",
  scss: "css",
  less: "css",
  html: "html",
  htm: "html",
  xml: "html",
  md: "markdown",
  mdx: "markdown",
  rs: "rust",
  py: "python",
  toml: "plaintext",
  yaml: "plaintext",
  yml: "plaintext",
  sh: "plaintext",
  bash: "plaintext",
  zsh: "plaintext",
  txt: "plaintext",
  svg: "html",
};

/** Preview-capable file types */
const PREVIEW_EXTENSIONS = new Set(["md", "mdx", "html", "htm", "svg"]);

export function detectLanguage(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return EXTENSION_LANGUAGE_MAP[ext] ?? "plaintext";
}

export function isPreviewable(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return PREVIEW_EXTENSIONS.has(ext);
}

export function isImageFile(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return ["png", "jpg", "jpeg", "gif", "webp", "ico", "bmp", "svg"].includes(ext);
}

// ---------------------------------------------------------------------------
// Initial state
// ---------------------------------------------------------------------------

function getInitialState(): FileEditorState {
  return {
    tabs: [],
    activeTabKey: null,
  };
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const fileEditorStore = createStore<FileEditorState>(getInitialState());

// ---------------------------------------------------------------------------
// Auto-save debounce management
// ---------------------------------------------------------------------------

const autoSaveTimers = new Map<string, ReturnType<typeof setTimeout>>();

function scheduleAutoSave(workspaceId: string, path: string): void {
  const key = getFileTabKey(workspaceId, path);
  cancelAutoSave(workspaceId, path);
  const timer = setTimeout(() => {
    autoSaveTimers.delete(key);
    void saveFile(workspaceId, path);
  }, AUTO_SAVE_DELAY_MS);
  autoSaveTimers.set(key, timer);
}

function cancelAutoSave(workspaceId: string, path: string): void {
  const key = getFileTabKey(workspaceId, path);
  const existing = autoSaveTimers.get(key);
  if (existing) {
    clearTimeout(existing);
    autoSaveTimers.delete(key);
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

export function setActiveTab(workspaceId: string, path: string): void {
  fileEditorStore.setState({ activeTabKey: getFileTabKey(workspaceId, path) });
}

/** Open a file in the editor. Single-click = preview tab, replaces existing preview. */
export async function openFile(
  workspaceId: string,
  path: string,
  pin: boolean = false,
): Promise<void> {
  const state = fileEditorStore.getState();
  const tabKey = getFileTabKey(workspaceId, path);

  // Already open in this workspace → activate
  const existing = findTab(state.tabs, workspaceId, path);
  if (existing) {
    fileEditorStore.setState({
      activeTabKey: tabKey,
      tabs: pin && !existing.isPinned
        ? state.tabs.map((tab) => (matchesTab(tab, workspaceId, path) ? { ...tab, isPinned: true } : tab))
        : state.tabs,
    });
    return;
  }

  // Build new tab
  const newTab: FileTab = {
    workspaceId,
    path,
    content: null,
    originalContent: null,
    language: detectLanguage(path),
    isDirty: false,
    isLoading: true,
    isPinned: pin,
    isBinary: false,
    previewMode: isPreviewable(path) ? "preview" : "editor",
    error: null,
  };

  // Replace unpinned preview tab from the same workspace, or add new
  let nextTabs: FileTab[];
  if (!pin) {
    const unpinnedIdx = state.tabs.findIndex(
      (tab) => tab.workspaceId === workspaceId && !tab.isPinned && !tab.isDirty,
    );
    if (unpinnedIdx >= 0) {
      cancelAutoSave(state.tabs[unpinnedIdx].workspaceId, state.tabs[unpinnedIdx].path);
      nextTabs = [...state.tabs];
      nextTabs[unpinnedIdx] = newTab;
    } else {
      nextTabs = [...state.tabs, newTab];
    }
  } else {
    nextTabs = [...state.tabs, newTab];
  }

  // Enforce MAX_TABS — evict oldest unpinned, non-dirty tab
  while (nextTabs.length > MAX_TABS) {
    const evictIdx = nextTabs.findIndex((tab) => !tab.isPinned && !tab.isDirty && getTabKey(tab) !== tabKey);
    if (evictIdx >= 0) {
      cancelAutoSave(nextTabs[evictIdx].workspaceId, nextTabs[evictIdx].path);
      nextTabs.splice(evictIdx, 1);
    } else {
      break;
    }
  }

  fileEditorStore.setState({ tabs: nextTabs, activeTabKey: tabKey });

  // Load content
  try {
    const dto = await fileRead(workspaceId, path);
    const isBinaryOnly = dto.isBinary && !dto.content;
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((tab) =>
        matchesTab(tab, workspaceId, path)
          ? {
              ...tab,
              content: dto.content,
              originalContent: dto.isBinary ? null : dto.content,
              isLoading: false,
              isBinary: isBinaryOnly,
              error: null,
            }
          : tab,
      ),
    }));
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((tab) =>
        matchesTab(tab, workspaceId, path)
          ? { ...tab, isLoading: false, isBinary: false, error: message }
          : tab,
      ),
    }));
  }
}

/** Pin a preview tab so it won't be replaced. */
export function pinTab(workspaceId: string, path: string): void {
  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((tab) => (matchesTab(tab, workspaceId, path) ? { ...tab, isPinned: true } : tab)),
  }));
}

/** Close a tab. */
export function closeTab(workspaceId: string, path: string): void {
  const tabKey = getFileTabKey(workspaceId, path);
  cancelAutoSave(workspaceId, path);
  fileEditorStore.setState((prev) => {
    const nextTabs = prev.tabs.filter((tab) => !matchesTab(tab, workspaceId, path));
    let nextActive = prev.activeTabKey;
    if (nextActive === tabKey) {
      const sameWorkspaceTabs = nextTabs.filter((tab) => tab.workspaceId === workspaceId);
      const fallbackTab = sameWorkspaceTabs[sameWorkspaceTabs.length - 1] ?? null;
      nextActive = fallbackTab ? getTabKey(fallbackTab) : null;
    }
    return { tabs: nextTabs, activeTabKey: nextActive };
  });
}

/** Close all tabs. */
export function closeAllTabs(): void {
  for (const [, timer] of autoSaveTimers) {
    clearTimeout(timer);
  }
  autoSaveTimers.clear();
  fileEditorStore.setState({ tabs: [], activeTabKey: null });
}

/** Update content (from editor onChange). Marks dirty + schedules auto-save. */
export function updateContent(workspaceId: string, path: string, content: string): void {
  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((tab) => {
      if (!matchesTab(tab, workspaceId, path)) return tab;
      const isDirty = content !== tab.originalContent;
      return { ...tab, content, isDirty };
    }),
  }));
  // Schedule auto-save
  const tab = findTab(fileEditorStore.getState().tabs, workspaceId, path);
  if (tab?.isDirty) {
    scheduleAutoSave(workspaceId, path);
  }
}

/** Save file immediately. */
export async function saveFile(workspaceId: string, path: string): Promise<void> {
  cancelAutoSave(workspaceId, path);
  const tab = findTab(fileEditorStore.getState().tabs, workspaceId, path);
  if (!tab || tab.content === null || !tab.isDirty) return;

  try {
    await fileWrite(workspaceId, path, tab.content);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((current) =>
        matchesTab(current, workspaceId, path)
          ? { ...current, isDirty: false, originalContent: current.content, error: null }
          : current,
      ),
    }));
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((current) =>
        matchesTab(current, workspaceId, path) ? { ...current, error: `Save failed: ${message}` } : current,
      ),
    }));
  }
}

/** Toggle preview mode for a tab. */
export function setPreviewMode(workspaceId: string, path: string, mode: "editor" | "preview"): void {
  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((tab) => (matchesTab(tab, workspaceId, path) ? { ...tab, previewMode: mode } : tab)),
  }));
}

/** Reload a tab's content from disk (e.g., after external change). */
export async function reloadTab(workspaceId: string, path: string): Promise<void> {
  const tab = findTab(fileEditorStore.getState().tabs, workspaceId, path);
  if (!tab) return;

  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((current) =>
      matchesTab(current, workspaceId, path) ? { ...current, isLoading: true, isBinary: false } : current,
    ),
  }));

  try {
    const dto = await fileRead(workspaceId, path);
    const isBinaryOnly = dto.isBinary && !dto.content;
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((current) =>
        matchesTab(current, workspaceId, path)
          ? {
              ...current,
              content: dto.content,
              originalContent: dto.isBinary ? null : dto.content,
              isDirty: false,
              isLoading: false,
              isBinary: isBinaryOnly,
              error: null,
            }
          : current,
      ),
    }));
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((current) =>
        matchesTab(current, workspaceId, path)
          ? { ...current, isLoading: false, isBinary: false, error: message }
          : current,
      ),
    }));
  }
}

// ---------------------------------------------------------------------------
// React hooks
// ---------------------------------------------------------------------------

export function useActiveTab(workspaceId?: string): FileTab | null {
  return useStoreBase(
    fileEditorStore,
    (state) => {
      const active = state.tabs.find((tab) => getTabKey(tab) === state.activeTabKey) ?? null;
      if (!workspaceId) return active;
      return active?.workspaceId === workspaceId ? active : null;
    },
  );
}

export function useOpenTabs(workspaceId?: string): FileTab[] {
  return useStoreBase(
    fileEditorStore,
    (state) => workspaceId ? state.tabs.filter((tab) => tab.workspaceId === workspaceId) : state.tabs,
    shallowEqual,
  );
}

export function useIsEditorMode(workspaceId?: string): boolean {
  return useStoreBase(
    fileEditorStore,
    (state) => workspaceId
      ? state.tabs.some((tab) => tab.workspaceId === workspaceId)
      : state.tabs.length > 0,
  );
}

export function useActiveTabPath(workspaceId?: string): string | null {
  return useStoreBase(
    fileEditorStore,
    (state) => {
      const active = state.tabs.find((tab) => getTabKey(tab) === state.activeTabKey) ?? null;
      if (!active) return null;
      if (workspaceId && active.workspaceId !== workspaceId) return null;
      return active.path;
    },
  );
}
