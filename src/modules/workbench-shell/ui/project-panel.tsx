import { useDeferredValue, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Check, ChevronDown, ChevronRight, Copy, FolderOpen, LoaderCircle, RefreshCw } from "lucide-react";
import {
  type DirectoryChildrenResponse,
  indexFilterFiles,
  indexGetChildren,
  indexRevealPath,
  indexGetTree,
  type FileFilterMatch,
  type FileFilterResponse,
  type FileTreeNode,
  type FileTreeResponse,
} from "@/services/bridge";
import { Input } from "@/shared/ui/input";
import { cn } from "@/shared/lib/utils";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import {
  DRAWER_LIST_LABEL_CLASS,
  DRAWER_LIST_META_CLASS,
  DRAWER_LIST_ROW_CLASS,
  DRAWER_LIST_STACK_CLASS,
  PROJECT_TREE_ITEMS,
} from "@/modules/workbench-shell/model/fixtures";
import { useWorkspaceOpenApps } from "@/modules/workbench-shell/model/use-workspace-open-apps";
import type { ProjectOption, ProjectTreeItem, WorkspaceOpenApp } from "@/modules/workbench-shell/model/types";
import { ProjectTreeIcon } from "@/modules/workbench-shell/ui/project-tree-icon";

const APP_ICON_FALLBACKS: Record<
  string,
  { src?: string; label: string; className: string }
> = {
  finder: { label: "F", className: "bg-linear-to-br from-sky-400 to-blue-500 text-white" },
  explorer: { label: "E", className: "bg-linear-to-br from-amber-300 to-yellow-500 text-slate-900" },
  terminal: { label: "T", className: "bg-linear-to-br from-slate-700 to-slate-950 text-white" },
  iterm2: { label: "IT", className: "bg-linear-to-br from-slate-600 to-slate-900 text-white" },
  warp: { label: "WP", className: "bg-linear-to-br from-lime-300 to-emerald-500 text-slate-950" },
  powershell: { label: "PS", className: "bg-linear-to-br from-sky-500 to-indigo-700 text-white" },
  "git-bash": { label: "GB", className: "bg-linear-to-br from-emerald-400 to-teal-600 text-white" },
  vscode: { label: "VS", className: "bg-linear-to-br from-sky-500 to-blue-700 text-white" },
  cursor: { src: "/llm-icons/cursor.svg", label: "C", className: "bg-slate-900 text-white" },
  windsurf: { src: "/llm-icons/windsurf.svg", label: "W", className: "bg-cyan-500 text-white" },
  zed: { label: "Z", className: "bg-linear-to-br from-orange-500 to-rose-500 text-white" },
  "intellij-idea": { label: "IJ", className: "bg-linear-to-br from-fuchsia-500 to-slate-950 text-white" },
  pycharm: { label: "PY", className: "bg-linear-to-br from-lime-300 to-emerald-700 text-slate-950" },
  goland: { label: "GO", className: "bg-linear-to-br from-cyan-300 to-blue-700 text-white" },
  "android-studio": { label: "AS", className: "bg-linear-to-br from-emerald-300 to-green-600 text-slate-950" },
};

type TreeState = {
  data: FileTreeResponse | null;
  error: string | null;
  isLoading: boolean;
};

type FilterState = {
  data: FileFilterResponse | null;
  error: string | null;
  isLoading: boolean;
};

type VisibleTreeRow = {
  kind: "node";
  node: FileTreeNode;
  depth: number;
  isExpanded: boolean;
  isLoading: boolean;
} | {
  kind: "load-more";
  parentPath: string;
  depth: number;
  isLoading: boolean;
};

const initialTreeState: TreeState = {
  data: null,
  error: null,
  isLoading: true,
};

const initialFilterState: FilterState = {
  data: null,
  error: null,
  isLoading: false,
};

function WorkspaceAppIcon({
  app,
  sizeClassName,
  radiusClassName,
}: {
  app: WorkspaceOpenApp;
  sizeClassName: string;
  radiusClassName: string;
}) {
  const fallback = APP_ICON_FALLBACKS[app.id] ?? {
    label: app.name.slice(0, 1).toUpperCase(),
    className: "bg-app-surface-muted text-app-foreground",
  };

  if (app.iconDataUrl) {
    return <img src={app.iconDataUrl} alt="" className={cn(sizeClassName, radiusClassName, "shrink-0 object-cover")} />;
  }

  if (fallback.src) {
    return (
      <span className={cn(sizeClassName, radiusClassName, fallback.className, "inline-flex shrink-0 items-center justify-center")}>
        <img src={fallback.src} alt="" className="size-[70%] object-contain" />
      </span>
    );
  }

  return (
    <span
      className={cn(
        sizeClassName,
        radiusClassName,
        fallback.className,
        "inline-flex shrink-0 items-center justify-center text-[9px] font-semibold tracking-[-0.02em]",
      )}
    >
      {fallback.label}
    </span>
  );
}

