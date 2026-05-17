import { useState, useEffect, type FC } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Button } from "@/shared/ui/button";
import {
  FilePlus,
  FolderPlus,
  Pencil,
  Trash2,
  Copy,
  ClipboardCopy,
  ExternalLink,
  EllipsisVertical,
} from "lucide-react";
import { fileCreate, fileDelete, fileRename } from "@/services/bridge/file-commands";
import { cn } from "@/shared/lib/utils";
import { useT } from "@/i18n";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface FileContextMenuProps {
  /** Relative path of the node (empty string for root). */
  nodePath: string;
  isDir: boolean;
  isRoot?: boolean;
  workspaceId: string;
  /** Workspace root absolute path, used to build absolute paths for "Copy Path". */
  workspaceRoot?: string;
  /** Called after a CRUD operation to refresh the tree. */
  onTreeRefresh: () => void;
  /** Copy relative path to clipboard. */
  onCopyPath?: (path: string) => void;
  /** Open in external app. */
  onOpenExternal?: (path: string) => void;
  /** Additional CSS class for the trigger button. */
  className?: string;
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

function joinWorkspacePath(workspaceRoot: string | undefined, nodePath: string): string {
  if (!workspaceRoot) return nodePath;
  if (!nodePath) return workspaceRoot;

  const separator = workspaceRoot.includes("\\") ? "\\" : "/";
  const normalizedRoot = workspaceRoot.replace(/[\\/]+$/, "");
  const normalizedNodePath = nodePath
    .replace(/^[\\/]+/, "")
    .replace(/[\\/]+/g, separator);

  return `${normalizedRoot}${separator}${normalizedNodePath}`;
}

// ---------------------------------------------------------------------------
// CRUD Dialogs
// ---------------------------------------------------------------------------

interface NewFileDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  parentPath: string;
  workspaceId: string;
  defaultIsDir?: boolean;
  onSuccess: () => void;
}

