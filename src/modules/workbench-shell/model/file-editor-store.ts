import { createStore, useStore as useStoreBase, shallowEqual } from "@/shared/lib/create-store";
import { fileRead, fileWrite } from "@/services/bridge/file-commands";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface FileTab {
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
  activeTabPath: string | null;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_TABS = 10;
const AUTO_SAVE_DELAY_MS = 10_000;

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
    activeTabPath: null,
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
  cancelAutoSave(path);
  const timer = setTimeout(() => {
    autoSaveTimers.delete(path);
    void saveFile(workspaceId, path);
  }, AUTO_SAVE_DELAY_MS);
  autoSaveTimers.set(path, timer);
}

function cancelAutoSave(path: string): void {
  const existing = autoSaveTimers.get(path);
  if (existing) {
    clearTimeout(existing);
    autoSaveTimers.delete(path);
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/** Open a file in the editor. Single-click = preview tab, replaces existing preview. */
export async function openFile(
  workspaceId: string,
  path: string,
  pin: boolean = false,
): Promise<void> {
  const state = fileEditorStore.getState();

  // Already open → activate
  const existing = state.tabs.find((t) => t.path === path);
  if (existing) {
    fileEditorStore.setState({
      activeTabPath: path,
      tabs: pin && !existing.isPinned
        ? state.tabs.map((t) => (t.path === path ? { ...t, isPinned: true } : t))
        : state.tabs,
    });
    return;
  }

  // Build new tab
  const newTab: FileTab = {
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

  // Replace unpinned preview tab, or add new
  let nextTabs: FileTab[];
  if (!pin) {
    const unpinnedIdx = state.tabs.findIndex((t) => !t.isPinned && !t.isDirty);
    if (unpinnedIdx >= 0) {
      cancelAutoSave(state.tabs[unpinnedIdx].path);
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
    const evictIdx = nextTabs.findIndex((t) => !t.isPinned && !t.isDirty && t.path !== path);
    if (evictIdx >= 0) {
      cancelAutoSave(nextTabs[evictIdx].path);
      nextTabs.splice(evictIdx, 1);
    } else {
      break;
    }
  }

  fileEditorStore.setState({ tabs: nextTabs, activeTabPath: path });

  // Load content
  try {
    const dto = await fileRead(workspaceId, path);
    const isBinaryOnly = dto.isBinary && !dto.content;
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((t) =>
        t.path === path
          ? {
              ...t,
              content: dto.content,
              originalContent: dto.isBinary ? null : dto.content,
              isLoading: false,
              isBinary: isBinaryOnly,
              error: null,
            }
          : t,
      ),
    }));
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((t) =>
        t.path === path ? { ...t, isLoading: false, isBinary: false, error: message } : t,
      ),
    }));
  }
}

/** Pin a preview tab so it won't be replaced. */
export function pinTab(path: string): void {
  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((t) => (t.path === path ? { ...t, isPinned: true } : t)),
  }));
}

/** Close a tab. */
export function closeTab(path: string): void {
  cancelAutoSave(path);
  fileEditorStore.setState((prev) => {
    const nextTabs = prev.tabs.filter((t) => t.path !== path);
    let nextActive = prev.activeTabPath;
    if (nextActive === path) {
      nextActive = nextTabs.length > 0 ? nextTabs[nextTabs.length - 1].path : null;
    }
    return { tabs: nextTabs, activeTabPath: nextActive };
  });
}

/** Close all tabs. */
export function closeAllTabs(): void {
  for (const [path] of autoSaveTimers) {
    cancelAutoSave(path);
  }
  fileEditorStore.setState({ tabs: [], activeTabPath: null });
}

/** Update content (from editor onChange). Marks dirty + schedules auto-save. */
export function updateContent(workspaceId: string, path: string, content: string): void {
  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((t) => {
      if (t.path !== path) return t;
      const isDirty = content !== t.originalContent;
      return { ...t, content, isDirty };
    }),
  }));
  // Schedule auto-save
  const tab = fileEditorStore.getState().tabs.find((t) => t.path === path);
  if (tab?.isDirty) {
    scheduleAutoSave(workspaceId, path);
  }
}

/** Save file immediately. */
export async function saveFile(workspaceId: string, path: string): Promise<void> {
  cancelAutoSave(path);
  const tab = fileEditorStore.getState().tabs.find((t) => t.path === path);
  if (!tab || tab.content === null || !tab.isDirty) return;

  try {
    await fileWrite(workspaceId, path, tab.content);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((t) =>
        t.path === path
          ? { ...t, isDirty: false, originalContent: t.content, error: null }
          : t,
      ),
    }));
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((t) =>
        t.path === path ? { ...t, error: `Save failed: ${message}` } : t,
      ),
    }));
  }
}

/** Toggle preview mode for a tab. */
export function setPreviewMode(path: string, mode: "editor" | "preview"): void {
  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((t) => (t.path === path ? { ...t, previewMode: mode } : t)),
  }));
}

/** Reload a tab's content from disk (e.g., after external change). */
export async function reloadTab(workspaceId: string, path: string): Promise<void> {
  const tab = fileEditorStore.getState().tabs.find((t) => t.path === path);
  if (!tab) return;

  fileEditorStore.setState((prev) => ({
    tabs: prev.tabs.map((t) => (t.path === path ? { ...t, isLoading: true, isBinary: false } : t)),
  }));

  try {
    const dto = await fileRead(workspaceId, path);
    const isBinaryOnly = dto.isBinary && !dto.content;
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((t) =>
        t.path === path
          ? {
              ...t,
              content: dto.content,
              originalContent: dto.isBinary ? null : dto.content,
              isDirty: false,
              isLoading: false,
              isBinary: isBinaryOnly,
              error: null,
            }
          : t,
      ),
    }));
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    fileEditorStore.setState((prev) => ({
      tabs: prev.tabs.map((t) =>
        t.path === path ? { ...t, isLoading: false, isBinary: false, error: message } : t,
      ),
    }));
  }
}

// ---------------------------------------------------------------------------
// React hooks
// ---------------------------------------------------------------------------

export function useActiveTab(): FileTab | null {
  return useStoreBase(
    fileEditorStore,
    (s) => s.tabs.find((t) => t.path === s.activeTabPath) ?? null,
  );
}

export function useOpenTabs(): FileTab[] {
  return useStoreBase(fileEditorStore, (s) => s.tabs, shallowEqual);
}

export function useIsEditorMode(): boolean {
  return useStoreBase(fileEditorStore, (s) => s.tabs.length > 0);
}

export function useActiveTabPath(): string | null {
  return useStoreBase(fileEditorStore, (s) => s.activeTabPath);
}
