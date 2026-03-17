import { useRef } from "react";
import { PanelBottom, RotateCcw, TerminalSquare, X } from "lucide-react";
import {
  TerminalHost,
  type TerminalHostHandle,
} from "@/features/terminal/ui/terminal-host";
import { useTerminalStore } from "@/features/terminal/model/terminal-store";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";

type ThreadTerminalPanelProps = {
  threadId: string | null;
  threadTitle: string | null;
  bootstrapError?: string | null;
  isPendingThread?: boolean;
  onCollapse: () => void;
};

const STATUS_LABELS = {
  starting: "启动中",
  running: "运行中",
  exited: "已退出",
} as const;

export function ThreadTerminalPanel({
  threadId,
  threadTitle,
  bootstrapError,
  isPendingThread = false,
  onCollapse,
}: ThreadTerminalPanelProps) {
  const terminalHostRef = useRef<TerminalHostHandle | null>(null);
  const session = useTerminalStore((current) =>
    threadId ? current.sessionsByThreadId[threadId] ?? null : null,
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-[38px] shrink-0 items-center justify-between gap-3 border-b border-app-border px-4 text-xs text-app-muted">
        <div className="min-w-0 flex items-center gap-2">
          <TerminalSquare className="size-3.5" />
          <span className="font-medium text-app-foreground">Terminal</span>
          <span className="truncate text-app-subtle">
            {threadTitle ?? "未选择线程"}
          </span>
          {session ? (
            <span
              className={cn(
                "rounded-full border px-2 py-0.5 text-[10px] uppercase tracking-[0.18em]",
                session.status === "running"
                  ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-300"
                  : session.status === "starting"
                    ? "border-sky-500/30 bg-sky-500/10 text-sky-300"
                    : "border-amber-500/30 bg-amber-500/10 text-amber-200",
              )}
            >
              {STATUS_LABELS[session.status]}
            </span>
          ) : null}
        </div>

        <div className="flex items-center gap-1">
          <Button
            size="icon"
            variant="ghost"
            className="size-7 text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground"
            aria-label="重启 terminal"
            title="重启 terminal"
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
            aria-label="关闭 terminal"
            title="关闭 terminal"
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
            aria-label="收起 terminal"
            title="收起 terminal"
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
          active
          bootstrapError={bootstrapError}
          idleMessage={
            isPendingThread
              ? "发生第一条消息后可进入Terminal"
              : undefined
          }
        />
      </div>
    </div>
  );
}
