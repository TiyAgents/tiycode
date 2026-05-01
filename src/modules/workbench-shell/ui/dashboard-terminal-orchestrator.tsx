import { useStore } from "@/shared/lib/create-store";
import { ThreadTerminalPanel } from "@/features/terminal/ui/thread-terminal-panel";
import { TerminalSettingsContext } from "@/features/terminal/model/terminal-settings-context";
import type { TerminalSettings } from "@/modules/settings-center/model/types";
import { projectStore } from "@/modules/workbench-shell/model/project-store";
import { uiLayoutStore } from "@/modules/workbench-shell/model/ui-layout-store";
import { useTerminalResize } from "@/modules/workbench-shell/hooks/use-terminal-resize";
import { cn } from "@/shared/lib/utils";

type DashboardTerminalOrchestratorProps = {
  active: boolean;
  idleMessage?: string;
  isPendingThread: boolean;
  onCollapse: () => void;
  terminal: TerminalSettings;
  threadId: string | null;
  threadTitle: string;
};

export function DashboardTerminalOrchestrator({
  active,
  idleMessage,
  isPendingThread,
  onCollapse,
  terminal,
  threadId,
  threadTitle,
}: DashboardTerminalOrchestratorProps) {
  const bootstrapError = useStore(projectStore, (s) => s.terminalBootstrapError);
  const height = useStore(uiLayoutStore, (s) => s.terminalHeight);
  const { handleTerminalResizeStart: onResizeStart } = useTerminalResize();
  return (
    <section
      className={cn(
        "relative shrink-0 overflow-hidden bg-app-terminal transition-[height,opacity,border-color] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
        active
          ? "border-t border-app-border opacity-100"
          : "border-t border-transparent opacity-0 pointer-events-none",
      )}
      style={{ height: active ? height : 0 }}
    >
      <div
        className={cn(
          "group absolute inset-x-0 top-0 z-10 flex h-4 -translate-y-1/2 items-start justify-center transition-opacity duration-200",
          active ? "cursor-row-resize opacity-100" : "opacity-0",
        )}
        role="presentation"
        onMouseDown={onResizeStart}
      >
        <div className="mt-1.5 h-[2px] w-9 rounded-full bg-app-border opacity-50 transition-all duration-200 ease-out group-hover:w-14 group-hover:bg-app-border-strong group-hover:opacity-100" />
      </div>
      <div
        className={cn(
          "flex h-full min-h-0 flex-col transition-opacity duration-200",
          active ? "opacity-100 delay-75" : "opacity-0",
        )}
      >
        <TerminalSettingsContext.Provider value={terminal}>
          <ThreadTerminalPanel
            threadId={threadId}
            threadTitle={threadTitle}
            active={active}
            bootstrapError={bootstrapError}
            isPendingThread={isPendingThread}
            idleMessage={idleMessage}
            onCollapse={onCollapse}
          />
        </TerminalSettingsContext.Provider>
      </div>
    </section>
  );
}
