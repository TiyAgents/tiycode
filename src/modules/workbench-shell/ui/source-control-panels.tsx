import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useT, type TranslationKey } from "@/i18n";
import { isTauri } from "@tauri-apps/api/core";
import {
  AlertCircle,
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  ChevronDown,
  CircleX,
  Download,
  FileSearch,
  GitBranch,
  LoaderCircle,
  Plus,
  RefreshCw,
  Sparkles,
  Undo2,
} from "lucide-react";
import {
  gitCommit,
  gitGenerateCommitMessage,
  gitGetDiff,
  gitGetFileStatus,
  gitGetHistory,
  gitFetch,
  gitPull,
  gitPush,
  gitGetSnapshot,
  gitRefresh,
  gitStage,
  gitSubscribe,
  gitUnstage,
} from "@/services/bridge";
import type {
  GitChangeKind,
  GitCommitSummaryDto,
  GitDiffDto,
  GitFileChangeDto,
  GitFileStatusDto,
  GitMutationAction,
  GitMutationResponseDto,
  GitSnapshotDto,
  GitStreamEvent,
  RunModelPlanDto,
} from "@/shared/types/api";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/shared/ui/tooltip";
import { cn } from "@/shared/lib/utils";
import {
  DRAWER_ICON_ACTION_CLASS,
  DRAWER_LIST_LABEL_CLASS,
  DRAWER_LIST_META_CLASS,
  DRAWER_LIST_ROW_CLASS,
  DRAWER_LIST_STACK_CLASS,
  DRAWER_SECTION_HEADER_CLASS,
  GIT_CHANGE_FILES,
  GIT_HISTORY_ITEMS,
} from "@/modules/workbench-shell/model/fixtures";
import { buildGitDiffPreview, buildGitSplitDiffRows } from "@/modules/workbench-shell/model/helpers";
import type {
  GitChangeFile,
  GitSplitDiffRow,
  ProjectOption,
  ProjectTreeItem,
} from "@/modules/workbench-shell/model/types";
import { ProjectTreeIcon } from "@/modules/workbench-shell/ui/project-tree-icon";

type TFunc = (key: TranslationKey, params?: Record<string, string | number>) => string;

type GitPanelProps = {
  workspaceId: string | null;
  currentProject: ProjectOption | null;
  workspaceBootstrapError: string | null;
  layoutResizeSignal: number;
  commitMessageLanguage: string;
  commitMessagePrompt: string;
  commitMessageModelPlan: RunModelPlanDto | null;
  onOpenDiffPreview: (selection: GitDiffSelection) => void;
};

export type GitDiffSelection = GitFileChangeDto & {
  staged: boolean;
  icon: ProjectTreeItem["icon"];
};

type GitDiffPreviewPanelProps = {
  workspaceId: string | null;
  selection: GitDiffSelection;
  onClose: () => void;
};

const ACTION_ALERT_TIMEOUT_MS = 4200;

const DEFAULT_HISTORY_HEIGHT = 228;
const MIN_CHANGES_BODY_HEIGHT = 160;
const CHANGES_SECTION_VERTICAL_GAP = 24;

function formatUiError(error: unknown, fallback = "Request failed") {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }

  if (typeof error === "object" && error !== null) {
    const record = error as Record<string, unknown>;
    const candidates = [
      record.userMessage,
      record.user_message,
      record.detail,
      record.error,
      record.message,
    ];

    for (const candidate of candidates) {
      if (typeof candidate === "string" && candidate.trim().length > 0) {
        return candidate;
      }
    }
  }

  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }

  return fallback;
}

function mapMockStatus(status: GitChangeFile["status"]): GitChangeKind {
  switch (status) {
    case "A":
      return "added";
    case "D":
      return "deleted";
    default:
      return "modified";
  }
}

function parseSummary(summary: string) {
  const additions = Number(summary.match(/\+(\d+)/)?.[1] ?? 0);
  const deletions = Number(summary.match(/-(\d+)/)?.[1] ?? 0);
  return { additions, deletions };
}

function inferIconFromPath(path: string): ProjectTreeItem["icon"] {
  const lowerName = path.toLowerCase();
  const extension = lowerName.includes(".") ? lowerName.split(".").pop() ?? "" : "";

  if (lowerName === ".gitignore" || lowerName.startsWith(".git")) {
    return "git";
  }

  if (extension === "json") {
    return "json";
  }

  if (extension === "html") {
    return "html";
  }

  if (extension === "css") {
    return "css";
  }

  if (lowerName.endsWith("license")) {
    return "license";
  }

  if (lowerName.endsWith("readme.md")) {
    return "readme";
  }

  if (extension === "ts" || extension === "tsx") {
    return "ts";
  }

  return "file";
}

function buildMockSnapshot(): GitSnapshotDto {
  const stagedFiles = GIT_CHANGE_FILES.filter((file) => file.initialStaged).map((file) => {
    const { additions, deletions } = parseSummary(file.summary);
    return {
      path: file.path,
      previousPath: null,
      status: mapMockStatus(file.status),
      additions,
      deletions,
    };
  });

  const unstagedFiles = GIT_CHANGE_FILES.filter(
    (file) => !file.initialStaged && file.status !== "A",
  ).map((file) => {
    const { additions, deletions } = parseSummary(file.summary);
    return {
      path: file.path,
      previousPath: null,
      status: mapMockStatus(file.status),
      additions,
      deletions,
    };
  });

  const untrackedFiles = GIT_CHANGE_FILES.filter(
    (file) => !file.initialStaged && file.status === "A",
  ).map((file) => {
    const { additions, deletions } = parseSummary(file.summary);
    return {
      path: file.path,
      previousPath: null,
      status: "added" as const,
      additions,
      deletions,
    };
  });

  return {
    workspaceId: "mock-workspace",
    repoRoot: "/mock/tiycode",
    capabilities: {
      repoAvailable: true,
      gitCliAvailable: true,
    },
    headRef: "main",
    headOid: "6a9f8d2",
    isDetached: false,
    aheadCount: 1,
    behindCount: 0,
    stagedFiles,
    unstagedFiles,
    untrackedFiles,
    recentCommits: GIT_HISTORY_ITEMS.map((item) => ({
      id: item.id,
      shortId: item.hash,
      summary: item.subject,
      authorName: item.author,
      committedAt: new Date().toISOString(),
      refs: [...(item.refs ?? [])],
      isHead: item.refs?.includes("HEAD") ?? false,
    })),
    lastRefreshedAt: new Date().toISOString(),
  };
}

function applyMockStageMutation(
  snapshot: GitSnapshotDto,
  paths: string[],
  action: "stage" | "unstage",
): GitSnapshotDto {
  const selected = new Set(paths);
  const nextStaged = [...snapshot.stagedFiles];
  const nextTracked = [...snapshot.unstagedFiles];
  const nextUntracked = [...snapshot.untrackedFiles];

  if (action === "stage") {
    const movedTracked = nextTracked.filter((file) => selected.has(file.path));
    const movedUntracked = nextUntracked.filter((file) => selected.has(file.path));

    return {
      ...snapshot,
      stagedFiles: [...nextStaged, ...movedTracked, ...movedUntracked].sort((left, right) =>
        left.path.localeCompare(right.path),
      ),
      unstagedFiles: nextTracked.filter((file) => !selected.has(file.path)),
      untrackedFiles: nextUntracked.filter((file) => !selected.has(file.path)),
      lastRefreshedAt: new Date().toISOString(),
    };
  }

  const moved = nextStaged.filter((file) => selected.has(file.path));

  return {
    ...snapshot,
    stagedFiles: nextStaged.filter((file) => !selected.has(file.path)),
    unstagedFiles: [
      ...nextTracked,
      ...moved.filter((file) => file.status !== "added"),
    ].sort((left, right) => left.path.localeCompare(right.path)),
    untrackedFiles: [
      ...nextUntracked,
      ...moved.filter((file) => file.status === "added"),
    ].sort((left, right) => left.path.localeCompare(right.path)),
    lastRefreshedAt: new Date().toISOString(),
  };
}