export const NewFileDialog: FC<NewFileDialogProps> = ({
  open,
  onOpenChange,
  parentPath,
  workspaceId,
  defaultIsDir = false,
  onSuccess,
}) => {
  const t = useT();
  const [name, setName] = useState("");
  const [isDir, setIsDir] = useState(defaultIsDir);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  // Sync defaultIsDir when dialog opens
  useEffect(() => {
    if (open) setIsDir(defaultIsDir);
  }, [open, defaultIsDir]);

  const handleSubmit = async () => {
    if (!name.trim()) return;
    setPending(true);
    setError(null);
    try {
      await fileCreate(workspaceId, parentPath, name.trim(), isDir);
      onOpenChange(false);
      setName("");
      setIsDir(false);
      onSuccess();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) { setName(""); setError(null); } }}>
      <DialogContent className="sm:max-w-[360px]">
        <DialogHeader>
          <DialogTitle className="text-sm">
            {isDir ? t("fileContextMenu.newFolder") : t("fileContextMenu.newFile")}
          </DialogTitle>
          <DialogDescription className="text-xs text-muted-foreground">
            {t("fileContextMenu.createIn")} <code className="text-[10px]">{parentPath || "/"}</code>
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 py-2">
          <Input
            autoFocus
            placeholder={isDir ? t("fileContextMenu.folderPlaceholder") : t("fileContextMenu.filePlaceholder")}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") void handleSubmit(); }}
            className="h-8 text-sm"
          />
          <div className="flex items-center gap-2">
            <button
              className={`rounded px-2 py-1 text-xs transition-colors ${!isDir ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setIsDir(false)}
            >
              {t("fileContextMenu.file")}
            </button>
            <button
              className={`rounded px-2 py-1 text-xs transition-colors ${isDir ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setIsDir(true)}
            >
              {t("fileContextMenu.folder")}
            </button>
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button size="sm" disabled={!name.trim() || pending} onClick={() => void handleSubmit()}>
            {pending ? t("fileContextMenu.creating") : t("fileContextMenu.create")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

interface RenameDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentPath: string;
  currentName: string;
  workspaceId: string;
  onSuccess: () => void;
}

const RenameDialog: FC<RenameDialogProps> = ({
  open,
  onOpenChange,
  currentPath,
  currentName,
  workspaceId,
  onSuccess,
}) => {
  const t = useT();
  const [name, setName] = useState(currentName);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const handleSubmit = async () => {
    if (!name.trim() || name.trim() === currentName) return;
    setPending(true);
    setError(null);
    try {
      await fileRename(workspaceId, currentPath, name.trim());
      onOpenChange(false);
      onSuccess();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) setError(null); }}>
      <DialogContent className="sm:max-w-[360px]">
        <DialogHeader>
          <DialogTitle className="text-sm">{t("fileContextMenu.rename")}</DialogTitle>
          <DialogDescription className="text-xs text-muted-foreground">
            <code className="text-[10px]">{currentPath}</code>
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 py-2">
          <Input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") void handleSubmit(); }}
            className="h-8 text-sm"
          />
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button
            size="sm"
            disabled={!name.trim() || name.trim() === currentName || pending}
            onClick={() => void handleSubmit()}
          >
            {pending ? t("fileContextMenu.renaming") : t("fileContextMenu.rename")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

interface DeleteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  path: string;
  isDir: boolean;
  workspaceId: string;
  onSuccess: () => void;
}

const DeleteDialog: FC<DeleteDialogProps> = ({
  open,
  onOpenChange,
  path,
  isDir,
  workspaceId,
  onSuccess,
}) => {
  const t = useT();
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const handleDelete = async () => {
    setPending(true);
    setError(null);
    try {
      await fileDelete(workspaceId, path);
      onOpenChange(false);
      onSuccess();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) setError(null); }}>
      <DialogContent className="sm:max-w-[360px]">
        <DialogHeader>
          <DialogTitle className="text-sm">
            {isDir ? t("fileContextMenu.deleteFolderTitle") : t("fileContextMenu.deleteFileTitle")}
          </DialogTitle>
          <DialogDescription className="text-xs">
            {t("fileContextMenu.deleteConfirm", { path: path })}
            {isDir && ` ${t("fileContextMenu.deleteRecursive")}`}
          </DialogDescription>
        </DialogHeader>
        {error && <p className="text-xs text-destructive px-1">{error}</p>}
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("fileContextMenu.cancel")}
          </Button>
          <Button variant="destructive" size="sm" disabled={pending} onClick={() => void handleDelete()}>
            {pending ? t("fileContextMenu.deleting") : t("fileContextMenu.deleteAction")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

// ---------------------------------------------------------------------------
// Context menu wrapper
// ---------------------------------------------------------------------------

export const FileContextMenu: FC<FileContextMenuProps> = ({
  nodePath,
  isDir,
  isRoot = false,
  workspaceId,
  workspaceRoot,
  onTreeRefresh,
  onCopyPath,
  onOpenExternal,
  className,
}) => {
  const [newDialog, setNewDialog] = useState(false);
  const [newDialogIsDir, setNewDialogIsDir] = useState(false);
  const [renameDialog, setRenameDialog] = useState(false);
  const [deleteDialog, setDeleteDialog] = useState(false);

  const fileName = nodePath.split("/").pop() ?? nodePath;
  const t = useT();

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={t("fileContextMenu.fileActions")}
            className={cn(
              "inline-flex size-7 shrink-0 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground",
              className,
            )}
            onClick={(e) => e.stopPropagation()}
          >
            <EllipsisVertical className="size-3.5" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-48">
          {/* New file/folder — available on directories */}
          {isDir && (
            <>
              <DropdownMenuItem
                className="gap-2 text-xs"
                onClick={() => { setNewDialogIsDir(false); setNewDialog(true); }}
              >
                <FilePlus className="size-3.5" />
                {t("fileContextMenu.newFile")}
              </DropdownMenuItem>
              <DropdownMenuItem
                className="gap-2 text-xs"
                onClick={() => { setNewDialogIsDir(true); setNewDialog(true); }}
              >
                <FolderPlus className="size-3.5" />
                {t("fileContextMenu.newFolder")}
              </DropdownMenuItem>
              {!isRoot && <DropdownMenuSeparator />}
            </>
          )}

          {/* Rename & Delete — not for root */}
          {!isRoot && (
            <>
              <DropdownMenuItem
                className="gap-2 text-xs"
                onClick={() => setRenameDialog(true)}
              >
                <Pencil className="size-3.5" />
                {t("fileContextMenu.rename")}
              </DropdownMenuItem>
              <DropdownMenuItem
                className="gap-2 text-xs text-destructive focus:text-destructive"
                onClick={() => setDeleteDialog(true)}
              >
                <Trash2 className="size-3.5" />
                {t("fileContextMenu.deleteAction")}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
            </>
          )}

          {/* Copy paths */}
          {onCopyPath && !isRoot && (
            <DropdownMenuItem
              className="gap-2 text-xs"
              onClick={() => onCopyPath(nodePath)}
            >
              <Copy className="size-3.5" />
              {t("fileContextMenu.copyRelativePath")}
            </DropdownMenuItem>
          )}
          {!isRoot && (
            <DropdownMenuItem
              className="gap-2 text-xs"
              onClick={() => {
                const absolutePath = joinWorkspacePath(workspaceRoot, nodePath);
                void navigator.clipboard.writeText(absolutePath);
              }}
            >
              <ClipboardCopy className="size-3.5" />
              {t("fileContextMenu.copyPath")}
            </DropdownMenuItem>
          )}

          {/* Open external */}
          {onOpenExternal && !isDir && !isRoot && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                className="gap-2 text-xs"
                onClick={() => onOpenExternal(nodePath)}
              >
                <ExternalLink className="size-3.5" />
                {t("fileContextMenu.openInExternalApp")}
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      {/* Dialogs */}
      <NewFileDialog
        open={newDialog}
        onOpenChange={setNewDialog}
        parentPath={nodePath}
        workspaceId={workspaceId}
        defaultIsDir={newDialogIsDir}
        onSuccess={onTreeRefresh}
      />
      {!isRoot && (
        <>
          <RenameDialog
            open={renameDialog}
            onOpenChange={setRenameDialog}
            currentPath={nodePath}
            currentName={fileName}
            workspaceId={workspaceId}
            onSuccess={onTreeRefresh}
          />
          <DeleteDialog
            open={deleteDialog}
            onOpenChange={setDeleteDialog}
            path={nodePath}
            isDir={isDir}
            workspaceId={workspaceId}
            onSuccess={onTreeRefresh}
          />
        </>
      )}
    </>
  );
};
