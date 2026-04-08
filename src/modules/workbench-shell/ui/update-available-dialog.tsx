import { Download, RefreshCw, RotateCcw, Terminal, X } from "lucide-react";

import { useT } from "@/i18n";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";

import type { AppUpdater, UpdateInfo } from "../hooks/use-app-updater";

type UpdateDialogPhase = "available" | "brewInstalled" | "downloading" | "readyToRestart" | "error";

interface UpdateAvailableDialogProps {
  phase: AppUpdater["phase"];
  updateInfo: UpdateInfo | null;
  downloadProgress: number;
  errorMessage: string | null;
  onDownloadAndInstall: () => void;
  onRestart: () => void;
  onRetry: () => void;
  onDismiss: () => void;
}

const DIALOG_PHASES = new Set<string>([
  "available",
  "brewInstalled",
  "downloading",
  "readyToRestart",
  "error",
]);

export function UpdateAvailableDialog({
  phase,
  updateInfo,
  downloadProgress,
  errorMessage,
  onDownloadAndInstall,
  onRestart,
  onRetry,
  onDismiss,
}: UpdateAvailableDialogProps) {
  const isOpen = DIALOG_PHASES.has(phase);
  const dialogPhase = phase as UpdateDialogPhase;

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && phase !== "downloading") {
          onDismiss();
        }
      }}
    >
      <DialogContent
        showCloseButton={phase !== "downloading"}
        className="max-w-[480px] rounded-2xl border-app-border bg-app-chrome"
      >
        {dialogPhase === "available" && updateInfo && (
          <AvailableContent
            updateInfo={updateInfo}
            onDownloadAndInstall={onDownloadAndInstall}
            onDismiss={onDismiss}
          />
        )}

        {dialogPhase === "brewInstalled" && updateInfo && (
          <BrewInstalledContent
            updateInfo={updateInfo}
            onDismiss={onDismiss}
          />
        )}

        {dialogPhase === "downloading" && (
          <DownloadingContent downloadProgress={downloadProgress} />
        )}

        {dialogPhase === "readyToRestart" && (
          <ReadyToRestartContent onRestart={onRestart} onDismiss={onDismiss} />
        )}

        {dialogPhase === "error" && (
          <ErrorContent
            errorMessage={errorMessage}
            onRetry={onRetry}
            onDismiss={onDismiss}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function AvailableContent({
  updateInfo,
  onDownloadAndInstall,
  onDismiss,
}: {
  updateInfo: UpdateInfo;
  onDownloadAndInstall: () => void;
  onDismiss: () => void;
}) {
  const t = useT();

  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-app-foreground">
          {t("update.newVersionAvailable")}
        </DialogTitle>
        <DialogDescription className="text-app-muted">
          {t("update.currentVersion", { version: updateInfo.currentVersion })}
          {" \u2192 "}
          {t("update.newVersion", { version: updateInfo.version })}
        </DialogDescription>
      </DialogHeader>

      {updateInfo.body && (
        <div className="max-h-[240px] overflow-y-auto rounded-xl border border-app-border bg-app-surface-muted p-4">
          <p className="mb-2 text-[12px] font-medium uppercase tracking-wider text-app-subtle">
            {t("update.releaseNotes")}
          </p>
          <div className="whitespace-pre-wrap text-[13px] leading-relaxed text-app-foreground">
            {updateInfo.body}
          </div>
        </div>
      )}

      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          className="h-9 rounded-xl border-app-border bg-app-surface-muted text-[13px] font-medium text-app-foreground shadow-none hover:border-app-border-strong hover:bg-app-surface-hover"
          onClick={onDismiss}
        >
          {t("update.later")}
        </Button>
        <Button
          type="button"
          className="h-9 rounded-xl text-[13px] font-medium shadow-none"
          onClick={onDownloadAndInstall}
        >
          <Download className="size-3.5" />
          {t("update.downloadAndInstall")}
        </Button>
      </DialogFooter>
    </>
  );
}

function BrewInstalledContent({
  updateInfo,
  onDismiss,
}: {
  updateInfo: UpdateInfo;
  onDismiss: () => void;
}) {
  const t = useT();

  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-app-foreground">
          {t("update.newVersionAvailable")}
        </DialogTitle>
        <DialogDescription className="text-app-muted">
          {t("update.currentVersion", { version: updateInfo.currentVersion })}
          {" \u2192 "}
          {t("update.newVersion", { version: updateInfo.version })}
        </DialogDescription>
      </DialogHeader>

      <div className="rounded-xl border border-app-border bg-app-surface-muted p-4">
        <div className="flex items-start gap-3">
          <Terminal className="mt-0.5 size-4 shrink-0 text-app-info" />
          <div className="space-y-2">
            <p className="text-[13px] leading-relaxed text-app-foreground">
              {t("update.brewDetected")}
            </p>
            <code className="block rounded-lg bg-app-code px-3 py-2 text-[13px] text-app-foreground">
              brew upgrade tiycode
            </code>
          </div>
        </div>
      </div>

      {updateInfo.body && (
        <div className="max-h-[240px] overflow-y-auto rounded-xl border border-app-border bg-app-surface-muted p-4">
          <p className="mb-2 text-[12px] font-medium uppercase tracking-wider text-app-subtle">
            {t("update.releaseNotes")}
          </p>
          <div className="whitespace-pre-wrap text-[13px] leading-relaxed text-app-foreground">
            {updateInfo.body}
          </div>
        </div>
      )}

      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          className="h-9 rounded-xl border-app-border bg-app-surface-muted text-[13px] font-medium text-app-foreground shadow-none hover:border-app-border-strong hover:bg-app-surface-hover"
          onClick={onDismiss}
        >
          {t("update.close")}
        </Button>
      </DialogFooter>
    </>
  );
}