function makeMockCommitId() {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID().replace(/-/g, "");
  }

  return `${Date.now().toString(16)}${Math.random().toString(16).slice(2, 10)}`;
}

function applyMockCommitMutation(
  snapshot: GitSnapshotDto,
  message: string,
): GitSnapshotDto {
  const trimmed = message.trim();
  if (trimmed.length === 0) {
    throw new Error("Commit message cannot be empty");
  }

  if (snapshot.stagedFiles.length === 0) {
    throw new Error("There are no staged changes to commit");
  }

  const nextCommitId = makeMockCommitId();
  const committedAt = new Date().toISOString();

  return {
    ...snapshot,
    stagedFiles: [],
    recentCommits: [
      {
        id: nextCommitId,
        shortId: nextCommitId.slice(0, 7),
        summary: trimmed,
        authorName: "TiyCode",
        committedAt,
        refs: snapshot.headRef ? [snapshot.headRef, "HEAD"] : ["HEAD"],
        isHead: true,
      },
      ...snapshot.recentCommits.map((commit) => ({
        ...commit,
        isHead: false,
        refs: commit.refs.filter((ref) => ref !== "HEAD"),
      })),
    ],
    aheadCount: snapshot.aheadCount + 1,
    lastRefreshedAt: committedAt,
  };
}

function applyMockRemoteMutation(
  snapshot: GitSnapshotDto,
  action: Exclude<GitMutationAction, "commit">,
): GitSnapshotDto {
  const refreshedAt = new Date().toISOString();

  if (action === "pull") {
    return {
      ...snapshot,
      behindCount: 0,
      lastRefreshedAt: refreshedAt,
    };
  }

  if (action === "push") {
    return {
      ...snapshot,
      aheadCount: 0,
      lastRefreshedAt: refreshedAt,
    };
  }

  return {
    ...snapshot,
    lastRefreshedAt: refreshedAt,
  };
}

function statusCode(status: GitChangeKind) {
  switch (status) {
    case "added":
      return "A";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
    case "typechange":
      return "T";
    case "unmerged":
      return "U";
    default:
      return "M";
  }
}

function statusLabel(status: GitChangeKind, t: TFunc) {
  switch (status) {
    case "added":
      return t("sourceControl.statusAdded");
    case "deleted":
      return t("sourceControl.statusDeleted");
    case "renamed":
      return t("sourceControl.statusRenamed");
    case "typechange":
      return t("sourceControl.statusTypeChanged");
    case "unmerged":
      return t("sourceControl.statusConflict");
    default:
      return t("sourceControl.statusModified");
  }
}

function gitActionLabel(action: GitMutationAction, t: TFunc) {
  switch (action) {
    case "commit":
      return t("sourceControl.actionCommit");
    case "fetch":
      return t("sourceControl.actionFetch");
    case "pull":
      return t("sourceControl.actionPull");
    case "push":
      return t("sourceControl.actionPush");
  }
}

function formatChangeSummary(change: GitFileChangeDto, t: TFunc) {
  const pieces = [];

  if (change.additions > 0) {
    pieces.push(`+${change.additions}`);
  }

  if (change.deletions > 0) {
    pieces.push(`-${change.deletions}`);
  }

  return pieces.join(" ") || statusLabel(change.status, t);
}

function renderChangeStats(change: GitFileChangeDto, t: TFunc) {
  const hasLineStats = change.additions > 0 || change.deletions > 0;

  if (!hasLineStats) {
    return <span className={DRAWER_LIST_META_CLASS}>{statusLabel(change.status, t)}</span>;
  }

  return (
    <span className={cn(DRAWER_LIST_META_CLASS, "flex items-center gap-2")}>
      {change.deletions > 0 ? (
        <span className="font-medium text-app-danger">-{change.deletions}</span>
      ) : null}
      {change.additions > 0 ? (
        <span className="font-medium text-app-success">+{change.additions}</span>
      ) : null}
    </span>
  );
}

function formatRelativeTime(value: string) {
  const timestamp = new Date(value).getTime();
  if (Number.isNaN(timestamp)) {
    return "just now";
  }

  const deltaSeconds = Math.round((timestamp - Date.now()) / 1000);
  const absoluteSeconds = Math.abs(deltaSeconds);
  const formatter = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

  if (absoluteSeconds < 60) {
    return formatter.format(deltaSeconds, "second");
  }

  if (absoluteSeconds < 3600) {
    return formatter.format(Math.round(deltaSeconds / 60), "minute");
  }

  if (absoluteSeconds < 86400) {
    return formatter.format(Math.round(deltaSeconds / 3600), "hour");
  }

  if (absoluteSeconds < 604800) {
    return formatter.format(Math.round(deltaSeconds / 86400), "day");
  }

  return formatter.format(Math.round(deltaSeconds / 604800), "week");
}

function toPreviewSelection(change: GitFileChangeDto, staged: boolean): GitDiffSelection {
  return {
    ...change,
    staged,
    icon: inferIconFromPath(change.path),
  };
}

function buildSplitRowsFromDiff(diff: GitDiffDto): ReadonlyArray<GitSplitDiffRow> {
  const rows: GitSplitDiffRow[] = [];
  let removeBuffer: Array<GitDiffDto["hunks"][number]["lines"][number]> = [];
  let addBuffer: Array<GitDiffDto["hunks"][number]["lines"][number]> = [];

  const flushBuffers = () => {
    const pairCount = Math.max(removeBuffer.length, addBuffer.length);

    for (let index = 0; index < pairCount; index += 1) {
      const removed = removeBuffer[index];
      const added = addBuffer[index];

      if (removed && added) {
        rows.push({
          kind: "modified",
          leftNumber: removed.oldNumber,
          rightNumber: added.newNumber,
          leftText: removed.text,
          rightText: added.text,
        });
        continue;
      }

      if (removed) {
        rows.push({
          kind: "remove",
          leftNumber: removed.oldNumber,
          rightNumber: null,
          leftText: removed.text,
          rightText: "",
        });
      }

      if (added) {
        rows.push({
          kind: "add",
          leftNumber: null,
          rightNumber: added.newNumber,
          leftText: "",
          rightText: added.text,
        });
      }
    }

    removeBuffer = [];
    addBuffer = [];
  };

  for (const hunk of diff.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === "remove") {
        removeBuffer.push(line);
        continue;
      }

      if (line.kind === "add") {
        addBuffer.push(line);
        continue;
      }

      if (removeBuffer.length > 0 || addBuffer.length > 0) {
        flushBuffers();
      }

      rows.push({
        kind: "context",
        leftNumber: line.oldNumber,
        rightNumber: line.newNumber,
        leftText: line.text,
        rightText: line.text,
      });
    }

    if (removeBuffer.length > 0 || addBuffer.length > 0) {
      flushBuffers();
    }
  }

  return rows;
}

function buildMockPreviewSelection(selection: GitDiffSelection, t: TFunc): GitChangeFile {
  const existing = GIT_CHANGE_FILES.find((file) => file.path === selection.path);
  if (existing) {
    return existing;
  }

  return {
    id: selection.path,
    path: selection.path,
    status: statusCode(selection.status) as GitChangeFile["status"],
    icon: selection.icon,
    summary: formatChangeSummary(selection, t),
    initialStaged: selection.staged,
  };
}

