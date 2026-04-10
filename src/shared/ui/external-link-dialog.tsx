"use client";

import { CheckIcon, CopyIcon, ExternalLinkIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { LinkSafetyModalProps } from "streamdown";
import { openExternalUrl } from "@/shared/lib/open-external-url";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";

const COPY_RESET_DELAY_MS = 2000;

export type ExternalLinkDialogProps = LinkSafetyModalProps;

export function ExternalLinkDialog({
  isOpen,
  onClose,
  url,
}: ExternalLinkDialogProps) {
  const [isCopied, setIsCopied] = useState(false);
  const [isOpening, setIsOpening] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const copyResetTimeoutRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (copyResetTimeoutRef.current !== null) {
        window.clearTimeout(copyResetTimeoutRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!isOpen) {
      setIsCopied(false);
      setIsOpening(false);
      setErrorMessage(null);
      if (copyResetTimeoutRef.current !== null) {
        window.clearTimeout(copyResetTimeoutRef.current);
        copyResetTimeoutRef.current = null;
      }
    }
  }, [isOpen]);

  const handleCopy = useCallback(async () => {
    if (typeof navigator === "undefined" || !navigator.clipboard?.writeText) {
      setErrorMessage("Copy is not available in this environment.");
      return;
    }

    try {
      await navigator.clipboard.writeText(url);
      setIsCopied(true);
      setErrorMessage(null);

      if (copyResetTimeoutRef.current !== null) {
        window.clearTimeout(copyResetTimeoutRef.current);
      }

      copyResetTimeoutRef.current = window.setTimeout(() => {
        setIsCopied(false);
        copyResetTimeoutRef.current = null;
      }, COPY_RESET_DELAY_MS);
    } catch {
      setErrorMessage("Couldn't copy the link. Please try again.");
    }
  }, [url]);

  const handleOpen = useCallback(async () => {
    setIsOpening(true);
    try {
      await openExternalUrl(url);
      setErrorMessage(null);
      onClose();
    } catch {
      setErrorMessage("Couldn't open the link. You can copy it and open it manually.");
    } finally {
      setIsOpening(false);
    }
  }, [onClose, url]);

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          onClose();
        }
      }}
    >
      <DialogContent className="max-w-xl border-app-border bg-app-canvas">
        <DialogHeader className="gap-3 text-left">
          <DialogTitle className="flex items-center gap-3 text-2xl font-semibold tracking-[-0.03em]">
            <ExternalLinkIcon className="size-6" />
            Open external link?
          </DialogTitle>
          <DialogDescription className="text-base text-app-muted">
            You&apos;re about to visit an external website.
          </DialogDescription>
        </DialogHeader>

        <div className="rounded-2xl bg-app-surface-muted px-5 py-4 font-mono text-[13px] leading-6 text-app-foreground [overflow-wrap:anywhere]">
          {url}
        </div>

        <DialogFooter className="gap-3 sm:grid sm:grid-cols-2">
          <Button
            className="h-12 rounded-2xl text-base"
            onClick={() => {
              void handleCopy();
            }}
            type="button"
            variant="outline"
          >
            {isCopied ? <CheckIcon className="size-5" /> : <CopyIcon className="size-5" />}
            {isCopied ? "Copied" : "Copy link"}
          </Button>
          <Button
            className="h-12 rounded-2xl text-base"
            disabled={isOpening}
            onClick={() => {
              void handleOpen();
            }}
            type="button"
          >
            <ExternalLinkIcon className="size-5" />
            {isOpening ? "Opening..." : "Open link"}
          </Button>
        </DialogFooter>

        {errorMessage ? (
          <p className="text-sm text-app-danger">{errorMessage}</p>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
