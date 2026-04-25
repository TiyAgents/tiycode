import type { TranslationKey } from "@/i18n";
import type { RunModelPlanDto } from "@/shared/types/api";
import type { RefObject } from "react";
import {
  Boxes,
  Folder,
  FolderOpen,
  FolderPlus,
  LoaderCircle,
  MessageSquarePlus,
  MoreHorizontal,
  Shuffle,
  Trash2,
} from "lucide-react";

import {
  DRAWER_LIST_LABEL_CLASS,
  DRAWER_LIST_ROW_CLASS,
  DRAWER_LIST_STACK_CLASS,
  DRAWER_OVERFLOW_ACTION_CLASS,
} from "@/modules/workbench-shell/model/fixtures";
import { sortWorkspacesWithWorktrees } from "@/modules/workbench-shell/model/helpers";
import type { WorkspaceItem } from "@/modules/workbench-shell/model/types";
import { cn } from "@/shared/lib/utils";
import { ThreadRenameInput } from "@/modules/workbench-shell/ui/thread-rename-input";
import { ThreadStatusIndicator } from "@/modules/workbench-shell/ui/thread-status-indicator";

const WORKSPACE_THREAD_PAGE_SIZE = 10;

type WorkspaceAction = {
  workspaceId: string;
  kind: "open" | "remove";
} | null;

type DashboardSidebarProps = {
  isSidebarOpen: boolean;
  isNewThreadMode: boolean;
  isMarketplaceOpen: boolean;
  handleEnterNewThreadMode: () => void;
  handleOpenMarketplace: () => void;
  t: (key: TranslationKey) => string;
  handleChooseWorkspaceFolder: () => void;
  isAddingWorkspace: boolean;
  isSidebarReady: boolean;
  workspaces: WorkspaceItem[];
  openWorkspaces: Record<string, boolean>;
  activeWorkspaceMenuId: string | null;
  workspaceAction: WorkspaceAction;
  workspaceThreadDisplayCounts: Record<string, number>;
  workspaceThreadHasMore: Record<string, boolean>;
  workspaceThreadLoadMorePending: Record<string, boolean>;
  workspaceMenuRef: RefObject<HTMLDivElement | null>;
  handleWorkspaceToggle: (workspaceId: string) => void;
  handleWorkspaceMenuToggle: (workspaceId: string) => void;
  handleNewThreadForWorkspace: (workspace: WorkspaceItem) => void;
  setActiveWorkspaceMenuId: (workspaceId: string | null) => void;
  setWorktreeDialogContext: (context: {
    repo: { id: string; name: string; canonicalPath: string };
  } | null) => void;
  handleOpenWorkspaceInSystem: (workspace: WorkspaceItem) => void;
  canOpenWorkspaceInSystem: boolean;
  workspaceOpenLabel: string;
  handleWorkspaceRemove: (workspace: WorkspaceItem) => void;
  pendingDeleteThreadId: string | null;
  deletingThreadId: string | null;
  editingThreadId: string | null;
  commitMessageModelPlan: RunModelPlanDto | null;
  handleThreadEditDone: (
    threadId: string,
    newTitle: string | null,
    previousTitle: string,
  ) => void;
  handleThreadSelect: (threadId: string) => void;
  handleThreadEditStart: (threadId: string) => void;
  handleThreadDeleteConfirm: (threadId: string) => void;
  handleThreadDeleteRequest: (threadId: string) => void;
  handleWorkspaceShowMore: (workspaceId: string) => void;
};