function EmptyState({
  title,
  body,
  error,
}: {
  title: string;
  body: string;
  error?: boolean;
}) {
  return (
    <div className="flex h-full min-h-0 items-center justify-center px-6 pb-8 pt-6 text-center">
      <div className="max-w-[240px]">
        <div
          className={cn(
            "mx-auto flex size-11 items-center justify-center rounded-2xl border",
            error
              ? "border-app-danger/20 bg-app-danger/10 text-app-danger"
              : "border-app-border bg-app-surface-muted text-app-subtle",
          )}
        >
          {error ? <AlertCircle className="size-5" /> : <GitBranch className="size-5" />}
        </div>
        <p className="mt-4 text-sm font-semibold text-app-foreground">{title}</p>
        <p className="mt-2 text-[12px] leading-5 text-app-subtle">{body}</p>
      </div>
    </div>
  );
}

function ChangeGroup({
  title,
  files,
  staged,
  pendingPaths,
  t,
  onOpenDiffPreview,
  onToggleStage,
  onToggleAll,
}: {
  title: string;
  files: GitFileChangeDto[];
  staged: boolean;
  pendingPaths: ReadonlySet<string>;
  t: TFunc;
  onOpenDiffPreview: (selection: GitDiffSelection) => void;
  onToggleStage: (paths: string[], staged: boolean) => void;
  onToggleAll: (paths: string[], staged: boolean) => void;
}) {
  if (files.length === 0) {
    return null;
  }

  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between px-2.5 pt-2">
        <p className="text-[11px] font-semibold uppercase tracking-[0.14em] text-app-subtle">
          {title}
        </p>
        <div className="flex items-center gap-1">
          <span className="text-[11px] text-app-subtle">{files.length}</span>
          <button
            type="button"
            aria-label={staged ? `Unstage all ${title}` : `Stage all ${title}`}
            title={staged ? t("sourceControl.unstageAll") : t("sourceControl.stageAll")}
            className={DRAWER_ICON_ACTION_CLASS}
            onClick={() => onToggleAll(files.map((file) => file.path), staged)}
          >
            {staged ? <Undo2 className="size-4" /> : <Plus className="size-4" />}
          </button>
        </div>
      </div>

      <div className={DRAWER_LIST_STACK_CLASS}>
        {files.map((file) => (
          <div
            key={`${staged ? "staged" : "working"}:${file.path}`}
            role="button"
            tabIndex={0}
            title={file.path}
            className={cn(
              "flex items-center gap-2 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-app-border-strong",
              DRAWER_LIST_ROW_CLASS,
            )}
            onClick={() => onOpenDiffPreview(toPreviewSelection(file, staged))}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onOpenDiffPreview(toPreviewSelection(file, staged));
              }
            }}
          >
            <span
              className={cn(
                "inline-flex min-w-5 shrink-0 items-center justify-center rounded px-1 text-[10px] font-semibold",
                file.status === "added"
                  ? "text-app-success"
                  : file.status === "deleted"
                    ? "text-app-danger"
                    : "text-app-subtle",
              )}
            >
              {statusCode(file.status)}
            </span>
            <span className="shrink-0">
              <ProjectTreeIcon icon={inferIconFromPath(file.path)} />
            </span>
            <span className={DRAWER_LIST_LABEL_CLASS}>
              {file.path.split("/").pop() ?? file.path}
            </span>
            {renderChangeStats(file, t)}
            <button
              type="button"
              aria-label={staged ? `Unstage ${file.path}` : `Stage ${file.path}`}
              title={staged ? t("sourceControl.unstage") : t("sourceControl.stage")}
              disabled={pendingPaths.has(file.path)}
              className={cn(
                "flex size-6 shrink-0 items-center justify-center rounded-md border border-app-border text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground disabled:cursor-wait disabled:opacity-60",
                staged && "bg-app-surface-muted text-app-foreground",
              )}
              onClick={(event) => {
                event.stopPropagation();
                onToggleStage([file.path], staged);
              }}
            >
              {pendingPaths.has(file.path) ? (
                <LoaderCircle className="size-3.5 animate-spin" />
              ) : staged ? (
                <Undo2 className="size-3.5" />
              ) : (
                <Plus className="size-3.5" />
              )}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

export function GitPanel({
  workspaceId,
  currentProject,
  workspaceBootstrapError,
  layoutResizeSignal,
  commitMessageLanguage,
  commitMessagePrompt,
  commitMessageModelPlan,
  onOpenDiffPreview,
}: GitPanelProps) {
  const t = useT();
  const isMockMode = !isTauri();
  const panelRef = useRef<HTMLDivElement | null>(null);
  const topContentRef = useRef<HTMLDivElement | null>(null);
  const changesHeaderRef = useRef<HTMLDivElement | null>(null);
  const historyResizeHandleRef = useRef<HTMLDivElement | null>(null);
  const copyResetTimeoutRef = useRef<number>(0);
  const actionAlertTimeoutRef = useRef<number>(0);
  const [snapshot, setSnapshot] = useState<GitSnapshotDto | null>(() =>
    isMockMode ? buildMockSnapshot() : null,
  );
  const [history, setHistory] = useState<GitCommitSummaryDto[]>(() =>
    isMockMode ? buildMockSnapshot().recentCommits : [],
  );
  const [isLoading, setIsLoading] = useState<boolean>(!isMockMode);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [pendingPaths, setPendingPaths] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [actionAlert, setActionAlert] = useState<string | null>(null);
  const [confirmAction, setConfirmAction] = useState<GitMutationAction | null>(null);
  const [pendingAction, setPendingAction] = useState<GitMutationAction | null>(null);
  const [isGeneratingCommitMessage, setIsGeneratingCommitMessage] = useState(false);
  const [commitMessage, setCommitMessage] = useState("");
  const [isCommitMessageExpanded, setCommitMessageExpanded] = useState(false);
  const [copiedCommitId, setCopiedCommitId] = useState<string | null>(null);
  const [historyHeight, setHistoryHeight] = useState(DEFAULT_HISTORY_HEIGHT);
  const [historyResize, setHistoryResize] = useState<{
    startY: number;
    startHeight: number;
  } | null>(null);

  useEffect(
    () => () => {
      window.clearTimeout(copyResetTimeoutRef.current);
      window.clearTimeout(actionAlertTimeoutRef.current);
    },
    [],
  );

  useEffect(() => {
    if (isMockMode) {
      const mockSnapshot = buildMockSnapshot();
      setSnapshot(mockSnapshot);
      setHistory(mockSnapshot.recentCommits);
      setIsLoading(false);
      setIsRefreshing(false);
      setError(null);
      setActionAlert(null);
      setConfirmAction(null);
      setPendingAction(null);
      setIsGeneratingCommitMessage(false);
      resetCommitMessage();
      return;
    }

    if (!workspaceId) {
      setSnapshot(null);
      setHistory([]);
      setIsLoading(false);
      setIsRefreshing(false);
      setError(null);
      setActionAlert(null);
      setConfirmAction(null);
      setPendingAction(null);
      setIsGeneratingCommitMessage(false);
      resetCommitMessage();
      return;
    }

    let cancelled = false;
    let unsubscribe: (() => Promise<void>) | null = null;
    setIsLoading(true);
    setError(null);
    setActionAlert(null);
    setConfirmAction(null);
    setPendingAction(null);
    setIsGeneratingCommitMessage(false);
    resetCommitMessage();

    void gitGetSnapshot(workspaceId)
      .then(async (nextSnapshot) => {
        if (cancelled || nextSnapshot === null) {
          return;
        }

        setSnapshot(nextSnapshot);

        if (!nextSnapshot.capabilities.repoAvailable) {
          setHistory([]);
          return;
        }

        const nextHistory = await gitGetHistory(workspaceId, 24);
        if (!cancelled) {
          setHistory(nextHistory);
        }
      })
      .catch((nextError) => {
        if (cancelled) {
          return;
        }
        const message = formatUiError(nextError, t("sourceControl.failedLoadGit"));
        setError(message);
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false);
        }
      });

    void gitSubscribe(workspaceId, (event: GitStreamEvent) => {
      if (cancelled) {
        return;
      }

      if (event.type === "refresh_started") {
        setIsRefreshing(true);
        return;
      }

      if (event.type === "snapshot_updated") {
        setSnapshot(event.snapshot);
        setHistory(event.snapshot.recentCommits);
        return;
      }

      if (event.type === "refresh_completed") {
        setIsRefreshing(false);
      }
    })
      .then((nextUnsubscribe) => {
        if (cancelled) {
          void nextUnsubscribe().catch(() => {});
          return;
        }
        unsubscribe = nextUnsubscribe;
      })
      .catch((subscriptionError) => {
        if (cancelled) {
          return;
        }

        const message = formatUiError(subscriptionError, t("sourceControl.failedSubscribeGit"));
        setError(message);
      });

    return () => {
      cancelled = true;
      if (unsubscribe) {
        void unsubscribe().catch(() => {});
      }
    };
  }, [isMockMode, workspaceId]);

  const getMaxHistoryHeight = () => {
    const panelHeight = panelRef.current?.getBoundingClientRect().height ?? DEFAULT_HISTORY_HEIGHT * 2;
    const topContentHeight = topContentRef.current?.getBoundingClientRect().height ?? 0;
    const historyResizeHandleHeight =
      historyResizeHandleRef.current?.getBoundingClientRect().height ?? 0;
    const changesHeaderHeight = changesHeaderRef.current?.getBoundingClientRect().height ?? 24;
    const minChangesHeight =
      Math.ceil(changesHeaderHeight) + CHANGES_SECTION_VERTICAL_GAP + MIN_CHANGES_BODY_HEIGHT;
    const maxByRatio = Math.floor(panelHeight * 0.5);
    const maxByAvailableSpace = Math.floor(
      panelHeight - topContentHeight - historyResizeHandleHeight - minChangesHeight,
    );

    return Math.max(0, Math.min(maxByRatio, maxByAvailableSpace));
  };

  const clampHistoryHeight = (nextHeight: number) =>
    Math.min(getMaxHistoryHeight(), Math.max(DEFAULT_HISTORY_HEIGHT, nextHeight));

  const getMinChangesSectionHeight = () => {
    const changesHeaderHeight = changesHeaderRef.current?.getBoundingClientRect().height ?? 24;
    return Math.ceil(changesHeaderHeight) + CHANGES_SECTION_VERTICAL_GAP + MIN_CHANGES_BODY_HEIGHT;
  };

  const getMinTopSectionHeight = () => {
    const topContentHeight = topContentRef.current?.getBoundingClientRect().height ?? (gitCliAvailable ? 36 : 88);
    return Math.ceil(topContentHeight) + 16 + getMinChangesSectionHeight();
  };

  useEffect(() => {
    const element = panelRef.current;
    if (!element) {
      return;
    }

    const syncHistoryHeight = () => {
      setHistoryHeight((current) => clampHistoryHeight(current));
    };

    syncHistoryHeight();

    if (typeof window !== "undefined") {
      window.addEventListener("resize", syncHistoryHeight);
    }

    if (typeof ResizeObserver === "undefined") {
      return () => {
        if (typeof window !== "undefined") {
          window.removeEventListener("resize", syncHistoryHeight);
        }
      };
    }

    const observer = new ResizeObserver(() => {
      syncHistoryHeight();
    });

    observer.observe(element);

    return () => {
      observer.disconnect();
      if (typeof window !== "undefined") {
        window.removeEventListener("resize", syncHistoryHeight);
      }
    };
  }, []);

  useEffect(() => {
    if (!historyResize || typeof window === "undefined") {
      return;
    }

    const handleMouseMove = (event: MouseEvent) => {
      const deltaY = historyResize.startY - event.clientY;
      const nextHeight = historyResize.startHeight + deltaY;
      setHistoryHeight(clampHistoryHeight(nextHeight));
    };

    const handleMouseUp = () => {
      setHistoryResize(null);
    };

    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;

    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [historyResize]);

  const totalChanges =
    (snapshot?.stagedFiles.length ?? 0) +
    (snapshot?.unstagedFiles.length ?? 0) +
    (snapshot?.untrackedFiles.length ?? 0);
  const hasStagedChanges = (snapshot?.stagedFiles.length ?? 0) > 0;
  const gitCliAvailable = snapshot?.capabilities.gitCliAvailable ?? false;
  const commitDisabled =
    !gitCliAvailable ||
    !hasStagedChanges ||
    pendingAction !== null ||
    commitMessage.trim().length === 0;
  const commitGeneratorDisabled =
    isMockMode ||
    !workspaceId ||
    commitMessageModelPlan === null ||
    totalChanges === 0 ||
    pendingAction !== null ||
    isGeneratingCommitMessage;

  useLayoutEffect(() => {
    setHistoryHeight((current) => clampHistoryHeight(current));
  }, [gitCliAvailable, isCommitMessageExpanded, layoutResizeSignal, totalChanges]);

  const branchLabel = snapshot?.isDetached
    ? "detached HEAD"
    : (snapshot?.headRef ?? t("sourceControl.noBranch"));

  const handleHistoryResizeStart = (event: React.MouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    setHistoryResize({
      startY: event.clientY,
      startHeight: historyHeight,
    });
  };

  const clearActionAlert = () => {
    setActionAlert(null);
    if (typeof window !== "undefined") {
      window.clearTimeout(actionAlertTimeoutRef.current);
    }
  };

  const clearConfirmAction = () => {
    setConfirmAction(null);
  };

  const resetCommitMessage = () => {
    setCommitMessage("");
    setCommitMessageExpanded(false);
  };

  const showActionAlert = (message: string) => {
    setActionAlert(message);
    if (typeof window === "undefined") {
      return;
    }

    window.clearTimeout(actionAlertTimeoutRef.current);
    actionAlertTimeoutRef.current = window.setTimeout(() => {
      setActionAlert((current) => (current === message ? null : current));
    }, ACTION_ALERT_TIMEOUT_MS);
  };

  const applyMutationResponse = (response: Extract<GitMutationResponseDto, { type: "completed" }>) => {
    setSnapshot(response.snapshot);
    setHistory(response.snapshot.recentCommits);
    setActionAlert(null);
    clearConfirmAction();
  };

  const runCliMutation = async (
    action: GitMutationAction,
    execute: (approved?: boolean) => Promise<GitMutationResponseDto>,
  ) => {
    setPendingAction(action);
    clearActionAlert();

    try {
      const response = await execute();
      if (response.type === "approval_required") {
        showActionAlert(response.reason);
        return;
      }

      applyMutationResponse(response);
      if (action === "commit") {
        resetCommitMessage();
      }
    } catch (nextError) {
      const message = formatUiError(nextError, t("sourceControl.gitActionFailed"));
      showActionAlert(message);
    } finally {
      setPendingAction(null);
    }
  };

  const handleRefresh = () => {
    if (isMockMode || !workspaceId || isRefreshing) {
      return;
    }

    setIsRefreshing(true);
    setError(null);
    clearActionAlert();
    clearConfirmAction();

    void gitRefresh(workspaceId)
      .then((nextSnapshot) => {
        setSnapshot(nextSnapshot);
        setHistory(nextSnapshot.recentCommits);
      })
      .catch((nextError) => {
        const message = formatUiError(nextError, t("sourceControl.failedRefreshSnapshot"));
        setError(message);
        setIsRefreshing(false);
      });
  };

  const handleToggleStage = (paths: string[], staged: boolean) => {
    if (paths.length === 0) {
      return;
    }

    setPendingPaths((current) => new Set([...current, ...paths]));
    clearActionAlert();
    clearConfirmAction();

    if (isMockMode) {
      setSnapshot((current) =>
        current === null ? current : applyMockStageMutation(current, paths, staged ? "unstage" : "stage"),
      );
      setPendingPaths((current) => {
        const next = new Set(current);
        paths.forEach((path) => next.delete(path));
        return next;
      });
      return;
    }

    if (!workspaceId) {
      setPendingPaths((current) => {
        const next = new Set(current);
        paths.forEach((path) => next.delete(path));
        return next;
      });
      return;
    }

    const mutate = staged ? gitUnstage : gitStage;

    void mutate(workspaceId, paths)
      .then((nextSnapshot) => {
        setSnapshot(nextSnapshot);
        setHistory(nextSnapshot.recentCommits);
      })
      .catch((nextError) => {
        const message = formatUiError(nextError, t("sourceControl.gitActionFailed"));
        showActionAlert(message);
      })
      .finally(() => {
        setPendingPaths((current) => {
          const next = new Set(current);
          paths.forEach((path) => next.delete(path));
          return next;
        });
      });
  };

  const handleToggleAll = (paths: string[], staged: boolean) => {
    handleToggleStage(paths, staged);
  };

  const handleCommit = () => {
    if (pendingAction !== null) {
      return;
    }

    clearActionAlert();
    setConfirmAction("commit");
  };

  const handleGenerateCommitMessage = () => {
    if (commitGeneratorDisabled || !workspaceId || !commitMessageModelPlan) {
      return;
    }

    setIsGeneratingCommitMessage(true);
    clearActionAlert();

    void gitGenerateCommitMessage(
      workspaceId,
      commitMessageModelPlan,
      commitMessageLanguage,
      commitMessagePrompt,
    )
      .then((message) => {
        setCommitMessage(message);
        setCommitMessageExpanded(message.includes("\n"));
      })
      .catch((nextError) => {
        const message = formatUiError(nextError, t("sourceControl.failedGenerateCommitMsg"));
        showActionAlert(message);
      })
      .finally(() => {
        setIsGeneratingCommitMessage(false);
      });
  };

  const handleRemoteAction = (action: Exclude<GitMutationAction, "commit">) => {
    if (pendingAction !== null) {
      return;
    }

    clearActionAlert();
    setConfirmAction(action);
  };

  const executeConfirmedAction = (action: GitMutationAction) => {
    if (pendingAction !== null) {
      return;
    }

    clearConfirmAction();

    if (action === "commit") {
      if (isMockMode) {
        try {
          setSnapshot((current) => {
            if (current === null) {
              return current;
            }

            const nextSnapshot = applyMockCommitMutation(current, commitMessage);
            setHistory(nextSnapshot.recentCommits);
            return nextSnapshot;
          });
          clearActionAlert();
          resetCommitMessage();
        } catch (nextError) {
          const message = formatUiError(nextError, t("sourceControl.commitFailed"));
          showActionAlert(message);
        }
        return;
      }

      if (!workspaceId) {
        return;
      }

      void runCliMutation("commit", () => gitCommit(workspaceId, commitMessage, true));
      return;
    }

    if (isMockMode) {
      setSnapshot((current) => {
        if (current === null) {
          return current;
        }

        const nextSnapshot = applyMockRemoteMutation(current, action);
        setHistory(nextSnapshot.recentCommits);
        return nextSnapshot;
      });
      clearActionAlert();
      return;
    }

    if (!workspaceId) {
      return;
    }

    const invokeAction =
      action === "fetch"
        ? () => gitFetch(workspaceId, true)
        : action === "pull"
          ? () => gitPull(workspaceId, true)
          : () => gitPush(workspaceId, true);

    void runCliMutation(action, invokeAction);
  };

  const handleCopyCommitId = async (commitId: string) => {
    if (typeof window === "undefined" || !navigator?.clipboard?.writeText) {
      return;
    }

    try {
      await navigator.clipboard.writeText(commitId);
      setCopiedCommitId(commitId);
      window.clearTimeout(copyResetTimeoutRef.current);
      copyResetTimeoutRef.current = window.setTimeout(() => {
        setCopiedCommitId((current) => (current === commitId ? null : current));
      }, 2000);
    } catch {
      // Ignore clipboard failures to avoid disrupting the Git panel.
    }
  };

  if (workspaceBootstrapError) {
    return (
      <EmptyState
        title={t("sourceControl.drawerUnavailable")}
        body={workspaceBootstrapError}
        error
      />
    );
  }

  if (!currentProject) {
    return (
      <EmptyState
        title={t("sourceControl.selectWorkspaceFirst")}
        body="Choose a workspace in the thread area before opening Git status and history."
      />
    );
  }

  if (!isMockMode && !workspaceId && !workspaceBootstrapError) {
    return (
      <EmptyState
        title={t("sourceControl.preparingGitContext")}
        body="The selected workspace is still being attached. Git data will load in a moment."
      />
    );
  }

  if (isLoading) {
    return (
      <div className="flex h-full min-h-0 items-center justify-center">
        <div className="flex items-center gap-2 text-sm text-app-subtle">
          <LoaderCircle className="size-4 animate-spin" />
          <span>Loading Git snapshot…</span>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <EmptyState
        title={t("sourceControl.failedLoadGitState")}
        body={error}
        error
      />
    );
  }

  if (!snapshot?.capabilities.repoAvailable) {
    return (
      <EmptyState
        title={t("sourceControl.notGitRepo")}
        body="Project browsing still works, but Git history and diff previews stay hidden until the workspace is inside a repository."
      />
    );
  }

  return (
    <>
      <Dialog
        open={confirmAction !== null}
        onOpenChange={(open) => {
          if (!open) {
            clearConfirmAction();
          }
        }}
      >
        <DialogContent
          showCloseButton={false}
          className="max-w-md rounded-2xl border-app-border bg-app-surface p-5"
          onKeyDown={(event) => {
            if (event.key !== "Enter" || event.nativeEvent.isComposing) {
              return;
            }

            if (!confirmAction) {
              return;
            }

            event.preventDefault();
            event.stopPropagation();
            executeConfirmedAction(confirmAction);
          }}
        >
          <DialogHeader className="gap-2 text-left">
            <DialogTitle className="text-base font-semibold text-app-foreground">
              {confirmAction ? t("sourceControl.confirmAction", { action: gitActionLabel(confirmAction, t) }) : t("sourceControl.confirmActionDefault")}
            </DialogTitle>
            <DialogDescription className="text-[13px] leading-6 text-app-subtle">
              {confirmAction === "commit"
                ? "Commit will create a new local revision from the currently staged changes."
                : confirmAction
                  ? `${gitActionLabel(confirmAction, t)} will operate on the remote repository for the current branch.`
                  : "This action requires confirmation."}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="mt-1">
            <Button
              variant="outline"
              onClick={clearConfirmAction}
            >
              Cancel
            </Button>
            <Button
              onClick={() => {
                if (confirmAction) {
                  executeConfirmedAction(confirmAction);
                }
              }}
              autoFocus
            >
              Confirm
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div
        ref={panelRef}
        className="relative flex h-full min-h-0 flex-col px-4 pb-4 pt-3"
      >
        {actionAlert ? (
          <div className="pointer-events-none absolute inset-x-4 bottom-4 z-30 flex justify-end">
            <Alert
              variant="destructive"
              className="pointer-events-auto max-w-[320px] grid-cols-[auto_1fr] border-app-danger/25 bg-app-surface shadow-[0_18px_40px_-24px_rgba(15,23,42,0.5)]"
            >
              <AlertCircle className="mt-0.5 size-4" />
              <AlertDescription className="col-start-2 text-[12px] leading-5 text-app-danger">
                {actionAlert}
              </AlertDescription>
            </Alert>
          </div>
        ) : null}

        <div
          className="flex min-h-0 flex-1 flex-col overflow-hidden"
          style={{ minHeight: `${getMinTopSectionHeight()}px` }}
        >
        <div ref={topContentRef} className="shrink-0">
          <div className="flex items-start gap-2">
            <div className={cn("relative h-9 min-w-0 flex-1", isCommitMessageExpanded && "z-20")}>
              <Textarea
                value={commitMessage}
                readOnly={!gitCliAvailable || pendingAction !== null}
                placeholder={
                  gitCliAvailable
                    ? t("sourceControl.commitMsgPlaceholder")
                    : t("sourceControl.installGitHint")
                }
                aria-label={t("sourceControl.commitMsgLabel")}
                rows={isCommitMessageExpanded ? 4 : 1}
                onChange={(event) => setCommitMessage(event.target.value)}
                onFocus={() => setCommitMessageExpanded(true)}
                onBlur={() => setCommitMessageExpanded(false)}
                className={cn(
                  "resize-none overflow-hidden rounded-xl border-app-border px-3 pr-10 text-[13px] font-medium leading-5 text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0",
                  isCommitMessageExpanded
                    ? "absolute inset-x-0 top-0 min-h-[112px] bg-app-surface shadow-[0_24px_48px_-24px_rgba(15,23,42,0.48)]"
                    : "h-9 min-h-9 bg-transparent shadow-none",
                  !gitCliAvailable && "cursor-not-allowed opacity-70",
                )}
              />
              <button
                type="button"
                aria-label={t("sourceControl.generateCommitMessage")}
                title={
                  isMockMode
                    ? t("sourceControl.mockModeUnavailable")
                    : !workspaceId
                      ? t("sourceControl.workspaceRequired")
                      : commitMessageModelPlan === null
                        ? t("sourceControl.selectModelFirst")
                        : totalChanges === 0
                          ? t("sourceControl.noChangesToSummarize")
                          : isGeneratingCommitMessage
                            ? t("sourceControl.generatingCommitMsg")
                            : t("sourceControl.generateFromProfile")
                }
                disabled={commitGeneratorDisabled}
                className={cn(
                  "absolute right-1.5 flex size-6 items-center justify-center rounded-md text-app-subtle transition-[top,transform] duration-200 disabled:cursor-not-allowed disabled:opacity-60",
                  isCommitMessageExpanded ? "top-3" : "top-1/2 -translate-y-1/2",
                )}
                onClick={handleGenerateCommitMessage}
              >
                {isGeneratingCommitMessage ? (
                  <LoaderCircle className="size-3.5 animate-spin" />
                ) : (
                  <Sparkles className="size-3.5" />
                )}
              </button>
            </div>
            <button
              type="button"
              aria-label={t("sourceControl.commitLabel")}
              title={
                !gitCliAvailable
                  ? t("sourceControl.gitCliRequired")
                  : !hasStagedChanges
                    ? t("sourceControl.stageChangesFirst")
                    : commitMessage.trim().length === 0
                      ? t("sourceControl.writeCommitMsgFirst")
                  : pendingAction === "commit"
                    ? t("sourceControl.commitInProgress")
                    : t("sourceControl.commitStagedChanges")
              }
              disabled={commitDisabled}
              className={cn(
                "flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-border transition-colors",
                commitDisabled
                  ? "cursor-not-allowed text-app-subtle opacity-60"
                  : "text-app-foreground hover:bg-app-surface-hover",
              )}
              onClick={handleCommit}
            >
              {pendingAction === "commit" ? (
                <LoaderCircle className="size-4 animate-spin" />
              ) : (
                <Check className="size-4" />
              )}
            </button>
          </div>

          {!gitCliAvailable ? (
            <div className="mt-3 rounded-xl border border-app-border bg-app-surface-muted/70 px-3 py-2 text-[12px] leading-5 text-app-subtle">
              Git CLI is unavailable. Status, diff, and history stay readable, but commit, fetch, pull, and push remain disabled.
            </div>
          ) : null}
        </div>

        <section
          className="mt-4 flex min-h-0 flex-1 flex-col"
          style={{ minHeight: `${getMinChangesSectionHeight()}px` }}
        >
          <div ref={changesHeaderRef} className={DRAWER_SECTION_HEADER_CLASS}>
            <div className="flex items-center gap-2">
              <p className="text-sm font-semibold text-app-foreground">Changes</p>
              <span className="rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
                {totalChanges}
              </span>
            </div>
            <div className="flex items-center gap-1">
              <button
                type="button"
                aria-label={t("sourceControl.refreshSnapshot")}
                title={t("sourceControl.refreshSnapshot")}
                className={DRAWER_ICON_ACTION_CLASS}
                onClick={handleRefresh}
              >
                <RefreshCw className={cn("size-4", isRefreshing && "animate-spin")} />
              </button>
            </div>
          </div>

          <div className="mt-2 min-h-[160px] flex-1 overflow-auto overscroll-contain pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            {totalChanges === 0 ? (
              <div className="flex h-full min-h-[180px] items-center justify-center">
                <div className="text-center">
                  <div className="mx-auto flex size-10 items-center justify-center rounded-2xl border border-app-border bg-app-surface-muted text-app-subtle">
                    <FileSearch className="size-4" />
                  </div>
                  <p className="mt-3 text-sm font-medium text-app-foreground">
                    Working tree is clean
                  </p>
                  <p className="mt-1 text-[12px] text-app-subtle">
                    No staged, unstaged, or untracked changes were found.
                  </p>
                </div>
              </div>
            ) : (
              <div className={DRAWER_LIST_STACK_CLASS}>
                <ChangeGroup
                  title={t("sourceControl.staged")}
                  files={snapshot.stagedFiles}
                  staged
                  pendingPaths={pendingPaths}
                  t={t}
                  onOpenDiffPreview={onOpenDiffPreview}
                  onToggleStage={handleToggleStage}
                  onToggleAll={handleToggleAll}
                />
                <ChangeGroup
                  title={t("sourceControl.tracked")}
                  files={snapshot.unstagedFiles}
                  staged={false}
                  pendingPaths={pendingPaths}
                  t={t}
                  onOpenDiffPreview={onOpenDiffPreview}
                  onToggleStage={handleToggleStage}
                  onToggleAll={handleToggleAll}
                />
                <ChangeGroup
                  title={t("sourceControl.untracked")}
                  files={snapshot.untrackedFiles}
                  staged={false}
                  pendingPaths={pendingPaths}
                  t={t}
                  onOpenDiffPreview={onOpenDiffPreview}
                  onToggleStage={handleToggleStage}
                  onToggleAll={handleToggleAll}
                />
              </div>
            )}
          </div>
        </section>
      </div>

      <div
        ref={historyResizeHandleRef}
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize history panel"
        className="group mt-3 flex h-3 shrink-0 cursor-row-resize items-center"
        onMouseDown={handleHistoryResizeStart}
      >
        <div className="flex w-full items-center justify-center">
          <div className="h-1 w-14 rounded-full bg-app-border transition-colors group-hover:bg-app-border-strong" />
        </div>
      </div>

      <section
        className="relative flex min-h-0 flex-col overflow-hidden"
        style={{ height: `${historyHeight}px` }}
      >
        <div className={DRAWER_SECTION_HEADER_CLASS}>
          <div className="flex items-center gap-2">
            <p className="text-sm font-semibold text-app-foreground">History</p>
            <span className="rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
              {history.length}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              aria-label="Fetch"
              title={
                gitCliAvailable
                  ? pendingAction === "fetch"
                    ? "Fetch in progress"
                    : "Fetch remote updates"
                  : "Git CLI is required"
              }
              disabled={!gitCliAvailable || pendingAction !== null}
              className={cn(
                DRAWER_ICON_ACTION_CLASS,
                (!gitCliAvailable || pendingAction !== null) && "opacity-60",
              )}
              onClick={() => handleRemoteAction("fetch")}
            >
              {pendingAction === "fetch" ? (
                <LoaderCircle className="size-4 animate-spin" />
              ) : (
                <Download className="size-4" />
              )}
            </button>
            <button
              type="button"
              aria-label="Pull"
              title={
                gitCliAvailable
                  ? pendingAction === "pull"
                    ? "Pull in progress"
                    : "Pull remote updates"
                  : "Git CLI is required"
              }
              disabled={!gitCliAvailable || pendingAction !== null}
              className={cn(
                DRAWER_ICON_ACTION_CLASS,
                (!gitCliAvailable || pendingAction !== null) && "opacity-60",
              )}
              onClick={() => handleRemoteAction("pull")}
            >
              {pendingAction === "pull" ? (
                <LoaderCircle className="size-4 animate-spin" />
              ) : (
                <ArrowDownToLine className="size-4" />
              )}
            </button>
            <button
              type="button"
              aria-label="Push"
              title={
                gitCliAvailable
                  ? pendingAction === "push"
                    ? "Push in progress"
                    : "Push local commits"
                  : "Git CLI is required"
              }
              disabled={!gitCliAvailable || pendingAction !== null}
              className={cn(
                DRAWER_ICON_ACTION_CLASS,
                (!gitCliAvailable || pendingAction !== null) && "opacity-60",
              )}
              onClick={() => handleRemoteAction("push")}
            >
              {pendingAction === "push" ? (
                <LoaderCircle className="size-4 animate-spin" />
              ) : (
                <ArrowUpFromLine className="size-4" />
              )}
            </button>
          </div>
        </div>

        <div className="mt-2 min-h-0 flex-1 overflow-auto overscroll-contain pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          <div className="mb-2 flex items-center gap-2 rounded-xl border border-app-border bg-app-surface-muted/60 px-3 py-2 text-[11px] text-app-subtle">
            <GitBranch className="size-3.5 shrink-0" />
            <span className="truncate">{branchLabel}</span>
            <span className="ml-auto">
              ↑ {snapshot.aheadCount} / ↓ {snapshot.behindCount}
            </span>
          </div>

          <TooltipProvider>
            <div className={DRAWER_LIST_STACK_CLASS}>
              {history.map((item, index) => (
                <div key={item.id} className="relative pl-4">
                  {item.isHead ? (
                    <span className="absolute inset-y-0 -left-1.5 rounded-lg bg-primary/8" />
                  ) : null}
                  {index < history.length - 1 ? (
                    <span className="absolute left-[4px] top-[18px] h-[calc(100%+0.25rem)] w-px bg-app-border" />
                  ) : null}
                  <span
                    className={cn(
                      "absolute left-0 top-1/2 size-2.5 -translate-y-1/2 rounded-full border",
                      item.isHead
                        ? "border-primary/30 bg-primary/72 shadow-[0_1px_2px_rgba(15,23,42,0.14)]"
                        : "border-app-border bg-app-drawer",
                    )}
                  />
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <div
                        role="button"
                        tabIndex={0}
                        aria-label={`Copy commit ${item.shortId} to clipboard`}
                        className={cn(
                          "relative cursor-pointer rounded-lg px-2.5 py-1.5 outline-none transition-colors duration-200 hover:bg-app-surface-muted/40 focus-visible:bg-app-surface-muted/40",
                          copiedCommitId === item.id && "bg-app-surface-muted/50",
                        )}
                        onClick={() => {
                          void handleCopyCommitId(item.id);
                        }}
                        onKeyDown={(event) => {
                          if (event.key !== "Enter" && event.key !== " ") {
                            return;
                          }

                          event.preventDefault();
                          void handleCopyCommitId(item.id);
                        }}
                      >
                        <div className="flex items-center justify-between gap-3">
                          <p className="min-w-0 flex-1 truncate text-[13px] font-medium leading-5 text-app-foreground">
                            {item.summary}
                          </p>
                          <span
                            className={cn(
                              "shrink-0 text-[11px] text-app-subtle transition-colors duration-200",
                              copiedCommitId === item.id && "text-app-success",
                            )}
                          >
                            {copiedCommitId === item.id ? "Copied" : item.shortId}
                          </span>
                        </div>
                        <div className="mt-1 flex items-center justify-between gap-3">
                          <p className="min-w-0 flex-1 truncate text-[11px] text-app-subtle">
                            {item.authorName} · {formatRelativeTime(item.committedAt)}
                          </p>
                          {item.refs.length > 0 ? (
                            <div className="flex shrink-0 flex-wrap justify-end gap-1">
                              {item.refs.map((ref) => (
                                <span
                                  key={ref}
                                  className={cn(
                                    "inline-flex items-center rounded-full font-medium leading-none transition-[background-color,color,box-shadow] duration-200",
                                    ref === "HEAD"
                                      ? "h-5 bg-primary/88 px-1.5 text-[9px] text-primary-foreground shadow-[0_1px_2px_rgba(15,23,42,0.14)]"
                                      : "h-6 bg-app-surface-muted px-2 text-[10px] text-app-muted",
                                  )}
                                >
                                  {ref}
                                </span>
                              ))}
                            </div>
                          ) : null}
                        </div>
                      </div>
                    </TooltipTrigger>
                    <TooltipContent
                      side="top"
                      align="start"
                      sideOffset={6}
                      className="max-w-[28rem] whitespace-normal break-words"
                    >
                      <div className="space-y-1">
                        <p>{item.summary}</p>
                        <p className="text-[11px] opacity-80">
                          Click to copy full commit id
                        </p>
                      </div>
                    </TooltipContent>
                  </Tooltip>
                </div>
              ))}
            </div>
          </TooltipProvider>
        </div>
      </section>
      </div>
    </>
  );
}