function buildMockTreeResponse(): FileTreeResponse {
  return {
    repoAvailable: true,
    tree: {
      name: "Project",
      path: "",
      isDir: true,
      isExpandable: true,
      childrenHasMore: false,
      children: PROJECT_TREE_ITEMS.map((item) => ({
        name: item.name,
        path: item.name,
        isDir: item.kind === "folder",
        isExpandable: false,
        childrenHasMore: false,
        gitState: item.ignored ? "ignored" : "tracked",
      })),
    },
  };
}

function buildMockFilterResponse(query: string): FileFilterResponse {
  const normalized = query.trim().toLowerCase();
  const results = PROJECT_TREE_ITEMS
    .filter((item) => item.name.toLowerCase().includes(normalized))
    .map((item) => ({
      name: item.name,
      path: item.name,
      parentPath: "",
    }));

  return {
    query,
    results,
    count: results.length,
  };
}

function inferIcon(name: string, isDir: boolean): ProjectTreeItem["icon"] {
  if (isDir) {
    return "folder";
  }

  const lowerName = name.toLowerCase();
  const extension = lowerName.includes(".") ? lowerName.split(".").pop() ?? "" : "";

  if (lowerName === ".gitignore" || lowerName.startsWith(".git")) {
    return "git";
  }

  if (lowerName.endsWith(".json")) {
    return "json";
  }

  if (lowerName.endsWith(".html")) {
    return "html";
  }

  if (lowerName.endsWith(".css")) {
    return "css";
  }

  if (lowerName === "license") {
    return "license";
  }

  if (lowerName === "readme.md") {
    return "readme";
  }

  if (extension === "ts" || extension === "tsx") {
    return "ts";
  }

  return "file";
}

function flattenVisibleTree(
  nodes: ReadonlyArray<FileTreeNode>,
  expandedPaths: ReadonlySet<string>,
  loadingPaths: ReadonlySet<string>,
  depth = 0,
): Array<VisibleTreeRow> {
  const rows: Array<VisibleTreeRow> = [];

  for (const node of nodes) {
    const isExpanded = expandedPaths.has(node.path);
    const isLoading = loadingPaths.has(node.path);

    rows.push({
      kind: "node",
      node,
      depth,
      isExpanded,
      isLoading,
    });

    if (node.children && node.children.length > 0 && isExpanded) {
      rows.push(...flattenVisibleTree(node.children, expandedPaths, loadingPaths, depth + 1));
    }

    if (isExpanded && node.childrenHasMore) {
      rows.push({
        kind: "load-more",
        parentPath: node.path,
        depth: depth + 1,
        isLoading,
      });
    }
  }

  return rows;
}

function mergeUniqueChildren(
  existingChildren: ReadonlyArray<FileTreeNode>,
  nextChildren: ReadonlyArray<FileTreeNode>,
): FileTreeNode[] {
  const merged = new Map<string, FileTreeNode>();

  for (const child of existingChildren) {
    merged.set(child.path, child);
  }

  for (const child of nextChildren) {
    merged.set(child.path, child);
  }

  return Array.from(merged.values());
}

function replaceNodeChildren(
  node: FileTreeNode,
  targetPath: string,
  response: DirectoryChildrenResponse,
  mode: "replace" | "append",
): FileTreeNode {
  if (node.path === targetPath) {
    const children =
      mode === "append"
        ? mergeUniqueChildren(node.children ?? [], response.children)
        : response.children;

    return {
      ...node,
      isExpandable: children.length > 0 || response.hasMore,
      childrenHasMore: response.hasMore,
      childrenNextOffset: response.nextOffset,
      children,
    };
  }

  if (!node.children) {
    return node;
  }

  return {
    ...node,
    children: node.children.map((child) => replaceNodeChildren(child, targetPath, response, mode)),
  };
}

function findNodeByPath(node: FileTreeNode, targetPath: string): FileTreeNode | null {
  if (node.path === targetPath) {
    return node;
  }

  if (!node.children) {
    return null;
  }

  for (const child of node.children) {
    const match = findNodeByPath(child, targetPath);
    if (match) {
      return match;
    }
  }

  return null;
}

function resolveProjectTargetPath(projectRoot: string, relativePath: string): string {
  const trimmedRoot = projectRoot.replace(/[\\/]+$/, "");
  const normalizedRelativePath = relativePath.replace(/^[/\\]+/, "");

  return normalizedRelativePath.length > 0
    ? `${trimmedRoot}/${normalizedRelativePath}`
    : trimmedRoot;
}

