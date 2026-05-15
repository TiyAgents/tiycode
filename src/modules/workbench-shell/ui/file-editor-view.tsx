import { useRef, useEffect, useCallback, type FC } from "react";
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
import { X, Eye, Code2, Save, Loader2, AlertCircle } from "lucide-react";
import { MessageResponse } from "@/components/ai-elements/message";
import { cn } from "@/shared/lib/utils";
import {
  type FileTab,
  useOpenTabs,
  useActiveTab,
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

  pathRef.current = path;
  workspaceIdRef.current = workspaceId;

  // Cmd+S / Ctrl+S save handler
  const saveKeymap = keymap.of([
    {
      key: "Mod-s",
      run: () => {
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
            updateContent(workspaceIdRef.current, pathRef.current, doc);
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

    return () => {
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

  return <div ref={containerRef} className="h-full w-full overflow-hidden" />;
};

// ---------------------------------------------------------------------------
// Preview renderers
// ---------------------------------------------------------------------------

const MarkdownPreview: FC<{ content: string }> = ({ content }) => (
  <div className="h-full overflow-auto bg-app-canvas/70 px-4 py-3 text-app-foreground">
    <div className="mx-auto max-w-4xl text-sm leading-6">
      <MessageResponse>{content}</MessageResponse>
    </div>
  </div>
);

const HtmlPreview: FC<{ content: string }> = ({ content }) => (
  <iframe
    sandbox="allow-scripts"
    srcDoc={content}
    className="h-full w-full border-0 bg-white"
    title="HTML Preview"
  />
);

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
  activeTabPath: string | null;
}

const TabBar: FC<TabBarProps> = ({ tabs, activeTabPath }) => (
  <div className="flex min-h-0 items-center gap-0 overflow-x-auto border-b border-app-border bg-app-drawer/50">
    {tabs.map((tab) => {
      const fileName = tab.path.split("/").pop() ?? tab.path;
      const isActive = tab.path === activeTabPath;
      return (
        <button
          key={tab.path}
          className={cn(
            "group relative flex shrink-0 items-center gap-1 border-r border-app-border px-2.5 py-1.5 text-[11px] transition-colors",
            isActive
              ? "bg-app-drawer text-foreground"
              : "text-muted-foreground hover:text-foreground hover:bg-app-drawer/80",
          )}
          onClick={() => {
            fileEditorStore.setState({ activeTabPath: tab.path });
          }}
          onDoubleClick={() => pinTab(tab.path)}
          onMouseDown={(e) => {
            // Middle-click to close
            if (e.button === 1) {
              e.preventDefault();
              closeTab(tab.path);
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
              closeTab(tab.path);
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
  workspaceId: string;
}

const EditorToolbar: FC<ToolbarProps> = ({ tab, workspaceId }) => {
  const canPreview = isPreviewable(tab.path);
  return (
    <div className="flex items-center justify-between border-b border-app-border px-2 py-1">
      <div className="flex items-center gap-1">
        {canPreview && (
          <>
            <button
              title="Editor"
              className={cn(
                "rounded p-1 text-xs transition-colors",
                tab.previewMode === "editor"
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => setPreviewMode(tab.path, "editor")}
            >
              <Code2 className="size-3.5" />
            </button>
            <button
              title="Preview"
              className={cn(
                "rounded p-1 text-xs transition-colors",
                tab.previewMode === "preview"
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => setPreviewMode(tab.path, "preview")}
            >
              <Eye className="size-3.5" />
            </button>
          </>
        )}
      </div>
      <div className="flex items-center gap-1">
        {tab.isDirty && (
          <button
            title="Save (⌘S)"
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            onClick={() => void saveFile(workspaceId, tab.path)}
          >
            <Save className="size-3" />
            <span>Save</span>
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
  const tabs = useOpenTabs();
  const activeTab = useActiveTab();

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (activeTab) {
          void saveFile(workspaceId, activeTab.path);
        }
      }
    },
    [workspaceId, activeTab],
  );

  // Listen for agent file-changed events to reload affected tabs
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    void listen<{ path: string }>("agent-file-changed", (event) => {
      if (cancelled) return;
      const { path } = event.payload;
      const tab = fileEditorStore.getState().tabs.find((t) => t.path === path);
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
        activeTabPath={activeTab?.path ?? null}
      />
      {activeTab && (
        <>
          <EditorToolbar tab={activeTab} workspaceId={workspaceId} />
          <div className="min-h-0 flex-1 overflow-hidden">
            {activeTab.isLoading ? (
              <div className="flex h-full items-center justify-center text-muted-foreground">
                <Loader2 className="mr-2 size-4 animate-spin" />
                <span className="text-xs">Loading…</span>
              </div>
            ) : activeTab.error && activeTab.content === null ? (
              <div className="flex h-full items-center justify-center p-4 text-center">
                <div>
                  <AlertCircle className="mx-auto mb-2 size-5 text-destructive" />
                  <p className="text-xs text-muted-foreground">{activeTab.error}</p>
                </div>
              </div>
            ) : isImageFile(activeTab.path) && activeTab.content ? (
              <ImagePreview content={activeTab.content} path={activeTab.path} />
            ) : activeTab.previewMode === "preview" && activeTab.content !== null ? (
              activeTab.path.endsWith(".html") || activeTab.path.endsWith(".htm") ? (
                <HtmlPreview content={activeTab.content} />
              ) : (
                <MarkdownPreview content={activeTab.content} />
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