export function GitDiffPreviewPanel({
  workspaceId,
  selection,
  onClose,
}: GitDiffPreviewPanelProps) {
  const t = useT();
  const [isMetaExpanded, setMetaExpanded] = useState(false);
  const [diff, setDiff] = useState<GitDiffDto | null>(null);
  const [fileStatus, setFileStatus] = useState<GitFileStatusDto | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(isTauri() && Boolean(workspaceId));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri() || !workspaceId) {
      setDiff(null);
      setFileStatus(null);
      setIsLoading(false);
      setError(null);
      return;
    }

    let cancelled = false;
    setIsLoading(true);
    setError(null);

    void Promise.all([
      gitGetDiff(workspaceId, selection.path, selection.staged),
      gitGetFileStatus(workspaceId, selection.path),
    ])
      .then(([nextDiff, nextFileStatus]) => {
        if (cancelled) {
          return;
        }

        setDiff(nextDiff);
        setFileStatus(nextFileStatus);
      })
      .catch((nextError) => {
        if (cancelled) {
          return;
        }

        const message = formatUiError(nextError, t("sourceControl.failedLoadFileDiff"));
        setError(message);
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [selection.path, selection.staged, workspaceId]);

  const mockFile = useMemo(
    () => buildMockPreviewSelection(selection, t),
    [selection, t],
  );
  const mockPreview = useMemo(() => buildGitDiffPreview(mockFile), [mockFile]);
  const splitRows = useMemo<ReadonlyArray<GitSplitDiffRow>>(() => {
    if (!isTauri() || !workspaceId || diff === null) {
      return buildGitSplitDiffRows(mockFile);
    }

    return buildSplitRowsFromDiff(diff);
  }, [diff, mockFile, workspaceId]);
  const fileName = selection.path.split("/").pop() ?? selection.path;

  const metaLines = !isTauri() || !workspaceId || diff === null
    ? mockPreview.meta
    : [
        `scope: ${selection.staged ? "staged" : "working_tree"}`,
        diff.oldPath ? `--- ${diff.oldPath}` : null,
        diff.newPath ? `+++ ${diff.newPath}` : null,
        ...diff.hunks.map((hunk) => hunk.header),
      ].filter((line): line is string => Boolean(line));

  const statusPills = !fileStatus
    ? []
    : [
        fileStatus.stagedStatus
          ? `staged ${statusLabel(fileStatus.stagedStatus, t)}`
          : null,
        fileStatus.unstagedStatus
          ? `working ${statusLabel(fileStatus.unstagedStatus, t)}`
          : null,
        fileStatus.isUntracked ? "untracked" : null,
        fileStatus.isIgnored ? "ignored" : null,
      ].filter((value): value is string => value !== null);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-app-chrome/50 px-6 py-12 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="flex h-[min(82vh,860px)] w-full max-w-7xl flex-col overflow-hidden rounded-[24px] border border-app-border bg-app-surface shadow-[0_32px_96px_rgba(15,23,42,0.28)] dark:shadow-[0_32px_96px_rgba(0,0,0,0.56)]"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-app-border px-5 py-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="shrink-0">
                <ProjectTreeIcon icon={selection.icon} />
              </span>
              <p className="truncate text-sm font-semibold text-app-foreground">{fileName}</p>
              <span className="shrink-0 rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
                {formatChangeSummary(selection, t)}
              </span>
              <span
                className={cn(
                  "shrink-0 rounded-md px-1.5 py-0.5 text-[11px]",
                  selection.staged
                    ? "bg-app-foreground text-app-drawer"
                    : "bg-app-surface-muted text-app-subtle",
                )}
              >
                {selection.staged ? "Staged" : "Working tree"}
              </span>
            </div>
            <div className="mt-1 flex flex-wrap items-center gap-1.5">
              <p className="truncate text-[12px] text-app-subtle">{selection.path}</p>
              {statusPills.map((pill) => (
                <span
                  key={pill}
                  className="rounded-full bg-app-surface-muted px-2 py-1 text-[10px] text-app-muted"
                >
                  {pill}
                </span>
              ))}
              <button
                type="button"
                aria-label={isMetaExpanded ? "Collapse diff metadata" : "Expand diff metadata"}
                title={isMetaExpanded ? "Collapse diff metadata" : "Expand diff metadata"}
                className="flex size-5 shrink-0 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                onClick={() => setMetaExpanded((current) => !current)}
              >
                <ChevronDown className={cn("size-3.5 transition-transform", !isMetaExpanded && "-rotate-90")} />
              </button>
            </div>
          </div>

          <button
            type="button"
            aria-label="Close diff preview"
            title="Close diff preview"
            className="flex size-8 shrink-0 items-center justify-center rounded-lg text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
            onClick={onClose}
          >
            <CircleX className="size-4" />
          </button>
        </div>

        {isMetaExpanded ? (
          <div className="shrink-0 border-b border-app-border bg-app-surface-muted/70 px-5 py-3 font-mono text-[11px] text-app-subtle">
            {metaLines.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        ) : null}

        {error ? (
          <div className="shrink-0 border-b border-app-border bg-app-danger/8 px-5 py-3 text-[12px] text-app-danger">
            {error}
          </div>
        ) : null}

        {!error && diff?.truncated ? (
          <div className="shrink-0 border-b border-app-border bg-app-surface-muted/70 px-5 py-3 text-[12px] text-app-subtle">
            Diff output was truncated to keep the preview responsive.
          </div>
        ) : null}

        {!error && diff?.isBinary ? (
          <div className="flex min-h-0 flex-1 items-center justify-center px-6 text-center">
            <div>
              <div className="mx-auto flex size-11 items-center justify-center rounded-2xl border border-app-border bg-app-surface-muted text-app-subtle">
                <FileSearch className="size-5" />
              </div>
              <p className="mt-4 text-sm font-semibold text-app-foreground">
                Binary diff preview is not available
              </p>
              <p className="mt-2 text-[12px] text-app-subtle">
                This file changed, but libgit2 reported binary content instead of patch text.
              </p>
            </div>
          </div>
        ) : (
          <>
            <div className="grid shrink-0 grid-cols-2 border-b border-app-border bg-app-surface-muted/50 text-[11px] uppercase tracking-[0.12em] text-app-subtle">
              <div className="border-r border-app-border px-4 py-2">Old</div>
              <div className="px-4 py-2">New</div>
            </div>

            <div className="min-h-0 flex-1 overflow-auto overscroll-contain bg-app-drawer font-mono text-[12px] leading-6 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              {isLoading ? (
                <div className="flex h-full items-center justify-center">
                  <div className="flex items-center gap-2 text-sm text-app-subtle">
                    <LoaderCircle className="size-4 animate-spin" />
                    <span>Loading diff preview…</span>
                  </div>
                </div>
              ) : (
                splitRows.map((row, index) => (
                  <div key={`${row.kind}-${index}-${row.leftText}-${row.rightText}`} className="grid grid-cols-2 border-b border-app-border/60">
                    <div
                      className={cn(
                        "grid min-w-0 grid-cols-[56px_1fr] items-start border-r border-app-border/70",
                        row.kind === "remove" || row.kind === "modified"
                          ? "bg-app-danger/10"
                          : "bg-transparent",
                      )}
                    >
                      <span className="select-none border-r border-app-border/60 px-3 text-right text-app-subtle">
                        {row.leftNumber ?? ""}
                      </span>
                      <span
                        className={cn(
                          "overflow-x-auto whitespace-pre px-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
                          row.kind === "remove" || row.kind === "modified"
                            ? "text-app-danger"
                            : "text-app-foreground",
                        )}
                      >
                        {row.leftText}
                      </span>
                    </div>

                    <div
                      className={cn(
                        "grid min-w-0 grid-cols-[56px_1fr] items-start",
                        row.kind === "add" || row.kind === "modified"
                          ? "bg-app-success/10"
                          : "bg-transparent",
                      )}
                    >
                      <span className="select-none border-r border-app-border/60 px-3 text-right text-app-subtle">
                        {row.rightNumber ?? ""}
                      </span>
                      <span
                        className={cn(
                          "overflow-x-auto whitespace-pre px-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
                          row.kind === "add" || row.kind === "modified"
                            ? "text-app-success"
                            : "text-app-foreground",
                        )}
                      >
                        {row.rightText}
                      </span>
                    </div>
                  </div>
                ))
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