function sortTreeNodes(nodes: ReadonlyArray<FileTreeNode>): FileTreeNode[] {
  return [...nodes].sort((left, right) => {
    if (left.isDir !== right.isDir) {
      return left.isDir ? -1 : 1;
    }

    return left.name.localeCompare(right.name);
  });
}

function mergeRevealedChildren(
  existingChildren: ReadonlyArray<FileTreeNode> | undefined,
  nextChildren: ReadonlyArray<FileTreeNode>,
): FileTreeNode[] {
  const merged = new Map<string, FileTreeNode>();

  for (const child of existingChildren ?? []) {
    merged.set(child.path, child);
  }

  for (const child of nextChildren) {
    merged.set(child.path, child);
  }

  return sortTreeNodes(Array.from(merged.values()));
}

function applyRevealSegment(
  node: FileTreeNode,
  targetPath: string,
  response: DirectoryChildrenResponse,
): FileTreeNode {
  if (node.path === targetPath) {
    const wasFullyLoaded = node.children !== undefined && !node.childrenHasMore;
    const children =
      node.children === undefined
        ? response.children
        : mergeRevealedChildren(node.children, response.children);
    const childrenHasMore = wasFullyLoaded ? false : response.hasMore;

    return {
      ...node,
      isExpandable: children.length > 0 || childrenHasMore,
      childrenHasMore,
      childrenNextOffset: childrenHasMore ? response.nextOffset : undefined,
      children,
    };
  }

  if (!node.children) {
    return node;
  }

  return {
    ...node,
    children: node.children.map((child) => applyRevealSegment(child, targetPath, response)),
  };
}