export function DashboardSidebar(props: DashboardSidebarProps) {
  const {
    isSidebarOpen,
    isNewThreadMode,
    isMarketplaceOpen,
    handleEnterNewThreadMode,
    handleOpenMarketplace,
    t,
    handleChooseWorkspaceFolder,
    isAddingWorkspace,
    isSidebarReady,
    workspaces,
    openWorkspaces,
    activeWorkspaceMenuId,
    workspaceAction,
    workspaceThreadDisplayCounts,
    workspaceThreadHasMore,
    workspaceThreadLoadMorePending,
    workspaceMenuRef,
    handleWorkspaceToggle,
    handleWorkspaceMenuToggle,
    handleNewThreadForWorkspace,
    setActiveWorkspaceMenuId,
    setWorktreeDialogContext,
    handleOpenWorkspaceInSystem,
    canOpenWorkspaceInSystem,
    workspaceOpenLabel,
    handleWorkspaceRemove,
    pendingDeleteThreadId,
    deletingThreadId,
    editingThreadId,
    commitMessageModelPlan,
    handleThreadEditDone,
    handleThreadSelect,
    handleThreadEditStart,
    handleThreadDeleteConfirm,
    handleThreadDeleteRequest,
    handleWorkspaceShowMore,
  } = props;

  return (
            <aside
              className={cn(
                "overflow-hidden bg-app-sidebar transition-[width,opacity,transform] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
                isSidebarOpen
                  ? "w-[320px] border-r border-app-border opacity-100 translate-x-0"
                  : "w-0 border-r-0 opacity-0 -translate-x-2 pointer-events-none",
              )}
            >
              <div className="flex h-full min-h-0 flex-col px-3 pb-3 pt-4">
                <div className="space-y-1">
                  <button
                    type="button"
                    className={cn(
                      "group flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-[transform,box-shadow,background-color,border-color,color] duration-200 active:scale-[0.99]",
                      isNewThreadMode
                        ? "border-app-border-strong bg-app-surface-active text-app-foreground shadow-[0_4px_14px_rgba(15,23,42,0.08)]"
                        : "border-transparent bg-transparent text-app-muted hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)]",
                    )}
                    onClick={handleEnterNewThreadMode}
                  >
                    <MessageSquarePlus
                      className={cn(
                        "size-4 shrink-0 transition-colors duration-200",
                        isNewThreadMode
                          ? "text-app-foreground"
                          : "text-app-subtle group-hover:text-app-foreground",
                      )}
                    />
                    <span className="truncate text-sm font-medium">{t("sidebar.newThread")}</span>
                  </button>

                  <button
                    type="button"
                    className={cn(
                      "group flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-[transform,box-shadow,background-color,border-color,color] duration-200 active:scale-[0.99]",
                      isMarketplaceOpen
                        ? "border-app-border-strong bg-app-surface-active text-app-foreground shadow-[0_4px_14px_rgba(15,23,42,0.08)]"
                        : "border-transparent bg-transparent text-app-muted hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)]",
                    )}
                    onClick={handleOpenMarketplace}
                  >
                    <Boxes
                      className={cn(
                        "size-4 shrink-0 transition-colors duration-200",
                        isMarketplaceOpen
                          ? "text-app-foreground"
                          : "text-app-subtle group-hover:text-app-foreground",
                      )}
                    />
                    <span className="truncate text-sm font-medium">
                      {t("sidebar.extensions")}
                    </span>
                  </button>
                </div>

                <div className="mt-6 flex items-center justify-between px-3">
                  <span className="text-xs uppercase tracking-[0.14em] text-app-subtle">
                    {t("sidebar.workspace")}
                  </span>
                  <button
                    type="button"
                    aria-label={t("sidebar.addWorkspace")}
                    title={t("sidebar.addWorkspace")}
                    className="inline-flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground disabled:cursor-not-allowed disabled:opacity-60"
                    onClick={handleChooseWorkspaceFolder}
                    disabled={isAddingWorkspace}
                  >
                    {isAddingWorkspace ? (
                      <LoaderCircle className="size-3.5 animate-spin" />
                    ) : (
                      <FolderPlus className="size-3.5" />
                    )}
                  </button>
                </div>

                <div className="mx-1 mt-3 h-px shrink-0 bg-app-border" />

                <div className="mt-3 min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                  <div className="space-y-1.5">
                    {!isSidebarReady ? (
                      <div className="space-y-3 px-1">
                        {/* Workspace skeleton */}
                        <div className="space-y-2">
                          <div className="flex items-center gap-2 rounded-lg px-2 py-1.5">
                            <div className="size-4 animate-pulse rounded bg-app-surface-hover" />
                            <div className="h-3.5 w-28 animate-pulse rounded bg-app-surface-hover" />
                          </div>
                          {/* Thread skeletons */}
                          {[1, 2, 3].map((i) => (
                            <div key={i} className="flex items-center gap-2 rounded-lg px-2 py-1.5 pl-7">
                              <div className="size-3.5 animate-pulse rounded bg-app-surface-hover" />
                              <div
                                className="h-3 animate-pulse rounded bg-app-surface-hover"
                                style={{ width: `${60 + i * 12}%` }}
                              />
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : (
                    sortWorkspacesWithWorktrees(workspaces as WorkspaceItem[]).map((workspace) => {
                      const isWorktreeRow = workspace.kind === "worktree";
                      const isRepoRow = workspace.kind === "repo";
                      const worktreeTag =
                        workspace.worktreeHash && workspace.worktreeHash.length > 0
                          ? workspace.worktreeHash
                          : null;
                      const isOpen =
                        openWorkspaces[workspace.id] ?? workspace.defaultOpen;
                      const FolderIcon = isWorktreeRow
                        ? Shuffle
                        : isOpen
                          ? FolderOpen
                          : Folder;
                      const isWorkspaceMenuOpen =
                        activeWorkspaceMenuId === workspace.id;
                      const isOpeningWorkspace =
                        workspaceAction?.workspaceId === workspace.id &&
                        workspaceAction.kind === "open";
                      const isRemovingWorkspace =
                        workspaceAction?.workspaceId === workspace.id &&
                        workspaceAction.kind === "remove";
                      const visibleThreadCount =
                        workspaceThreadDisplayCounts[workspace.id] ??
                        WORKSPACE_THREAD_PAGE_SIZE;
                      const visibleThreads = workspace.threads.slice(
                        0,
                        visibleThreadCount,
                      );
                      const hasMoreThreads =
                        (workspaceThreadHasMore[workspace.id] ?? false) ||
                        workspace.threads.length > visibleThreadCount;
                      const isLoadingMoreThreads =
                        workspaceThreadLoadMorePending[workspace.id] ?? false;

                      return (
                        <div key={workspace.id} className="space-y-1">
                          <div className="group px-1">
                            <div
                              ref={
                                isWorkspaceMenuOpen ? workspaceMenuRef : undefined
                              }
                              className="relative"
                            >
                              <button
                                type="button"
                                className={cn(
                                  "flex items-center gap-2 pr-10 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                  DRAWER_LIST_ROW_CLASS,
                                )}
                                onClick={() => handleWorkspaceToggle(workspace.id)}
                              >
                                <FolderIcon className="size-4 shrink-0 text-app-muted" />
                                <div className="flex min-w-0 flex-1 items-center gap-1.5">
                                  <span className="truncate text-[13px] leading-5">
                                    {workspace.name}
                                  </span>
                                  {worktreeTag ? (
                                    <span
                                      title={t("worktree.tag.label")}
                                      className="shrink-0 rounded bg-app-surface-hover px-1.5 py-0.5 font-mono text-[10px] text-app-subtle"
                                    >
                                      {worktreeTag}
                                    </span>
                                  ) : null}
                                </div>
                              </button>
                              <button
                                type="button"
                                aria-label={t("dashboard.moreActions")}
                                title={t("dashboard.moreActions")}
                                aria-haspopup="menu"
                                aria-expanded={isWorkspaceMenuOpen}
                                className={cn(
                                  DRAWER_OVERFLOW_ACTION_CLASS,
                                  isWorkspaceMenuOpen &&
                                    "opacity-100 text-app-foreground",
                                )}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  handleWorkspaceMenuToggle(workspace.id);
                                }}
                              >
                                <MoreHorizontal className="size-4" />
                              </button>

                              {isWorkspaceMenuOpen ? (
                                <div className="absolute right-0 top-[calc(100%+0.35rem)] z-20 min-w-[11rem] overflow-hidden rounded-xl border border-app-border bg-app-menu/98 p-1 shadow-[0_18px_40px_-26px_rgba(15,23,42,0.38)] backdrop-blur-xl dark:bg-app-menu/94">
                                  <button
                                    type="button"
                                    role="menuitem"
                                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm text-app-foreground transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:text-app-subtle"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      handleNewThreadForWorkspace(workspace);
                                    }}
                                    disabled={
                                      !workspace.path ||
                                      isOpeningWorkspace ||
                                      isRemovingWorkspace
                                    }
                                  >
                                    <MessageSquarePlus className="size-4 shrink-0" />
                                    <span>{t("sidebar.newThreadForWorkspace")}</span>
                                  </button>
                                  {isRepoRow ? (
                                    <button
                                      type="button"
                                      role="menuitem"
                                      className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm text-app-foreground transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:text-app-subtle"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setActiveWorkspaceMenuId(null);
                                        if (workspace.path) {
                                          setWorktreeDialogContext({
                                            repo: {
                                              id: workspace.id,
                                              name: workspace.name,
                                              canonicalPath: workspace.path,
                                            },
                                          });
                                        }
                                      }}
                                      disabled={!workspace.path}
                                    >
                                      <Shuffle className="size-4 shrink-0" />
                                      <span>{t("worktree.menu.newWorktree")}</span>
                                    </button>
                                  ) : null}
                                  <button
                                    type="button"
                                    role="menuitem"
                                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm text-app-foreground transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:text-app-subtle"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      handleOpenWorkspaceInSystem(workspace);
                                    }}
                                    disabled={
                                      !canOpenWorkspaceInSystem ||
                                      !workspace.path ||
                                      isOpeningWorkspace ||
                                      isRemovingWorkspace
                                    }
                                  >
                                    {isOpeningWorkspace ? (
                                      <LoaderCircle className="size-4 shrink-0 animate-spin" />
                                    ) : (
                                      <FolderOpen className="size-4 shrink-0" />
                                    )}
                                    <span>{workspaceOpenLabel}</span>
                                  </button>
                                  <button
                                    type="button"
                                    role="menuitem"
                                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-sm text-app-danger transition-colors hover:bg-app-danger/10 disabled:cursor-not-allowed disabled:opacity-60"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      handleWorkspaceRemove(workspace);
                                    }}
                                    disabled={
                                      isOpeningWorkspace || isRemovingWorkspace
                                    }
                                  >
                                    {isRemovingWorkspace ? (
                                      <LoaderCircle className="size-4 shrink-0 animate-spin" />
                                    ) : (
                                      <Trash2 className="size-4 shrink-0" />
                                    )}
                                    <span>{t("sidebar.remove")}</span>
                                  </button>
                                </div>
                              ) : null}
                            </div>
                          </div>

                          {isOpen && visibleThreads.length > 0 ? (
                            <div className={cn(DRAWER_LIST_STACK_CLASS, "pl-2.5")}>
                              {visibleThreads.map((thread) => {
                                const isDeletePending =
                                  pendingDeleteThreadId === thread.id;
                                const isDeleting = deletingThreadId === thread.id;
                                const isEditing = editingThreadId === thread.id;

                                return (
                                  <div key={thread.id} className="group relative">
                                    {isEditing ? (
                                      <ThreadRenameInput
                                        threadId={thread.id}
                                        initialName={thread.name}
                                        isActive={thread.active}
                                        status={thread.status}
                                        modelPlan={commitMessageModelPlan}
                                        onDone={(newTitle) =>
                                          handleThreadEditDone(
                                            thread.id,
                                            newTitle,
                                            thread.name,
                                          )
                                        }
                                      />
                                    ) : (
                                    <button
                                      type="button"
                                      className={cn(
                                        `${DRAWER_LIST_ROW_CLASS} border pr-[4.5rem]`,
                                        thread.active
                                          ? "border-app-border-strong bg-app-surface-active text-app-foreground"
                                          : "border-transparent bg-transparent text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                      )}
                                      onClick={() => handleThreadSelect(thread.id)}
                                      onDoubleClick={(e) => {
                                        e.stopPropagation();
                                        handleThreadEditStart(thread.id);
                                      }}
                                    >
                                      <div className="flex items-center gap-2">
                                        <ThreadStatusIndicator
                                          status={thread.status}
                                          emphasis={
                                            thread.active ? "default" : "subtle"
                                          }
                                        />
                                        <p className={DRAWER_LIST_LABEL_CLASS}>
                                          {thread.name}
                                        </p>
                                      </div>
                                    </button>
                                    )}
                                    {isEditing ? null : (
                                    <>
                                    <span
                                      className={cn(
                                        "pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-[11px] text-app-subtle transition-opacity duration-200",
                                        isDeletePending || isDeleting
                                          ? "opacity-0"
                                          : "group-hover:opacity-0",
                                      )}
                                    >
                                      {thread.time}
                                    </span>
                                    {isDeletePending || isDeleting ? (
                                      <button
                                        type="button"
                                        aria-label={
                                          isDeleting
                                            ? t("dashboard.deletingThread")
                                            : t("dashboard.confirmDeleteThread")
                                        }
                                        title={isDeleting ? t("sidebar.deleting") : t("sidebar.delete")}
                                        className="absolute right-1.5 top-1/2 inline-flex h-7 -translate-y-1/2 items-center justify-center rounded-md border border-app-danger/20 bg-app-danger/10 px-2 text-[11px] font-medium text-app-danger transition-colors hover:border-app-danger/30 hover:bg-app-danger/14 disabled:cursor-not-allowed disabled:opacity-80"
                                        onClick={(event) => {
                                          event.stopPropagation();
                                          handleThreadDeleteConfirm(thread.id);
                                        }}
                                        disabled={isDeleting}
                                      >
                                        {isDeleting ? (
                                          <LoaderCircle className="size-3.5 animate-spin" />
                                        ) : (
                                          t("sidebar.delete")
                                        )}
                                      </button>
                                    ) : (
                                      <button
                                        type="button"
                                        aria-label={t("dashboard.deleteThread")}
                                        title="Delete thread"
                                        className="absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-danger opacity-0 transition-all duration-200 hover:bg-app-danger/10 hover:text-app-danger group-hover:opacity-100"
                                        onClick={(event) => {
                                          event.stopPropagation();
                                          handleThreadDeleteRequest(thread.id);
                                        }}
                                      >
                                        <Trash2 className="size-4" />
                                      </button>
                                    )}
                                    </>
                                    )}
                                  </div>
                                );
                              })}
                              {hasMoreThreads ? (
                                <button
                                  type="button"
                                  className={cn(
                                    `${DRAWER_LIST_ROW_CLASS} flex items-center justify-end gap-2 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground`,
                                    isLoadingMoreThreads && "cursor-wait",
                                  )}
                                  onClick={() =>
                                    handleWorkspaceShowMore(workspace.id)
                                  }
                                  disabled={isLoadingMoreThreads}
                                >
                                  <span>{t("sidebar.showMore")}</span>
                                  {isLoadingMoreThreads ? (
                                    <LoaderCircle className="size-3.5 animate-spin" />
                                  ) : null}
                                </button>
                              ) : null}
                            </div>
                          ) : null}
                        </div>
                      );
                    })
                    )}
                  </div>
                </div>
              </div>
            </aside>  );
}
