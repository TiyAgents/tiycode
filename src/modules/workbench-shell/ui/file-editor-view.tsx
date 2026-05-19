import { useRef, useEffect, useCallback, useState, useMemo, type FC } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { EditorView, basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { markdown } from "@codemirror/lang-markdown";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { keymap } from "@codemirror/view";
import type { ViewUpdate } from "@codemirror/view";
import { X, Eye, Code2, Save, Loader2, AlertCircle, RefreshCw } from "lucide-react";
import { MessageResponse } from "@/components/ai-elements/message";
import { cn } from "@/shared/lib/utils";
import { useT } from "@/i18n";
import {
  type FileTab,
  useOpenTabs,
  useActiveTab,
  getFileTabKey,
  setActiveTab,
  closeTab,
  pinTab,
  setPreviewMode,
  saveFile,
  updateContent,
  isPreviewable,
  isImageFile,
  fileEditorStore,
  reloadTab,
} from "@/modules/workbench-shell/model/file-editor-store";
import { fileRead, type FileContentDto } from "@/services/bridge/file-commands";
import { isLocalRelativeUrl, normalizeRelativePath } from "./markdown-preview-paths";

// ---------------------------------------------------------------------------
// Language extension resolver
// ---------------------------------------------------------------------------

function getLanguageExtension(lang: string) {
  switch (lang) {
    case "typescript":
      return javascript({ jsx: true, typescript: true });
    case "javascript":
      return javascript({ jsx: true });
    case "json":
      return json();
    case "css":
      return css();
    case "html":
      return html();
    case "markdown":
      return markdown();
    case "rust":
      return rust();
    case "python":
      return python();
    default:
      return [];
  }
}

// ---------------------------------------------------------------------------
// CodeMirror Editor
// ---------------------------------------------------------------------------

const EDITOR_STORE_SYNC_DELAY_MS = 120;
const editorContentFlushers = new Map<string, () => void>();
const inFlightMarkdownImageReads = new Map<string, Promise<FileContentDto>>();

function markdownImageCacheKey(workspaceId: string, path: string): string {
  return `${workspaceId}\u0000${path}`;
}

function readMarkdownImage(workspaceId: string, path: string): Promise<FileContentDto> {
  const cacheKey = markdownImageCacheKey(workspaceId, path);
  const cached = inFlightMarkdownImageReads.get(cacheKey);
  if (cached) return cached;

  const request = fileRead(workspaceId, path).finally(() => {
    inFlightMarkdownImageReads.delete(cacheKey);
  });
  inFlightMarkdownImageReads.set(cacheKey, request);
  return request;
}

function flushEditorContent(workspaceId: string, path: string): void {
  editorContentFlushers.get(getFileTabKey(workspaceId, path))?.();
}

interface CodeMirrorEditorProps {
  content: string;
  language: string;
  workspaceId: string;
  path: string;
}

const CodeMirrorEditor: FC<CodeMirrorEditorProps> = ({
  content,
  language,
  workspaceId,
  path,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const pathRef = useRef(path);
  const workspaceIdRef = useRef(workspaceId);
  const pendingSyncRef = useRef<{
    workspaceId: string;
    path: string;
    doc: string;
  } | null>(null);
  const syncTimerRef = useRef<number | null>(null);

  pathRef.current = path;
  workspaceIdRef.current = workspaceId;

  const clearPendingSync = useCallback(() => {
    if (syncTimerRef.current !== null) {
      window.clearTimeout(syncTimerRef.current);
      syncTimerRef.current = null;
    }
  }, []);

  const flushPendingContent = useCallback(() => {
    const pendingSync = pendingSyncRef.current;
    if (pendingSync === null) return;
    pendingSyncRef.current = null;
    clearPendingSync();
    updateContent(pendingSync.workspaceId, pendingSync.path, pendingSync.doc);
  }, [clearPendingSync]);

  const scheduleContentSync = useCallback((doc: string) => {
    pendingSyncRef.current = {
      workspaceId: workspaceIdRef.current,
      path: pathRef.current,
      doc,
    };
    clearPendingSync();
    syncTimerRef.current = window.setTimeout(() => {
      syncTimerRef.current = null;
      flushPendingContent();
    }, EDITOR_STORE_SYNC_DELAY_MS);
  }, [clearPendingSync, flushPendingContent]);

  useEffect(() => {
    const key = getFileTabKey(workspaceId, path);
    editorContentFlushers.set(key, flushPendingContent);
    return () => {
      if (editorContentFlushers.get(key) === flushPendingContent) {
        editorContentFlushers.delete(key);
      }
    };
  }, [flushPendingContent, path, workspaceId]);

  // Cmd+S / Ctrl+S save handler
  const saveKeymap = keymap.of([
    {
      key: "Mod-s",
      run: () => {
        flushPendingContent();
        void saveFile(workspaceIdRef.current, pathRef.current);
        return true;
      },
    },
  ]);

  useEffect(() => {
    if (!containerRef.current) return;

    const state = EditorState.create({
      doc: content,
      extensions: [
        basicSetup,
        oneDark,
        getLanguageExtension(language),
        saveKeymap,
        EditorView.updateListener.of((update: ViewUpdate) => {
          if (update.docChanged) {
            const doc = update.state.doc.toString();
            scheduleContentSync(doc);
          }
        }),
        EditorView.theme({
          "&": { height: "100%", fontSize: "12px" },
          ".cm-scroller": { overflow: "auto" },
          ".cm-content": { fontFamily: "var(--font-mono, monospace)" },
        }),
      ],
    });

    const view = new EditorView({
      state,
      parent: containerRef.current,
    });
    viewRef.current = view;

    const refreshFrame = window.requestAnimationFrame(() => {
      view.requestMeasure();
    });
    const resizeObserver = typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(() => {
          view.requestMeasure();
        });
    resizeObserver?.observe(containerRef.current);

    return () => {
      window.cancelAnimationFrame(refreshFrame);
      resizeObserver?.disconnect();
      flushPendingContent();
      view.destroy();
      viewRef.current = null;
    };
    // Re-create editor when path or language changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, language]);

  // Sync external content changes (e.g., after Agent modifies file or reloadTab)
  const lastSyncedContentRef = useRef(content);
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    // Skip if content hasn't actually changed from what we last synced
    if (content === lastSyncedContentRef.current) return;
    lastSyncedContentRef.current = content;
    // Skip if the editor's current doc matches (user typed it)
    const currentDoc = view.state.doc.toString();
    if (currentDoc === content) return;
    // Replace the editor document
    view.dispatch({
      changes: { from: 0, to: currentDoc.length, insert: content },
    });
  }, [content]);

  return <div ref={containerRef} className="file-editor-codemirror h-full w-full overflow-hidden" />;
};

// ---------------------------------------------------------------------------
// Preview renderers
// ---------------------------------------------------------------------------

/**
 * An `<img>` replacement for markdown preview that resolves workspace-relative
 * image paths via the existing `fileRead` command (returns base64 data URIs
 * for image files).  Remote URLs pass through unchanged.
 */
const WorkspaceImage: FC<
  React.ImgHTMLAttributes<HTMLImageElement> & {
    workspaceId: string;
    /** Directory of the markdown file relative to the workspace root (e.g. "" or "docs"). */
    fileDir: string;
  }
> = ({ src, workspaceId, fileDir, ...rest }) => {
  const [resolved, setResolved] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const isLocal = isLocalRelativeUrl(src);

  useEffect(() => {
    setFailed(false);

    if (!isLocal || !src) {
      setResolved(src ?? null);
      return;
    }

    setResolved(null);

    let cancelled = false;
    const relative = normalizeRelativePath(fileDir ? `${fileDir}/${src}` : src);

    readMarkdownImage(workspaceId, relative)
      .then((dto) => {
        if (cancelled) return;
        // fileRead returns data:<mime>;base64,… for image files
        if (dto.content) setResolved(dto.content);
        else setFailed(true);
      })
      .catch(() => { if (!cancelled) setFailed(true); });

    return () => { cancelled = true; };
  }, [src, workspaceId, fileDir, isLocal]);

  if (failed) {
    // Fall back to showing the original (likely broken) img so alt text is visible
    return <img src={src} {...rest} />;
  }
  if (resolved === null && isLocal) {
    // Still loading — show a tiny placeholder to avoid layout shift
    return (
      <span className="inline-block h-4 w-4 animate-pulse rounded bg-muted" />
    );
  }
  return <img src={resolved ?? src} {...rest} />;
};

interface MarkdownPreviewProps {
  content: string;
  workspaceId: string;
  /** Relative path of the previewed file within the workspace (e.g. "README.md"). */
  filePath: string;
}

const MarkdownPreview: FC<MarkdownPreviewProps> = ({ content, workspaceId, filePath }) => {
  const fileDir = filePath.split("/").slice(0, -1).join("/");

  const components = useMemo(
    () => ({
      img: (props: React.ImgHTMLAttributes<HTMLImageElement>) => (
        <WorkspaceImage {...props} workspaceId={workspaceId} fileDir={fileDir} />
      ),
    }),
    [workspaceId, fileDir],
  );

  return (
    <div className="h-full overflow-auto bg-app-canvas/70 px-4 py-3 text-app-foreground">
      <div className="mx-auto max-w-4xl text-sm leading-6">
        <MessageResponse components={components}>{content}</MessageResponse>
      </div>
    </div>
  );
};

const HtmlPreview: FC<{ content: string }> = ({ content }) => {
  const t = useT();
  return (
    <iframe
      sandbox="allow-scripts"
      srcDoc={content}
      className="h-full w-full border-0 bg-white"
      title={t("fileEditor.htmlPreview")}
    />
  );
};

const ImagePreview: FC<{ content: string; path: string }> = ({ content, path }) => (
  <div className="flex h-full items-center justify-center overflow-auto p-4">
    <img
      src={content}
      alt={path.split("/").pop() ?? "image"}
      className="max-h-full max-w-full object-contain"
    />
  </div>
);

// ---------------------------------------------------------------------------
// Tab bar
// ---------------------------------------------------------------------------

interface TabBarProps {
  tabs: FileTab[];
  activeTabKey: string | null;
}

const TabBar: FC<TabBarProps> = ({ tabs, activeTabKey }) => (
  <div className="flex min-h-0 items-center gap-0 overflow-x-auto border-b border-app-border bg-app-drawer/50">
    {tabs.map((tab) => {
      const fileName = tab.path.split("/").pop() ?? tab.path;
      const tabKey = getFileTabKey(tab.workspaceId, tab.path);
      const isActive = tabKey === activeTabKey;
      return (
        <button
          key={tabKey}
          className={cn(
            "group relative flex shrink-0 items-center gap-1 border-r border-app-border px-2.5 py-1.5 text-[11px] transition-colors",
            isActive
              ? "bg-app-drawer text-foreground"
              : "text-muted-foreground hover:text-foreground hover:bg-app-drawer/80",
          )}
          onClick={() => {
            setActiveTab(tab.workspaceId, tab.path);
          }}
          onDoubleClick={() => pinTab(tab.workspaceId, tab.path)}
          onMouseDown={(e) => {
            // Middle-click to close
            if (e.button === 1) {
              e.preventDefault();
              closeTab(tab.workspaceId, tab.path);
            }
          }}
        >
          <span className={cn("max-w-[120px] truncate", !tab.isPinned && "italic")}>
            {fileName}
          </span>
          {tab.isDirty && (
            <span className="inline-block size-1.5 shrink-0 rounded-full bg-yellow-400" />
          )}
          {tab.error && !tab.isDirty && (
            <AlertCircle className="size-3 shrink-0 text-destructive" />
          )}
          <span
            className="ml-0.5 inline-flex size-4 shrink-0 items-center justify-center rounded opacity-0 transition-opacity hover:bg-muted group-hover:opacity-100"
            onClick={(e) => {
              e.stopPropagation();
              closeTab(tab.workspaceId, tab.path);
            }}
          >
            <X className="size-3" />
          </span>
        </button>
      );
    })}
  </div>
);

// ---------------------------------------------------------------------------
// Toolbar
// ---------------------------------------------------------------------------

interface ToolbarProps {
  tab: FileTab;
}

const EditorToolbar: FC<ToolbarProps> = ({ tab }) => {
  const t = useT();
  const canPreview = isPreviewable(tab.path);
  return (
    <div className="flex items-center justify-between border-b border-app-border px-2 py-1">
      <div className="flex items-center gap-1">
        {canPreview && (
          <>
            <button
              title={t("fileEditor.editor")}
              className={cn(
                "rounded p-1 text-xs transition-colors",
                tab.previewMode === "editor"
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => setPreviewMode(tab.workspaceId, tab.path, "editor")}
            >
              <Code2 className="size-3.5" />
            </button>
            <button
              title={t("fileEditor.preview")}
              className={cn(
                "rounded p-1 text-xs transition-colors",
                tab.previewMode === "preview"
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => setPreviewMode(tab.workspaceId, tab.path, "preview")}
            >
              <Eye className="size-3.5" />
            </button>
          </>
        )}
      </div>
      <div className="flex items-center gap-1">
        <button
          title={tab.isDirty ? t("fileEditor.refreshDisabledDirty") : t("fileEditor.refresh")}
          className="rounded p-1 text-xs text-muted-foreground transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:text-muted-foreground"
          disabled={tab.isDirty || tab.isLoading}
          onClick={() => {
            void reloadTab(tab.workspaceId, tab.path);
          }}
        >
          <RefreshCw className={cn("size-3.5", tab.isLoading && "animate-spin")} />
        </button>
        {tab.isDirty && (
          <button
            title={t("fileEditor.saveShortcut")}
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            onClick={() => {
              flushEditorContent(tab.workspaceId, tab.path);
              void saveFile(tab.workspaceId, tab.path);
            }}
          >
            <Save className="size-3" />
            <span>{t("fileEditor.save")}</span>
          </button>
        )}
        {tab.error && (
          <span title={tab.error}>
            <AlertCircle className="size-3.5 text-destructive" />
          </span>
        )}
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Main FileEditorView
// ---------------------------------------------------------------------------

export interface FileEditorViewProps {
  workspaceId: string;
}

export const FileEditorView: FC<FileEditorViewProps> = ({ workspaceId }) => {
  const t = useT();
  const tabs = useOpenTabs(workspaceId);
  const activeTab = useActiveTab(workspaceId);
  const activeTabKey = activeTab ? getFileTabKey(activeTab.workspaceId, activeTab.path) : null;

  useEffect(() => {
    if (!activeTab && tabs.length > 0) {
      const fallback = tabs[tabs.length - 1];
      setActiveTab(fallback.workspaceId, fallback.path);
    }
  }, [activeTab, tabs]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (activeTab) {
          flushEditorContent(activeTab.workspaceId, activeTab.path);
          void saveFile(activeTab.workspaceId, activeTab.path);
        }
      }
    },
    [activeTab],
  );

  // Listen for agent file-changed events to reload affected tabs
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    void listen<{ path: string }>("agent-file-changed", (event) => {
      if (cancelled) return;
      const { path } = event.payload;
      const tab = fileEditorStore.getState().tabs.find(
        (item) => item.workspaceId === workspaceId && item.path === path,
      );
      if (tab && !tab.isDirty) {
        void reloadTab(workspaceId, path);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => { cancelled = true; unlisten?.(); };
  }, [workspaceId]);

  if (tabs.length === 0) return null;

  return (
    <div className="flex h-full min-h-0 flex-col" onKeyDown={handleKeyDown}>
      <TabBar
        tabs={tabs}
        activeTabKey={activeTabKey}
      />
      {activeTab && (
        <>
          <EditorToolbar tab={activeTab} />
          <div className="min-h-0 flex-1 overflow-hidden">
            {activeTab.isLoading ? (
              <div className="flex h-full items-center justify-center text-muted-foreground">
                <Loader2 className="mr-2 size-4 animate-spin" />
                <span className="text-xs">{t("fileEditor.loading")}</span>
              </div>
            ) : (activeTab.isBinary || (activeTab.error && activeTab.content === null)) ? (
              <div className="flex h-full items-center justify-center p-4 text-center">
                <div>
                  <AlertCircle className="mx-auto mb-2 size-5 text-destructive" />
                  <p className="text-xs text-muted-foreground">
                    {activeTab.isBinary ? t("fileEditor.binaryFile") : activeTab.error}
                  </p>
                </div>
              </div>
            ) : isImageFile(activeTab.path) && activeTab.content ? (
              <ImagePreview content={activeTab.content} path={activeTab.path} />
            ) : activeTab.previewMode === "preview" && activeTab.content !== null ? (
              activeTab.path.endsWith(".html") || activeTab.path.endsWith(".htm") ? (
                <HtmlPreview content={activeTab.content} />
              ) : (
                <MarkdownPreview content={activeTab.content} workspaceId={workspaceId} filePath={activeTab.path} />
              )
            ) : activeTab.content !== null ? (
              <CodeMirrorEditor
                content={activeTab.content}
                language={activeTab.language}
                workspaceId={workspaceId}
                path={activeTab.path}
              />
            ) : null}
          </div>
        </>
      )}
    </div>
  );
};