export function ProjectPanel({
  currentProject,
  workspaceId,
  workspaceBootstrapError = null,
}: {
  currentProject: ProjectOption | null;
  workspaceId: string | null;
  workspaceBootstrapError?: string | null;
}) {
  const [filterValue, setFilterValue] = useState("");
  const [treeState, setTreeState] = useState<TreeState>(initialTreeState);
  const [filterState, setFilterState] = useState<FilterState>(initialFilterState);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(() => new Set());
  const [loadingPaths, setLoadingPaths] = useState<Set<string>>(() => new Set());
  const [pendingRevealPath, setPendingRevealPath] = useState<string | null>(null);
  const [revealedPath, setRevealedPath] = useState<string | null>(null);
  const [activeFilterRevealPath, setActiveFilterRevealPath] = useState<string | null>(null);
  const [isRefreshingTree, setRefreshingTree] = useState(false);
  const [treeReloadVersion, setTreeReloadVersion] = useState(0);
  const [isOpenMenuOpen, setOpenMenuOpen] = useState(false);
  const [preferredOpenAppId, setPreferredOpenAppId] = useState<string | null>(null);
  const [activeOpenTargetId, setActiveOpenTargetId] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const [copiedPath, setCopiedPath] = useState<string | null>(null);
  const deferredFilterValue = useDeferredValue(filterValue);
  const openMenuRef = useRef<HTMLDivElement | null>(null);
  const errorTimeoutRef = useRef<number | null>(null);
  const revealTimeoutRef = useRef<number | null>(null);
  const treeScrollRef = useRef<HTMLDivElement | null>(null);
  const treeRowRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const { data: openApps, error: openAppsError, isLoading: isLoadingOpenApps } = useWorkspaceOpenApps();
  const normalizedFilter = deferredFilterValue.trim().toLowerCase();
  const projectName = currentProject?.name ?? "Project";
  const projectPath = currentProject?.path ?? null;
  const preferredOpenApp = openApps.find((app) => app.id === preferredOpenAppId) ?? openApps[0] ?? null;

  useEffect(() => {
    return () => {
      if (errorTimeoutRef.current) {
        window.clearTimeout(errorTimeoutRef.current);
      }
      if (revealTimeoutRef.current) {
        window.clearTimeout(revealTimeoutRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!copiedPath || typeof window === "undefined") {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      setCopiedPath((current) => (current === copiedPath ? null : current));
    }, 1600);

    return () => window.clearTimeout(timeoutId);
  }, [copiedPath]);

  useEffect(() => {
    if (!isOpenMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && openMenuRef.current?.contains(target)) {
        return;
      }

      setOpenMenuOpen(false);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isOpenMenuOpen]);

  useEffect(() => {
    if (!preferredOpenAppId || !openApps.some((app) => app.id === preferredOpenAppId)) {
      setPreferredOpenAppId(openApps[0]?.id ?? null);
    }
  }, [openApps, preferredOpenAppId]);

  useEffect(() => {
    setExpandedPaths(new Set());
    setLoadingPaths(new Set());
    setPendingRevealPath(null);
    setRevealedPath(null);
    setActiveFilterRevealPath(null);
    setCopiedPath(null);
  }, [workspaceId, projectPath]);

  useEffect(() => {
    if (!pendingRevealPath || normalizedFilter.length > 0) {
      return;
    }

    let frameId = window.requestAnimationFrame(() => {
      const targetRow = treeRowRefs.current.get(pendingRevealPath);
      if (!targetRow) {
        return;
      }

      targetRow.scrollIntoView({
        block: "center",
        inline: "nearest",
      });
      setRevealedPath(pendingRevealPath);
      setPendingRevealPath(null);

      if (revealTimeoutRef.current) {
        window.clearTimeout(revealTimeoutRef.current);
      }

      revealTimeoutRef.current = window.setTimeout(() => {
        setRevealedPath((current) => (current === pendingRevealPath ? null : current));
        revealTimeoutRef.current = null;
      }, 1600);
    });

    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [pendingRevealPath, normalizedFilter.length, treeState.data, expandedPaths]);

  const setTreeRowRef = (path: string, element: HTMLButtonElement | null) => {
    if (element) {
      treeRowRefs.current.set(path, element);
    } else {
      treeRowRefs.current.delete(path);
    }
  };

  useEffect(() => {
    let cancelled = false;

    if (!currentProject) {
      setRefreshingTree(false);
      setTreeState({ data: null, error: null, isLoading: false });
      return () => {
        cancelled = true;
      };
    }

    if (!isTauri()) {
      setRefreshingTree(false);
      setTreeState({
        data: buildMockTreeResponse(),
        error: null,
        isLoading: false,
      });
      return () => {
        cancelled = true;
      };
    }

    if (!workspaceId) {
      setRefreshingTree(false);
      setTreeState((current) => ({
        data: current.data,
        error: null,
        isLoading: true,
      }));
      return () => {
        cancelled = true;
      };
    }

    setTreeState((current) => ({
      data: current.data,
      error: null,
      isLoading: true,
    }));

    void indexGetTree(workspaceId)
      .then((response) => {
        if (cancelled) {
          return;
        }

        setTreeState({
          data: response ?? buildMockTreeResponse(),
          error: null,
          isLoading: false,
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = getInvokeErrorMessage(error, "读取文件树失败");
        setTreeState({
          data: null,
          error: message,
          isLoading: false,
        });
      })
      .finally(() => {
        if (!cancelled) {
          setRefreshingTree(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [projectPath, workspaceId, treeReloadVersion]);

  useEffect(() => {
    let cancelled = false;

    if (normalizedFilter.length === 0) {
      setFilterState(initialFilterState);
      return () => {
        cancelled = true;
      };
    }

    if (!currentProject) {
      setFilterState(initialFilterState);
      return () => {
        cancelled = true;
      };
    }

    if (!isTauri()) {
      setFilterState({
        data: buildMockFilterResponse(normalizedFilter),
        error: null,
        isLoading: false,
      });
      return () => {
        cancelled = true;
      };
    }

    if (!workspaceId) {
      setFilterState((current) => ({
        data: current.data,
        error: null,
        isLoading: true,
      }));
      return () => {
        cancelled = true;
      };
    }

    setFilterState((current) => ({
      data: current.data,
      error: null,
      isLoading: true,
    }));

    void indexFilterFiles(workspaceId, normalizedFilter, 200)
      .then((response) => {
        if (cancelled) {
          return;
        }

        setFilterState({
          data: response,
          error: null,
          isLoading: false,
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        const message = getInvokeErrorMessage(error, "过滤文件失败");
        setFilterState({
          data: null,
          error: message,
          isLoading: false,
        });
      });

    return () => {
      cancelled = true;
    };
  }, [projectPath, normalizedFilter, workspaceId]);

  const handleRefreshTree = () => {
    if (!currentProject || !workspaceId || !isTauri() || treeState.isLoading) {
      return;
    }

    setRefreshingTree(true);
    setExpandedPaths(new Set());
    setLoadingPaths(new Set());
    setPendingRevealPath(null);
    setRevealedPath(null);
    setActiveFilterRevealPath(null);
    setTreeState((current) => ({
      data: current.data,
      error: null,
      isLoading: true,
    }));
    setTreeReloadVersion((current) => current + 1);
  };

  const handleOpenInApp = async (app: WorkspaceOpenApp) => {
    if (!projectPath) {
      return;
    }

    setActiveOpenTargetId(app.id);

    try {
      await invoke("open_workspace_in_app", {
        targetPath: projectPath,
        appId: app.id,
        appPath: app.openWith,
      });
      setPreferredOpenAppId(app.id);
      setOpenMenuOpen(false);
      setOpenError(null);
    } catch (error) {
      const message = getInvokeErrorMessage(error, `Couldn't open in ${app.name}`);
      setOpenError(message);
      if (errorTimeoutRef.current) {
        window.clearTimeout(errorTimeoutRef.current);
      }
      errorTimeoutRef.current = window.setTimeout(() => {
        setOpenError(null);
        errorTimeoutRef.current = null;
      }, 2200);
    } finally {
      setActiveOpenTargetId(null);
    }
  };

  const handleOpenTreeFile = async (relativePath: string) => {
    if (!projectPath || !preferredOpenApp) {
      return;
    }

    const targetPath = resolveProjectTargetPath(projectPath, relativePath);
    setActiveOpenTargetId(preferredOpenApp.id);

    try {
      await invoke("open_tree_path_in_app", {
        targetPath,
        isDirectory: false,
        appId: preferredOpenApp.id,
        appPath: preferredOpenApp.openWith,
      });
      setOpenError(null);
    } catch (error) {
      const message = getInvokeErrorMessage(error, `Couldn't open file in ${preferredOpenApp.name}`);
      setOpenError(message);
      if (errorTimeoutRef.current) {
        window.clearTimeout(errorTimeoutRef.current);
      }
      errorTimeoutRef.current = window.setTimeout(() => {
        setOpenError(null);
        errorTimeoutRef.current = null;
      }, 2200);
    } finally {
      setActiveOpenTargetId(null);
    }
  };

  const loadChildrenIntoTree = async (path: string, sourceTree: FileTreeNode) => {
    if (!workspaceId) {
      return {
        tree: sourceTree,
        response: {
          children: [],
          hasMore: false,
        } satisfies DirectoryChildrenResponse,
      };
    }

    setLoadingPaths((current) => new Set(current).add(path));

    try {
      const response = await indexGetChildren(workspaceId, path, 0, 200);
      return {
        tree: replaceNodeChildren(sourceTree, path, response, "replace"),
        response,
      };
    } finally {
      setLoadingPaths((current) => {
        const next = new Set(current);
        next.delete(path);
        return next;
      });
    }
  };

  const loadMoreChildrenIntoTree = async (path: string, sourceTree: FileTreeNode) => {
    if (!workspaceId) {
      return sourceTree;
    }

    const targetNode = findNodeByPath(sourceTree, path);
    if (!targetNode || !targetNode.childrenHasMore) {
      return sourceTree;
    }

    const offset = targetNode.childrenNextOffset ?? targetNode.children?.length ?? 0;
    setLoadingPaths((current) => new Set(current).add(path));

    try {
      const response = await indexGetChildren(workspaceId, path, offset, 200);
      return replaceNodeChildren(sourceTree, path, response, "append");
    } finally {
      setLoadingPaths((current) => {
        const next = new Set(current);
        next.delete(path);
        return next;
      });
    }
  };

  const handleTreeToggle = async (node: FileTreeNode) => {
    if (!node.isDir || !node.isExpandable) {
      return;
    }

    if (expandedPaths.has(node.path)) {
      setExpandedPaths((current) => {
        const next = new Set(current);
        next.delete(node.path);
        return next;
      });
      return;
    }

    setExpandedPaths((current) => new Set(current).add(node.path));

    if (node.children !== undefined || !treeState.data || loadingPaths.has(node.path)) {
      return;
    }

    try {
      const { tree: nextTree } = await loadChildrenIntoTree(node.path, treeState.data.tree);
      setTreeState((current) => {
        if (!current.data) {
          return current;
        }

        return {
          ...current,
          data: {
            ...current.data,
            tree: nextTree,
          },
        };
      });
    } catch (error) {
      const message = getInvokeErrorMessage(error, "读取目录内容失败");
      setTreeState((current) => ({
        ...current,
        error: message,
      }));
    }
  };

  const handleLoadMore = async (parentPath: string) => {
    if (!treeState.data || loadingPaths.has(parentPath)) {
      return;
    }

    try {
      const nextTree = await loadMoreChildrenIntoTree(parentPath, treeState.data.tree);
      setTreeState((current) => {
        if (!current.data) {
          return current;
        }

        return {
          ...current,
          data: {
            ...current.data,
            tree: nextTree,
          },
        };
      });
    } catch (error) {
      const message = getInvokeErrorMessage(error, "继续加载目录内容失败");
      setTreeState((current) => ({
        ...current,
        error: message,
      }));
    }
  };

  const handleRevealFilterResult = async (match: FileFilterMatch) => {
    if (!treeState.data || activeFilterRevealPath) {
      return;
    }

    setActiveFilterRevealPath(match.path);

    if (!workspaceId) {
      setFilterValue("");
      setActiveFilterRevealPath(null);
      return;
    }

    try {
      const response = await indexRevealPath(workspaceId, match.path);
      const nextExpanded = new Set(expandedPaths);
      let nextTree = treeState.data.tree;

      for (const segment of response.segments) {
        nextTree = applyRevealSegment(nextTree, segment.directoryPath, {
          children: segment.children,
          hasMore: segment.hasMore,
          nextOffset: segment.nextOffset,
        });

        if (segment.directoryPath) {
          nextExpanded.add(segment.directoryPath);
        }
      }

      setTreeState((current) => {
        if (!current.data) {
          return current;
        }

        return {
          ...current,
          data: {
            ...current.data,
            tree: nextTree,
          },
        };
      });
      setExpandedPaths(nextExpanded);
      setPendingRevealPath(response.targetPath);
      setFilterValue("");
    } catch (error) {
      const message = getInvokeErrorMessage(error, "无法展开筛选结果路径");
      setFilterState((current) => ({
        ...current,
        error: message,
      }));
    } finally {
      setActiveFilterRevealPath(null);
    }
  };

  const handleCopyRelativePath = async (path: string) => {
    if (typeof window === "undefined") {
      setCopiedPath(null);
      setOpenError("Failed to copy relative path");
      return;
    }

    try {
      if (navigator?.clipboard?.writeText) {
        await navigator.clipboard.writeText(path);
      } else {
        const textArea = document.createElement("textarea");
        textArea.value = path;
        textArea.setAttribute("readonly", "true");
        textArea.style.position = "fixed";
        textArea.style.opacity = "0";
        textArea.style.pointerEvents = "none";
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        const didCopy = document.execCommand("copy");
        document.body.removeChild(textArea);

        if (!didCopy) {
          throw new Error("copy command failed");
        }
      }

      setCopiedPath(path);
      setOpenError(null);
    } catch {
      setCopiedPath(null);
      setOpenError("Failed to copy relative path");
    }
  };

  const visibleRows = flattenVisibleTree(
    treeState.data?.tree.children ?? [],
    expandedPaths,
    loadingPaths,
  );
  const filterResults = filterState.data?.results ?? [];
  const isFiltering = normalizedFilter.length > 0;

  return (
    <div className="flex h-full min-h-0 flex-col px-4 pb-5 pt-2">
      <div className="shrink-0 bg-app-drawer">
        <div className="flex items-center justify-between gap-3 px-1 pr-1 text-[15px] font-medium">
          <div className="flex min-w-0 items-center gap-3">
            <FolderOpen className="size-4 shrink-0 text-app-subtle" />
            <span className="truncate text-app-foreground">{projectName}</span>
          </div>
          {isLoadingOpenApps || preferredOpenApp ? (
            <div ref={openMenuRef} className="relative shrink-0">
              <div
                className={cn(
                  "inline-flex h-8 items-stretch overflow-hidden rounded-2xl border border-app-border bg-app-surface/90 text-app-subtle transition-[border-color,background-color,box-shadow]",
                  isOpenMenuOpen && "border-app-border-strong bg-app-surface text-app-foreground shadow-[0_8px_18px_rgba(15,23,42,0.08)]",
                )}
              >
                <button
                  type="button"
                  aria-label={preferredOpenApp ? `Open folder with ${preferredOpenApp.name}` : "Loading supported apps"}
                  title={preferredOpenApp ? `Open folder with ${preferredOpenApp.name}` : "Loading supported apps"}
                  disabled={!projectPath || isLoadingOpenApps || openApps.length === 0 || !preferredOpenApp}
                  className="inline-flex min-w-0 items-center px-2.5 transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:opacity-60"
                  onClick={() => {
                    if (preferredOpenApp) {
                      void handleOpenInApp(preferredOpenApp);
                    }
                  }}
                >
                  {isLoadingOpenApps ? (
                    <LoaderCircle className="size-4 shrink-0 animate-spin text-app-subtle" />
                  ) : preferredOpenApp ? (
                    <WorkspaceAppIcon app={preferredOpenApp} sizeClassName="size-[18px]" radiusClassName="rounded-[5px]" />
                  ) : null}
                </button>

                <div className="w-px bg-app-border/80" />

                <button
                  type="button"
                  aria-label="Choose app to open folder"
                  title="Choose app to open folder"
                  aria-haspopup="menu"
                  aria-expanded={isOpenMenuOpen}
                  disabled={!projectPath || isLoadingOpenApps || openApps.length === 0}
                  className="inline-flex w-7 items-center justify-center transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:opacity-60"
                  onClick={() => setOpenMenuOpen((current) => !current)}
                >
                  <ChevronDown
                    className={cn(
                      "size-3.5 shrink-0 transition-transform duration-200",
                      isOpenMenuOpen && "rotate-180",
                    )}
                  />
                </button>
              </div>

              {isOpenMenuOpen ? (
                <div className="absolute right-0 top-[calc(100%+0.45rem)] z-20 min-w-[220px] overflow-hidden rounded-2xl border border-app-border bg-app-menu/98 p-1.5 shadow-[0_18px_40px_-26px_rgba(15,23,42,0.38)] backdrop-blur-xl dark:bg-app-menu/94">
                  <div className="px-2.5 pb-1.5 pt-1">
                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-app-subtle">Open in</div>
                  </div>
                  <div className="space-y-0.5">
                    {openApps.map((app) => {
                      const isPending = activeOpenTargetId === app.id;
                      const isPreferred = preferredOpenApp?.id === app.id;

                      return (
                        <button
                          key={app.id}
                          type="button"
                          className={cn(
                            "flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-left transition-colors disabled:cursor-wait disabled:opacity-70",
                            isPreferred
                              ? "bg-app-surface-hover/80 text-app-foreground"
                              : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                          )}
                          disabled={Boolean(activeOpenTargetId)}
                          onClick={() => void handleOpenInApp(app)}
                        >
                          {isPending ? (
                            <LoaderCircle className="size-4 shrink-0 animate-spin text-app-subtle" />
                          ) : (
                            <WorkspaceAppIcon app={app} sizeClassName="size-5" radiusClassName="rounded-[7px]" />
                          )}
                          <span className="min-w-0 flex-1 truncate text-[12px] font-medium">{app.name}</span>
                          {isPreferred ? <Check className="size-3.5 shrink-0 text-app-foreground" /> : null}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        <div className="relative mt-2.5 pl-5 pr-1 pb-2.5">
          <div className="absolute bottom-0 left-[6px] top-0 w-px bg-app-border" />
          <div className="relative">
            <Input
              value={filterValue}
              onChange={(event) => setFilterValue(event.target.value)}
              placeholder="Filter files"
              aria-label="Filter files"
              className="h-8 rounded-lg border-app-border bg-app-surface-muted px-2.5 pr-10 text-[13px] text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0"
            />
            <button
              type="button"
              aria-label="Refresh tree view"
              title="Refresh tree view"
              disabled={!currentProject || !workspaceId || treeState.isLoading}
              className="absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground disabled:cursor-not-allowed disabled:opacity-50"
              onClick={handleRefreshTree}
            >
              <RefreshCw className={cn("size-3.5", isRefreshingTree && "animate-spin")} />
            </button>
          </div>
          {openAppsError ? <p className="mt-2 text-[11px] text-app-danger">{openAppsError}</p> : null}
          {openError ? <p className="mt-2 text-[11px] text-app-danger">{openError}</p> : null}
          {workspaceBootstrapError ? (
            <p className="mt-2 text-[11px] text-app-danger">{workspaceBootstrapError}</p>
          ) : null}
          {!openAppsError && !openError && treeState.data && !treeState.data.repoAvailable ? (
            <p className="mt-2 text-[11px] text-app-subtle">Git overlay unavailable for this workspace</p>
          ) : null}
          {!openAppsError && !openError && !isFiltering && treeState.isLoading && workspaceId ? (
            <p className="mt-2 flex items-center gap-1.5 text-[11px] text-app-subtle">
              <LoaderCircle className="size-3 animate-spin" />
              <span>Loading tree…</span>
            </p>
          ) : null}
          {!openAppsError && !openError && isFiltering && filterState.isLoading ? (
            <p className="mt-2 flex items-center gap-1.5 text-[11px] text-app-subtle">
              <LoaderCircle className="size-3 animate-spin" />
              <span>Searching all files…</span>
            </p>
          ) : null}
          {!openAppsError && !openError && !workspaceBootstrapError && !workspaceId && currentProject ? (
            <p className="mt-2 flex items-center gap-1.5 text-[11px] text-app-subtle">
              <LoaderCircle className="size-3 animate-spin" />
              <span>Preparing workspace…</span>
            </p>
          ) : null}
          {treeState.error ? <p className="mt-2 text-[11px] text-app-danger">{treeState.error}</p> : null}
          {filterState.error ? <p className="mt-2 text-[11px] text-app-danger">{filterState.error}</p> : null}
        </div>
      </div>

      <div
        ref={treeScrollRef}
        className="min-h-0 flex-1 overflow-auto overscroll-none pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        <div className="relative pl-5">
          <div className="absolute bottom-0 left-[6px] top-0 w-px bg-app-border" />
          <div className={DRAWER_LIST_STACK_CLASS}>
            {isFiltering
              ? filterResults.map((match) => {
                  const icon = inferIcon(match.name, false);
                  const isRevealing = activeFilterRevealPath === match.path;

                  return (
                    <button
                      key={match.path}
                      type="button"
                      className={cn(
                        `${DRAWER_LIST_ROW_CLASS} relative flex items-center gap-2 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground`,
                        isRevealing && "bg-app-surface-hover text-app-foreground",
                        activeFilterRevealPath && !isRevealing && "opacity-60",
                      )}
                      disabled={Boolean(activeFilterRevealPath)}
                      onClick={() => void handleRevealFilterResult(match)}
                    >
                      <span className="flex size-4 shrink-0 items-center justify-center text-app-subtle/80">
                        {isRevealing ? <LoaderCircle className="size-3 animate-spin" /> : null}
                      </span>
                      <ProjectTreeIcon icon={icon} muted={false} />
                      <span className={DRAWER_LIST_LABEL_CLASS}>{match.name}</span>
                      {match.parentPath ? (
                        <span className={cn(DRAWER_LIST_META_CLASS, "max-w-[48%] truncate")}>
                          {match.parentPath}
                        </span>
                      ) : null}
                    </button>
                  );
                })
              : visibleRows.map((row) => {
                  if (row.kind === "load-more") {
                    return (
                      <button
                        key={`load-more:${row.parentPath}`}
                        type="button"
                        className="flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                        style={{ paddingLeft: `${10 + row.depth * 14}px` }}
                        onClick={() => void handleLoadMore(row.parentPath)}
                      >
                        <span className="flex size-4 shrink-0 items-center justify-center text-app-subtle/80">
                          {row.isLoading ? <LoaderCircle className="size-3 animate-spin" /> : <ChevronRight className="size-3.5" />}
                        </span>
                        <span className={DRAWER_LIST_META_CLASS}>Load more…</span>
                      </button>
                    );
                  }

                  const { node, depth, isExpanded, isLoading } = row;
                  const isIgnored = node.gitState === "ignored";
                  const isModified = node.gitState === "modified";
                  const isUntracked = node.gitState === "untracked";
                  const badgeLabel = isUntracked ? "U" : isModified ? "M" : null;
                  const icon = inferIcon(node.name, node.isDir);
                  const isCopied = copiedPath === node.path;

                  return (
                    <div
                      key={node.path || node.name}
                      className={cn(
                        `${DRAWER_LIST_ROW_CLASS} group relative flex items-center gap-2`,
                        revealedPath === node.path && "bg-app-surface-hover/90 ring-1 ring-app-border-strong",
                        isIgnored
                          ? "text-app-subtle/70 hover:bg-app-surface-hover/60 hover:text-app-muted"
                          : isUntracked || isModified
                            ? "text-app-foreground hover:bg-app-surface-hover hover:text-app-foreground"
                            : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                      )}
                      style={{ paddingLeft: `${10 + depth * 14}px` }}
                    >
                      <button
                        ref={(element) => setTreeRowRef(node.path, element)}
                        data-tree-path={node.path}
                        type="button"
                        className="min-w-0 flex flex-1 items-center gap-2 text-left"
                        onClick={() => void handleTreeToggle(node)}
                        onDoubleClick={() => {
                          if (!node.isDir) {
                            void handleOpenTreeFile(node.path);
                          }
                        }}
                      >
                        <span className="flex size-4 shrink-0 items-center justify-center text-app-subtle/80">
                          {isLoading ? (
                            <LoaderCircle className="size-3 animate-spin" />
                          ) : node.isDir && node.isExpandable ? (
                            isExpanded ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />
                          ) : null}
                        </span>
                        <ProjectTreeIcon icon={icon} muted={isIgnored} />
                        <span className={cn(DRAWER_LIST_LABEL_CLASS, "min-w-0 flex-1")}>{node.name}</span>
                        {badgeLabel ? (
                          <span className="shrink-0 rounded-full bg-app-warning/12 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-app-warning">
                            {badgeLabel}
                          </span>
                        ) : null}
                      </button>

                      <button
                        type="button"
                        aria-label={`${isCopied ? "Copied relative path for" : "Copy relative path for"} ${node.name}`}
                        title={isCopied ? "Copied" : "Copy relative path"}
                        className={cn(
                          "ml-auto inline-flex size-7 shrink-0 items-center justify-center rounded-md opacity-100 transition-colors hover:bg-app-surface-hover sm:opacity-0 sm:group-hover:opacity-100 sm:focus-visible:opacity-100",
                          isCopied ? "text-app-success" : "text-app-subtle hover:text-app-foreground",
                        )}
                        onClick={(event) => {
                          event.stopPropagation();
                          void handleCopyRelativePath(node.path);
                        }}
                      >
                        {isCopied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                      </button>
                    </div>
                  );
                })}

            {isFiltering && !filterState.isLoading && !filterState.error && filterResults.length === 0 ? (
              <div className="px-2.5 py-2 text-[13px] text-app-subtle">No matching files</div>
            ) : null}

            {!isFiltering && !treeState.isLoading && !treeState.error && visibleRows.length === 0 ? (
              <div className="px-2.5 py-2 text-[13px] text-app-subtle">No files to display</div>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
