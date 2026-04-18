import { useCallback, useEffect, useMemo, useState } from "react";
import { GitBranch, LoaderCircle, Plus } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { useT } from "@/i18n";
import { cn } from "@/shared/lib/utils";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import {
  gitListBranches,
  workspaceCreateWorktree,
} from "@/services/bridge";
import type {
  GitBranchDto,
  WorkspaceDto,
  WorktreeCreateInput,
} from "@/shared/types/api";

export type NewWorktreeDialogContext = {
  /** The repo workspace this worktree will belong to. */
  repo: Pick<WorkspaceDto, "id" | "name" | "canonicalPath">;
};

type Tab = "existing" | "new";

export function NewWorktreeDialog({
  context,
  onClose,
  onCreated,
}: {
  context: NewWorktreeDialogContext | null;
  onClose: () => void;
  onCreated?: (workspace: WorkspaceDto) => void;
}) {
  const t = useT();
  const isOpen = context !== null;

  const [tab, setTab] = useState<Tab>("new");
  const [branch, setBranch] = useState("");
  const [baseRef, setBaseRef] = useState("");
  const [path, setPath] = useState("");
  const [pathTouched, setPathTouched] = useState(false);
  const [branches, setBranches] = useState<GitBranchDto[]>([]);
  const [branchesLoading, setBranchesLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset state whenever the dialog opens for a new repo.
  useEffect(() => {
    if (!isOpen) return;
    setTab("new");
    setBranch("");
    setBaseRef("");
    setPath("");
    setPathTouched(false);
    setError(null);
  }, [isOpen, context?.repo.id]);

  // Fetch branches lazily when the dialog opens.
  useEffect(() => {
    if (!isOpen || !context) return;
    let cancelled = false;
    setBranchesLoading(true);
    gitListBranches(context.repo.id)
      .then((list) => {
        if (!cancelled) setBranches(list);
      })
      .catch(() => {
        if (!cancelled) setBranches([]);
      })
      .finally(() => {
        if (!cancelled) setBranchesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [isOpen, context]);

  // When left empty, the backend auto-generates the path under
  // `~/.tiy/workspace/<hash>/<repo-name>`. We intentionally do NOT auto-fill
  // the input so users see the placeholder and trust the default.
  const defaultPathHint = useMemo(() => {
    if (!context) return "";
    return `~/.tiy/workspace/<hash>/${context.repo.name}`;
  }, [context]);

  useEffect(() => {
    if (!isOpen) return;
    // Reset path back to empty whenever the dialog opens for a new repo;
    // backend will auto-generate on submit.
    if (!pathTouched) {
      setPath("");
    }
  }, [isOpen, pathTouched]);

  const canSubmit = Boolean(context && branch.trim() && !submitting);

  const handleSubmit = useCallback(async () => {
    if (!context) return;
    const trimmed = branch.trim();
    if (!trimmed) {
      setError(t("worktree.error.branchRequired"));
      return;
    }
    setError(null);
    setSubmitting(true);
    try {
      const input: WorktreeCreateInput = {
        branch: trimmed,
        createBranch: tab === "new",
        baseRef: tab === "new" && baseRef.trim() ? baseRef.trim() : undefined,
        path: path.trim() || undefined,
      };
      const created = await workspaceCreateWorktree(context.repo.id, input);
      onCreated?.(created);
      onClose();
    } catch (e) {
      setError(getInvokeErrorMessage(e, "Failed to create worktree"));
    } finally {
      setSubmitting(false);
    }
  }, [context, branch, baseRef, path, tab, t, onCreated, onClose]);

  const handleBrowsePath = useCallback(async () => {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: path || undefined,
    });
    if (typeof selected === "string" && selected) {
      setPath(selected);
      setPathTouched(true);
    }
  }, [path]);

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <GitBranch className="h-4 w-4" />
            {t("worktree.createTitle")}
          </DialogTitle>
          <DialogDescription>{t("worktree.createDescription")}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex gap-1 rounded-md bg-muted p-1 text-sm">
            <button
              type="button"
              onClick={() => setTab("new")}
              className={cn(
                "flex-1 rounded px-3 py-1.5 transition-colors",
                tab === "new"
                  ? "bg-background shadow-sm font-medium"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Plus className="mr-1 inline h-3.5 w-3.5" />
              {t("worktree.tab.newBranch")}
            </button>
            <button
              type="button"
              onClick={() => setTab("existing")}
              className={cn(
                "flex-1 rounded px-3 py-1.5 transition-colors",
                tab === "existing"
                  ? "bg-background shadow-sm font-medium"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <GitBranch className="mr-1 inline h-3.5 w-3.5" />
              {t("worktree.tab.existingBranch")}
            </button>
          </div>

          <div className="flex flex-col gap-1.5">
            <label className="text-sm font-medium">{t("worktree.field.branch")}</label>
            {tab === "existing" ? (
              <div className="flex max-h-48 flex-col gap-1 overflow-y-auto rounded border bg-muted/20 p-1">
                {branchesLoading ? (
                  <div className="flex items-center gap-2 px-2 py-4 text-sm text-muted-foreground">
                    <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
                    Loading…
                  </div>
                ) : branches.length === 0 ? (
                  <div className="px-2 py-4 text-sm text-muted-foreground">
                    {t("worktree.empty.branches")}
                  </div>
                ) : (
                  branches.map((b) => {
                    const isActive = branch === b.name;
                    const label = b.isRemote ? "remote" : "local";
                    return (
                      <button
                        key={`${label}:${b.name}`}
                        type="button"
                        onClick={() => setBranch(b.name)}
                        className={cn(
                          "flex items-center justify-between rounded px-2 py-1 text-left text-sm hover:bg-background",
                          isActive && "bg-background font-medium",
                        )}
                      >
                        <span className="truncate">{b.name}</span>
                        <span className="ml-2 text-[10px] uppercase tracking-wider text-muted-foreground">
                          {label}
                        </span>
                      </button>
                    );
                  })
                )}
              </div>
            ) : (
              <Input
                value={branch}
                onChange={(e) => setBranch(e.target.value)}
                placeholder={t("worktree.field.branchPlaceholder")}
                autoFocus
              />
            )}
            {tab === "existing" && branch && (
              <div className="text-xs text-muted-foreground">{branch}</div>
            )}
          </div>

          {tab === "new" ? (
            <div className="flex flex-col gap-1.5">
              <label className="text-sm font-medium">{t("worktree.field.baseRef")}</label>
              <Input
                value={baseRef}
                onChange={(e) => setBaseRef(e.target.value)}
                placeholder={t("worktree.field.baseRefPlaceholder")}
              />
            </div>
          ) : null}

          <div className="flex flex-col gap-1.5">
            <label className="text-sm font-medium">{t("worktree.field.path")}</label>
            <div className="flex gap-2">
              <Input
                value={path}
                onChange={(e) => {
                  setPath(e.target.value);
                  setPathTouched(true);
                }}
                placeholder={defaultPathHint}
                className="flex-1 font-mono text-xs"
              />
              <Button type="button" variant="outline" onClick={handleBrowsePath}>
                {t("worktree.field.pathBrowse")}
              </Button>
            </div>
            <div className="text-xs text-muted-foreground">
              {t("worktree.field.pathHint")}
            </div>
          </div>

          {error ? (
            <div className="rounded border border-destructive/50 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          ) : null}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={submitting}>
            {t("worktree.cancel")}
          </Button>
          <Button onClick={handleSubmit} disabled={!canSubmit}>
            {submitting ? (
              <>
                <LoaderCircle className="mr-2 h-3.5 w-3.5 animate-spin" />
                {t("worktree.submitting")}
              </>
            ) : (
              t("worktree.submit")
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
