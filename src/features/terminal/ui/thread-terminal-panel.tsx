import { useRef } from "react";
import { PanelBottom, RotateCcw, TerminalSquare, X } from "lucide-react";
import {
  TerminalHost,
  type TerminalHostHandle,
} from "@/features/terminal/ui/terminal-host";
import { useT } from "@/i18n";
import { Button } from "@/shared/ui/button";

type ThreadTerminalPanelProps = {
  threadId: string | null;
  threadTitle: string | null;
  active: boolean;
  bootstrapError?: string | null;
  isPendingThread?: boolean;
  idleMessage?: string;
  onCollapse: () => void;
};

export function ThreadTerminalPanel({
  threadId,
  threadTitle,
  active,
  bootstrapError,
  isPendingThread = false,
  idleMessage,
  onCollapse,
}: ThreadTerminalPanelProps) {
  const t = useT();
  const terminalHostRef = useRef<TerminalHostHandle | null>(null);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-[38px] shrink-0 items-center justify-between gap-3 border-b border-app-border px-4 text-xs text-app-muted">
        <div className="min-w-0 flex items-center gap-2">
          <TerminalSquare className="size-3.5" />
          <span className="font-medium text-app-foreground">Terminal</span>
          {isPendingThread ? null : (
            <span className="truncate text-app-subtle">
              {threadTitle ?? t("terminal.noThread")}
            </span>
          )}
        </div>

        <div className="flex items-center gap-1">
          <Button
            size="icon"
            variant="ghost"
            className="size-7 text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground"
            aria-label={t("terminal.restartTerminal")}
            title={t("terminal.restartTerminal")}
            disabled={!threadId}
            onClick={() => {
              void terminalHostRef.current?.restart();
            }}
          >
            <RotateCcw className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-7 text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground"
            aria-label={t("terminal.closeTerminal")}
            title={t("terminal.closeTerminal")}
            disabled={!threadId}
            onClick={() => {
              void terminalHostRef.current?.close();
            }}
          >
            <X className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-7 text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground"
            aria-label={t("terminal.collapseTerminal")}
            title={t("terminal.collapseTerminal")}
            onClick={onCollapse}
          >
            <PanelBottom className="size-4" />
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 bg-app-terminal">
        <TerminalHost
          key={`${threadId ?? "pending"}:${isPendingThread ? "new" : "bound"}`}
          ref={terminalHostRef}
          threadId={threadId}
          active={active}
          bootstrapError={bootstrapError}
          idleMessage={
            isPendingThread
              ? (idleMessage ?? t("terminal.sendFirstMessageHint"))
              : undefined
          }
        />
      </div>
    </div>
  );
}
