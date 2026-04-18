import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  GitBranch,
  LoaderCircle,
  Plus,
  Search,
  Sparkles,
} from "lucide-react";

import { useT } from "@/i18n";
import {
  gitCheckoutBranch,
  gitCreateBranch,
  gitGenerateBranchName,
  gitListBranches,
} from "@/services/bridge/git-commands";
import type { GitBranchDto, GitMutationResponseDto, GitSnapshotDto, RunModelPlanDto } from "@/shared/types/api";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { cn } from "@/shared/lib/utils";

function mostFrequent(counts: Record<string, number>, fallback: string): string {
  let best = fallback;
  let max = 0;
  for (const [key, count] of Object.entries(counts)) {
    if (count > max) {
      max = count;
      best = key;
    }
  }
  return best;
}

function sanitizeBranchSegment(value: string): string {
  return value
    .replace(/[^a-zA-Z0-9-]/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "")
    .toLowerCase() || "update";
}

export function BranchSelector({
  workspaceId,
  snapshot,
  modelPlan,
  readOnly,
}: {
  workspaceId: string | null;
  snapshot: Pick<GitSnapshotDto, "headRef" | "isDetached" | "stagedFiles" | "unstagedFiles" | "untrackedFiles"> | null;
  modelPlan: RunModelPlanDto | null;
  readOnly?: boolean;
}) {
  const t = useT();
  const [isOpen, setOpen] = useState(false);
  const [branches, setBranches] = useState<GitBranchDto[]>([]);
  const [isLoading, setLoading] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [switchingBranch, setSwitchingBranch] = useState<string | null>(null);
  const [approvalPending, setApprovalPending] = useState<{
    action: "checkout" | "create";
    branch: string;
    reason: string;
  } | null>(null);

  // Create branch mode
  const [isCreateMode, setCreateMode] = useState(false);
  const [newBranchName, setNewBranchName] = useState("");
  const [isCreating, setCreating] = useState(false);
  const [isGeneratingName, setGeneratingName] = useState(false);
  const [hasGeneratedSuggestion, setHasGeneratedSuggestion] = useState(false);

  // Remote branches visibility
  const [showRemoteBranches, setShowRemoteBranches] = useState(false);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const newBranchInputRef = useRef<HTMLInputElement | null>(null);

  const currentBranch = snapshot?.isDetached
    ? null
    : (snapshot?.headRef ?? null);

  const branchLabel = snapshot?.isDetached
    ? t("sourceControl.branch.detachedHead")
    : (snapshot?.headRef ?? t("sourceControl.noBranch"));

  const localBranches = useMemo(
    () => branches.filter((b) => !b.isRemote),
    [branches],
  );
  const remoteBranches = useMemo(
    () => branches.filter((b) => b.isRemote),
    [branches],
  );

  const filteredLocal = useMemo(() => {
    if (!searchQuery) return localBranches;
    const query = searchQuery.toLowerCase();
    return localBranches.filter((b) => b.name.toLowerCase().includes(query));
  }, [localBranches, searchQuery]);

  const filteredRemote = useMemo(() => {
    if (!searchQuery) return remoteBranches;
    const query = searchQuery.toLowerCase();
    return remoteBranches.filter((b) => b.name.toLowerCase().includes(query));
  }, [remoteBranches, searchQuery]);

  const loadBranches = useCallback(async () => {
    if (!workspaceId) return;
    setLoading(true);
    setError(null);
    try {
      const result = await gitListBranches(workspaceId);
      setBranches(result);
    } catch (err) {
      setError(getInvokeErrorMessage(err, t("sourceControl.branch.loading")));
    } finally {
      setLoading(false);
    }
  }, [workspaceId, t]);

  useEffect(() => {
    if (isOpen) {
      void loadBranches();
      requestAnimationFrame(() => {
        searchInputRef.current?.focus();
      });
    } else {
      setSearchQuery("");
      setCreateMode(false);
      setNewBranchName("");
      setHasGeneratedSuggestion(false);
      setGeneratingName(false);
      setError(null);
      setApprovalPending(null);
      setShowRemoteBranches(false);
    }
  }, [isOpen, loadBranches]);

  // Close on click outside
  useEffect(() => {
    if (!isOpen || typeof window === "undefined") return;

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (target && containerRef.current?.contains(target)) return;
      setOpen(false);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isOpen]);

  // Close on Escape
  useEffect(() => {
    if (!isOpen || typeof window === "undefined") return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (isCreateMode) {
          setCreateMode(false);
          setNewBranchName("");
          setHasGeneratedSuggestion(false);
        } else {
          setOpen(false);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, isCreateMode]);

  const handleMutationResponse = useCallback(
    (
      response: GitMutationResponseDto,
      action: "checkout" | "create",
      branch: string,
    ) => {
      if (response.type === "approval_required") {
        setApprovalPending({
          action,
          branch,
          reason: response.reason,
        });
        return false;
      }
      return true;
    },
    [],
  );

  const handleCheckout = useCallback(
    async (branchName: string, approved?: boolean) => {
      if (!workspaceId) return;
      setSwitchingBranch(branchName);
      setError(null);
      try {
        const response = await gitCheckoutBranch(
          workspaceId,
          branchName,
          approved,
        );
        if (handleMutationResponse(response, "checkout", branchName)) {
          setOpen(false);
        }
      } catch (err) {
        setError(getInvokeErrorMessage(err, t("sourceControl.branch.checkoutFailed")));
      } finally {
        setSwitchingBranch(null);
      }
    },
    [workspaceId, handleMutationResponse, t],
  );

  const doCreateBranch = useCallback(
    async (branchToCreate: string, approved?: boolean) => {
      if (!workspaceId || !branchToCreate.trim()) return;
      setCreating(true);
      setError(null);
      try {
        const response = await gitCreateBranch(
          workspaceId,
          branchToCreate.trim(),
          approved,
        );
        if (handleMutationResponse(response, "create", branchToCreate.trim())) {
          setOpen(false);
        }
      } catch (err) {
        setError(getInvokeErrorMessage(err, t("sourceControl.branch.createFailed")));
      } finally {
        setCreating(false);
      }
    },
    [workspaceId, handleMutationResponse, t],
  );

  const handleApprovalConfirm = useCallback(async () => {
    if (!approvalPending) return;
    const { action, branch } = approvalPending;
    setApprovalPending(null);
    if (action === "checkout") {
      await handleCheckout(branch, true);
    } else {
      await doCreateBranch(branch, true);
    }
  }, [approvalPending, handleCheckout, doCreateBranch]);

  const generateStrategyRef = useRef(0);

  // Quick local heuristic for instant fallback
  const generateLocalFallbackName = useCallback((): string => {
    const changedFiles = snapshot
      ? [...snapshot.stagedFiles, ...snapshot.unstagedFiles, ...snapshot.untrackedFiles]
      : [];

    const prefixCounts: Record<string, number> = {};
    for (const branch of localBranches) {
      const slashIndex = branch.name.indexOf("/");
      if (slashIndex > 0) {
        const prefix = branch.name.substring(0, slashIndex);
        prefixCounts[prefix] = (prefixCounts[prefix] ?? 0) + 1;
      }
    }
    const conventionPrefix = mostFrequent(prefixCounts, "");
    let defaultPrefix: string;
    if (conventionPrefix) {
      defaultPrefix = conventionPrefix;
    } else {
      const hasOnlyModifications =
        changedFiles.length > 0 && changedFiles.every((f) => f.status === "modified");
      defaultPrefix = hasOnlyModifications ? "fix" : "feat";
    }

    const candidates: string[] = [];
    if (changedFiles.length > 0) {
      const folders = changedFiles.slice(0, 8).map((f) => {
        const parts = f.path.split("/");
        return parts.length > 1 ? parts[parts.length - 2] : null;
      }).filter(Boolean) as string[];
      if (folders.length > 0) {
        const folderCounts: Record<string, number> = {};
        for (const folder of folders) folderCounts[folder] = (folderCounts[folder] ?? 0) + 1;
        candidates.push(`${defaultPrefix}/${sanitizeBranchSegment(mostFrequent(folderCounts, "update"))}`);
      }
      const firstFile = changedFiles[0];
      if (firstFile) {
        const fileName = firstFile.path.split("/").pop()?.split(".")[0] ?? "";
        if (fileName) candidates.push(`${defaultPrefix}/${sanitizeBranchSegment(fileName)}`);
      }
    }
    if (candidates.length === 0) {
      const timestamp = new Date().toISOString().slice(5, 10).replace("-", "");
      candidates.push(`${defaultPrefix}/update-${timestamp}`);
    }

    const unique = [...new Set(candidates)];
    const index = generateStrategyRef.current % unique.length;
    generateStrategyRef.current += 1;
    return unique[index] ?? `${defaultPrefix}/update`;
  }, [snapshot, localBranches]);

  // AI-powered branch name generation
  const generateSmartBranchName = useCallback(() => {
    // Set instant local fallback first
    const fallback = generateLocalFallbackName();
    setNewBranchName(fallback);
    setHasGeneratedSuggestion(true);

    // Then call AI if model is available
    if (!workspaceId || !modelPlan) return;

    setGeneratingName(true);
    void gitGenerateBranchName(workspaceId, modelPlan)
      .then((name) => {
        setNewBranchName(name);
      })
      .catch(() => {
        // AI failed — keep the local fallback, no error needed
      })
      .finally(() => {
        setGeneratingName(false);
      });
  }, [workspaceId, modelPlan, generateLocalFallbackName]);

  const handleEnterCreateMode = useCallback(() => {
    setCreateMode(true);
    setError(null);
    generateSmartBranchName();
    requestAnimationFrame(() => {
      newBranchInputRef.current?.focus();
      newBranchInputRef.current?.select();
    });
  }, [generateSmartBranchName]);

  if (!workspaceId || !snapshot) {
    return (
      <span className="inline-flex items-center gap-1.5 text-xs text-app-subtle">
        <GitBranch className="size-3.5" />
        <span>{t("sourceControl.noBranch")}</span>
      </span>
    );
  }

  if (readOnly) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-app-subtle">
        <GitBranch className="size-3.5 shrink-0" />
        <span className="max-w-[120px] truncate">{branchLabel}</span>
      </span>
    );
  }

  return (
    <div className="relative" ref={containerRef}>
      <button
        type="button"
        className={cn(
          "inline-flex items-center gap-1.5 rounded-md px-1.5 py-1 text-xs transition-colors",
          isOpen
            ? "bg-app-surface-hover text-app-foreground"
            : "text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground",
        )}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={isOpen}
        aria-haspopup="listbox"
        title={branchLabel}
      >
        <GitBranch className="size-3.5 shrink-0" />
        <span className="max-w-[120px] truncate">{branchLabel}</span>
        <ChevronDown className={cn("size-3.5 shrink-0 transition-transform duration-200", isOpen && "rotate-180")} />
      </button>

      {isOpen ? (
        <div className="absolute right-0 top-[calc(100%+0.35rem)] z-30 w-[280px] overflow-hidden rounded-xl border border-app-border bg-app-menu/98 shadow-[0_20px_48px_rgba(15,23,42,0.18)] backdrop-blur-xl dark:bg-app-menu/94 dark:shadow-[0_20px_48px_rgba(0,0,0,0.42)]">
          {approvalPending ? (
            <div className="p-3">
              <p className="mb-2 text-sm text-app-foreground">
                {approvalPending.reason}
              </p>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  className="flex-1 rounded-lg border border-app-border bg-app-surface-muted px-3 py-1.5 text-xs font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
                  onClick={() => setApprovalPending(null)}
                >
                  {t("sourceControl.branch.cancel")}
                </button>
                <button
                  type="button"
                  className="flex-1 rounded-lg bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90"
                  onClick={() => void handleApprovalConfirm()}
                >
                  {t("sourceControl.branch.confirm")}
                </button>
              </div>
            </div>
          ) : isCreateMode ? (
            <div className="p-2">
              <div className="mb-2 flex items-center gap-2 px-1">
                <Plus className="size-3.5 shrink-0 text-app-subtle" />
                <span className="text-xs font-medium text-app-foreground">
                  {t("sourceControl.branch.createBranch")}
                </span>
              </div>
              <div className="relative">
                <input
                  ref={newBranchInputRef}
                  type="text"
                  className="w-full rounded-lg border border-app-border bg-app-surface-muted px-3 py-2 text-xs text-app-foreground outline-none placeholder:text-app-subtle focus:border-primary/60 focus:ring-1 focus:ring-primary/30"
                  placeholder={t("sourceControl.branch.newBranchName")}
                  value={newBranchName}
                  onChange={(e) => setNewBranchName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && newBranchName.trim()) {
                      void doCreateBranch(newBranchName);
                    }
                  }}
                />
                {hasGeneratedSuggestion ? (
                  <button
                    type="button"
                    className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-app-subtle transition-colors hover:text-primary active:scale-90 disabled:pointer-events-none"
                    title={t("sourceControl.branch.smartCreate")}
                    disabled={isGeneratingName}
                    onClick={() => {
                      generateSmartBranchName();
                      newBranchInputRef.current?.focus();
                    }}
                  >
                    {isGeneratingName ? (
                      <LoaderCircle className="size-3.5 animate-spin" />
                    ) : (
                      <Sparkles className="size-3.5" />
                    )}
                  </button>
                ) : null}
              </div>
              {error ? (
                <p className="mt-1.5 px-1 text-[11px] text-app-danger">{error}</p>
              ) : null}
              <div className="mt-2 flex items-center gap-2">
                <button
                  type="button"
                  className="flex-1 rounded-lg border border-app-border bg-app-surface-muted px-3 py-1.5 text-xs font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => {
                    setCreateMode(false);
                    setNewBranchName("");
                    setHasGeneratedSuggestion(false);
                    setError(null);
                  }}
                >
                  {t("sourceControl.branch.cancel")}
                </button>
                <button
                  type="button"
                  className="flex-1 rounded-lg bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={!newBranchName.trim() || isCreating}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => void doCreateBranch(newBranchName)}
                >
                  {isCreating ? (
                    <LoaderCircle className="mx-auto size-3.5 animate-spin" />
                  ) : (
                    t("sourceControl.branch.create")
                  )}
                </button>
              </div>
            </div>
          ) : (
            <>
              {/* Search bar */}
              <div className="border-b border-app-border px-2 py-1.5">
                <div className="relative">
                  <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-app-subtle" />
                  <input
                    ref={searchInputRef}
                    type="text"
                    className="w-full rounded-md border-none bg-transparent py-1.5 pl-7 pr-2 text-xs text-app-foreground outline-none placeholder:text-app-subtle"
                    placeholder={t("sourceControl.branch.search")}
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                  />
                </div>
              </div>

              {/* Branch list */}
              <div className="max-h-[280px] overflow-auto overscroll-contain p-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                {isLoading ? (
                  <div className="flex items-center justify-center gap-2 py-6 text-xs text-app-subtle">
                    <LoaderCircle className="size-3.5 animate-spin" />
                    <span>{t("sourceControl.branch.loading")}</span>
                  </div>
                ) : error && branches.length === 0 ? (
                  <div className="px-3 py-4 text-center text-xs text-app-danger">
                    {error}
                  </div>
                ) : (
                  <>
                    {/* Local branches */}
                    {filteredLocal.length > 0 ? (
                      <div>
                        <div className="px-2 pb-1 pt-1.5 text-[10px] font-semibold uppercase tracking-wider text-app-subtle">
                          {t("sourceControl.branch.localBranches")}
                        </div>
                        {filteredLocal.map((branch) => {
                          const isCurrent = branch.name === currentBranch;
                          const isSwitching = switchingBranch === branch.name;

                          return (
                            <button
                              key={branch.name}
                              type="button"
                              className={cn(
                                "flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs transition-colors",
                                isCurrent
                                  ? "bg-app-surface-hover/80 text-app-foreground"
                                  : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                isSwitching && "cursor-wait opacity-70",
                              )}
                              disabled={isCurrent || !!switchingBranch}
                              onClick={() => void handleCheckout(branch.name)}
                            >
                              <GitBranch className="size-3.5 shrink-0 text-app-subtle" />
                              <span className="min-w-0 flex-1 truncate">{branch.name}</span>
                              {isSwitching ? (
                                <LoaderCircle className="size-3 shrink-0 animate-spin text-app-subtle" />
                              ) : isCurrent ? (
                                <Check className="size-3.5 shrink-0 text-primary" />
                              ) : null}
                            </button>
                          );
                        })}
                      </div>
                    ) : null}

                    {/* Remote branches (collapsible) */}
                    {filteredRemote.length > 0 ? (
                      <div className="mt-1">
                        <button
                          type="button"
                          className="flex w-full items-center gap-1 px-2 pb-1 pt-1.5 text-[10px] font-semibold uppercase tracking-wider text-app-subtle hover:text-app-foreground"
                          onClick={() => setShowRemoteBranches((s) => !s)}
                        >
                          <ChevronRight
                            className={cn(
                              "size-3 transition-transform duration-150",
                              showRemoteBranches && "rotate-90",
                            )}
                          />
                          <span>
                            {t("sourceControl.branch.remoteBranches")} ({filteredRemote.length})
                          </span>
                        </button>
                        {showRemoteBranches
                          ? filteredRemote.map((branch) => {
                              const isSwitching = switchingBranch === branch.name;

                              return (
                                <button
                                  key={branch.name}
                                  type="button"
                                  className={cn(
                                    "flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-app-muted transition-colors hover:bg-app-surface-hover hover:text-app-foreground",
                                    isSwitching && "cursor-wait opacity-70",
                                  )}
                                  disabled={!!switchingBranch}
                                  onClick={() => void handleCheckout(branch.name)}
                                >
                                  <GitBranch className="size-3.5 shrink-0 text-app-subtle" />
                                  <span className="min-w-0 flex-1 truncate">{branch.name}</span>
                                  {isSwitching ? (
                                    <LoaderCircle className="size-3 shrink-0 animate-spin text-app-subtle" />
                                  ) : null}
                                </button>
                              );
                            })
                          : null}
                      </div>
                    ) : null}

                    {filteredLocal.length === 0 && filteredRemote.length === 0 && searchQuery ? (
                      <div className="px-3 py-4 text-center text-xs text-app-subtle">
                        {t("sourceControl.branch.noResults")}
                      </div>
                    ) : null}

                    {error && branches.length > 0 ? (
                      <p className="px-2 py-1 text-[11px] text-app-danger">{error}</p>
                    ) : null}
                  </>
                )}
              </div>

              {/* Smart create branch */}
              <div className="border-t border-app-border p-1">
                <button
                  type="button"
                  className="flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left text-xs text-app-muted transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                  onClick={handleEnterCreateMode}
                >
                  <Sparkles className="size-3.5 shrink-0 text-primary/70" />
                  <span>{t("sourceControl.branch.smartCreate")}</span>
                </button>
              </div>
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}