function DownloadingContent({
  downloadProgress,
}: {
  downloadProgress: number;
}) {
  const t = useT();

  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-app-foreground">
          {t("update.downloading")}
        </DialogTitle>
        <DialogDescription className="text-app-muted">
          {t("update.downloadProgress", { progress: String(downloadProgress) })}
        </DialogDescription>
      </DialogHeader>

      <div className="py-2">
        <div className="h-2.5 w-full overflow-hidden rounded-full bg-app-surface-muted">
          <div
            className={cn(
              "h-full rounded-full bg-app-info transition-[width] duration-300 ease-out",
              downloadProgress < 100 && "animate-pulse",
            )}
            style={{ width: `${downloadProgress}%` }}
          />
        </div>
      </div>
    </>
  );
}

function ReadyToRestartContent({
  onRestart,
  onDismiss,
}: {
  onRestart: () => void;
  onDismiss: () => void;
}) {
  const t = useT();

  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-app-foreground">
          {t("update.readyToRestart")}
        </DialogTitle>
        <DialogDescription className="text-app-muted">
          {t("update.readyToRestartDesc")}
        </DialogDescription>
      </DialogHeader>

      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          className="h-9 rounded-xl border-app-border bg-app-surface-muted text-[13px] font-medium text-app-foreground shadow-none hover:border-app-border-strong hover:bg-app-surface-hover"
          onClick={onDismiss}
        >
          {t("update.restartLater")}
        </Button>
        <Button
          type="button"
          className="h-9 rounded-xl text-[13px] font-medium shadow-none"
          onClick={onRestart}
        >
          <RotateCcw className="size-3.5" />
          {t("update.restartNow")}
        </Button>
      </DialogFooter>
    </>
  );
}

function ErrorContent({
  errorMessage,
  onRetry,
  onDismiss,
}: {
  errorMessage: string | null;
  onRetry: () => void;
  onDismiss: () => void;
}) {
  const t = useT();

  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-app-foreground">
          {t("update.errorTitle")}
        </DialogTitle>
        {errorMessage && (
          <DialogDescription className="text-app-muted">
            {errorMessage}
          </DialogDescription>
        )}
      </DialogHeader>

      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          className="h-9 rounded-xl border-app-border bg-app-surface-muted text-[13px] font-medium text-app-foreground shadow-none hover:border-app-border-strong hover:bg-app-surface-hover"
          onClick={onDismiss}
        >
          <X className="size-3.5" />
          {t("update.close")}
        </Button>
        <Button
          type="button"
          className="h-9 rounded-xl text-[13px] font-medium shadow-none"
          onClick={onRetry}
        >
          <RefreshCw className="size-3.5" />
          {t("update.retry")}
        </Button>
      </DialogFooter>
    </>
  );
}
