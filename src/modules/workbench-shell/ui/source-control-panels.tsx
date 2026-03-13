import { useEffect, useRef, useState } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  ChevronDown,
  CircleX,
  Download,
  Plus,
  RefreshCw,
  Sparkles,
  Undo2,
} from "lucide-react";
import { Input } from "@/shared/ui/input";
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
import type { GitChangeFile } from "@/modules/workbench-shell/model/types";
import { ProjectTreeIcon } from "@/modules/workbench-shell/ui/project-tree-icon";

export function GitPanel({ onOpenDiffPreview }: { onOpenDiffPreview: (fileId: string, isStaged: boolean) => void }) {
  const [commitMessage, setCommitMessage] = useState("");
  const [stagedFiles, setStagedFiles] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(GIT_CHANGE_FILES.map((file) => [file.id, file.initialStaged])),
  );
  const [activeHistoryAction, setActiveHistoryAction] = useState<"fetch" | "pull" | "push" | "refresh" | null>(null);
  const historyActionTimeoutRef = useRef<number | null>(null);
  const stagedCount = GIT_CHANGE_FILES.filter((file) => stagedFiles[file.id]).length;

  useEffect(() => {
    return () => {
      if (historyActionTimeoutRef.current) {
        window.clearTimeout(historyActionTimeoutRef.current);
      }
    };
  }, []);

  const handleToggleStage = (fileId: string) => {
    setStagedFiles((current) => ({
      ...current,
      [fileId]: !current[fileId],
    }));
  };

  const handleStageAll = () => {
    setStagedFiles(Object.fromEntries(GIT_CHANGE_FILES.map((file) => [file.id, true])));
  };

  const handleUnstageAll = () => {
    setStagedFiles(Object.fromEntries(GIT_CHANGE_FILES.map((file) => [file.id, false])));
  };

  const handleGenerateCommitMessage = () => {
    setCommitMessage(
      stagedCount >= 2
        ? "feat(git-panel): align source control workflow with VS Code"
        : "chore(git-panel): update tracked changes and history panel",
    );
  };

  const handleHistoryAction = (action: "fetch" | "pull" | "push" | "refresh") => {
    setActiveHistoryAction(action);

    if (historyActionTimeoutRef.current) {
      window.clearTimeout(historyActionTimeoutRef.current);
    }

    historyActionTimeoutRef.current = window.setTimeout(() => {
      setActiveHistoryAction(null);
      historyActionTimeoutRef.current = null;
    }, 800);
  };

  return (
    <div className="relative flex h-full min-h-0 flex-col px-4 pb-4 pt-3">
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex items-center gap-2">
          <div className="relative min-w-0 flex-1">
            <Input
              value={commitMessage}
              onChange={(event) => setCommitMessage(event.target.value)}
              placeholder="Commit Message"
              aria-label="Commit Message"
              className="h-9 rounded-xl border-app-border bg-transparent px-3 pr-10 text-[13px] font-medium text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0"
            />
            <button
              type="button"
              aria-label="智能生成 Commit Message"
              title="智能生成 Commit Message"
              className="absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={handleGenerateCommitMessage}
            >
              <Sparkles className="size-3.5" />
            </button>
          </div>
          <button
            type="button"
            aria-label="Commit"
            title="Commit"
            className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-border text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
          >
            <Check className="size-4" />
          </button>
        </div>

        <section className="mt-4 flex min-h-0 flex-1 flex-col">
          <div className={DRAWER_SECTION_HEADER_CLASS}>
            <div className="flex items-center gap-2">
              <p className="text-sm font-semibold text-app-foreground">Changes</p>
              <span className="rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
                {GIT_CHANGE_FILES.length}
              </span>
            </div>
            <div className="flex items-center gap-1">
              <button
                type="button"
                aria-label="全部取消"
                title="全部取消"
                className={DRAWER_ICON_ACTION_CLASS}
                onClick={handleUnstageAll}
              >
                <Undo2 className="size-4" />
              </button>
              <button
                type="button"
                aria-label="全部加入"
                title="全部加入"
                className={DRAWER_ICON_ACTION_CLASS}
                onClick={handleStageAll}
              >
                <Plus className="size-4" />
              </button>
            </div>
          </div>

          <div className="mt-2 min-h-0 flex-1 overflow-auto overscroll-contain pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <div className={DRAWER_LIST_STACK_CLASS}>
              {GIT_CHANGE_FILES.map((file) => {
                const isStaged = Boolean(stagedFiles[file.id]);

                return (
                  <div
                    key={file.id}
                    role="button"
                    tabIndex={0}
                    title={file.path}
                    className={cn(
                      "flex cursor-pointer items-center gap-2 hover:bg-app-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-app-border-strong",
                      DRAWER_LIST_ROW_CLASS,
                    )}
                    onClick={() => onOpenDiffPreview(file.id, isStaged)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        onOpenDiffPreview(file.id, isStaged);
                      }
                    }}
                  >
                    <span
                      className={cn(
                        "inline-flex min-w-5 shrink-0 items-center justify-center rounded px-1 text-[10px] font-semibold",
                        file.status === "A"
                          ? "text-app-success"
                          : file.status === "D"
                            ? "text-app-danger"
                            : "text-app-subtle",
                      )}
                    >
                      {file.status}
                    </span>
                    <span className={cn(DRAWER_LIST_LABEL_CLASS, "text-app-muted")}>{file.path.split("/").pop()}</span>
                    <span className={DRAWER_LIST_META_CLASS}>{file.summary}</span>
                    <button
                      type="button"
                      role="checkbox"
                      aria-checked={isStaged}
                      aria-label={isStaged ? `取消暂存 ${file.path}` : `暂存 ${file.path}`}
                      title={isStaged ? "Unstage" : "Stage"}
                      className={cn(
                        "flex size-4 shrink-0 items-center justify-center rounded border shadow-[0_1px_2px_rgba(15,23,42,0.12)] transition-[background-color,border-color,color,box-shadow,transform] duration-200",
                        isStaged
                          ? "border-primary/20 bg-primary/88 text-primary-foreground hover:bg-primary/82 hover:shadow-[0_4px_10px_rgba(15,23,42,0.14)]"
                          : "border-app-border bg-transparent text-transparent hover:border-app-border-strong",
                      )}
                      onClick={(event) => {
                        event.stopPropagation();
                        handleToggleStage(file.id);
                      }}
                    >
                      <Check className="size-2.5" />
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        </section>
      </div>

      <section className="mt-3 flex h-[208px] shrink-0 flex-col">
        <div className={DRAWER_SECTION_HEADER_CLASS}>
          <p className="text-sm font-semibold text-app-foreground">Network</p>
          <div className="flex items-center gap-1">
            <button
              type="button"
              aria-label="Fetch"
              title="Fetch"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("fetch")}
            >
              <Download className={cn("size-4", activeHistoryAction === "fetch" && "animate-pulse")} />
            </button>
            <button
              type="button"
              aria-label="Pull"
              title="Pull"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("pull")}
            >
              <ArrowDownToLine className={cn("size-4", activeHistoryAction === "pull" && "animate-pulse")} />
            </button>
            <button
              type="button"
              aria-label="Push"
              title="Push"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("push")}
            >
              <ArrowUpFromLine className={cn("size-4", activeHistoryAction === "push" && "animate-pulse")} />
            </button>
            <button
              type="button"
              aria-label="刷新历史"
              title="刷新历史"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("refresh")}
            >
              <RefreshCw className={cn("size-4", activeHistoryAction === "refresh" && "animate-spin")} />
            </button>
          </div>
        </div>

        <div className="mt-2.5 min-h-0 flex-1 overflow-auto overscroll-contain pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          <div className={DRAWER_LIST_STACK_CLASS}>
            {GIT_HISTORY_ITEMS.map((item, index) => (
              <div key={item.id} className="relative pl-4">
                {item.refs?.includes("HEAD") ? (
                  <span className="absolute inset-y-0 -left-1.5 rounded-lg bg-primary/8" />
                ) : null}
                {index < GIT_HISTORY_ITEMS.length - 1 ? (
                  <span className="absolute left-[4px] top-[18px] h-[calc(100%+0.25rem)] w-px bg-app-border" />
                ) : null}
                <span
                  className={cn(
                    "absolute left-0 top-1/2 size-2.5 -translate-y-1/2 rounded-full border",
                    item.refs?.includes("HEAD")
                      ? "border-primary/30 bg-primary/72 shadow-[0_1px_2px_rgba(15,23,42,0.14)]"
                      : "border-app-border bg-app-drawer",
                  )}
                />
                <div className="relative flex items-center justify-between gap-3 rounded-lg px-2.5 py-1.5">
                  <p className="min-w-0 flex-1 truncate text-[13px] font-medium leading-5 text-app-foreground">{item.subject}</p>
                  {item.refs?.length ? (
                    <div className="flex shrink-0 flex-wrap justify-end gap-1">
                      {item.refs.map((ref) => (
                        <span
                          key={ref}
                          className={cn(
                            "rounded-full px-2 py-1 text-[10px] transition-[background-color,color,box-shadow] duration-200",
                            ref === "HEAD"
                              ? "bg-primary/88 text-primary-foreground shadow-[0_1px_2px_rgba(15,23,42,0.14)]"
                              : "bg-app-surface-muted text-app-muted",
                          )}
                        >
                          {ref}
                        </span>
                      ))}
                    </div>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>
    </div>
  );
}

export function GitDiffPreviewPanel({
  file,
  isStaged,
  onClose,
}: {
  file: GitChangeFile;
  isStaged: boolean;
  onClose: () => void;
}) {
  const [isMetaExpanded, setMetaExpanded] = useState(false);
  const preview = buildGitDiffPreview(file);
  const splitRows = buildGitSplitDiffRows(file);
  const fileName = file.path.split("/").pop() ?? file.path;

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
                <ProjectTreeIcon icon={file.icon} />
              </span>
              <p className="truncate text-sm font-semibold text-app-foreground">{fileName}</p>
              <span className="shrink-0 rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
                {file.summary}
              </span>
              <span
                className={cn(
                  "shrink-0 rounded-md px-1.5 py-0.5 text-[11px]",
                  isStaged ? "bg-app-foreground text-app-drawer" : "bg-app-surface-muted text-app-subtle",
                )}
              >
                {isStaged ? "Staged" : "Unstaged"}
              </span>
            </div>
            <div className="mt-1 flex items-center gap-1">
              <p className="truncate text-[12px] text-app-subtle">{file.path}</p>
              <button
                type="button"
                aria-label={isMetaExpanded ? "折叠 diff 指令信息" : "展开 diff 指令信息"}
                title={isMetaExpanded ? "折叠 diff 指令信息" : "展开 diff 指令信息"}
                className="flex size-5 shrink-0 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                onClick={() => setMetaExpanded((current) => !current)}
              >
                <ChevronDown className={cn("size-3.5 transition-transform", !isMetaExpanded && "-rotate-90")} />
              </button>
            </div>
          </div>

          <button
            type="button"
            aria-label="关闭 Diff 预览"
            title="关闭 Diff 预览"
            className="flex size-8 shrink-0 items-center justify-center rounded-lg text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
            onClick={onClose}
          >
            <CircleX className="size-4" />
          </button>
        </div>

        {isMetaExpanded ? (
          <div className="shrink-0 border-b border-app-border bg-app-surface-muted/70 px-5 py-3 font-mono text-[11px] text-app-subtle">
            {preview.meta.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        ) : null}

        <div className="grid shrink-0 grid-cols-2 border-b border-app-border bg-app-surface-muted/50 text-[11px] uppercase tracking-[0.12em] text-app-subtle">
          <div className="border-r border-app-border px-4 py-2">Old</div>
          <div className="px-4 py-2">New</div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto overscroll-contain bg-app-drawer font-mono text-[12px] leading-6 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {splitRows.map((row, index) => (
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
                    row.kind === "remove" || row.kind === "modified" ? "text-app-danger" : "text-app-foreground",
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
                    row.kind === "add" || row.kind === "modified" ? "text-app-success" : "text-app-foreground",
                  )}
                >
                  {row.rightText}
                </span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
