import { useDeferredValue, useEffect, useRef, useState } from "react";
import { FolderOpen, RefreshCw } from "lucide-react";
import { Input } from "@/shared/ui/input";
import { cn } from "@/shared/lib/utils";
import {
  DRAWER_LIST_LABEL_CLASS,
  DRAWER_LIST_ROW_CLASS,
  DRAWER_LIST_STACK_CLASS,
  PROJECT_TREE_ITEMS,
} from "@/modules/workbench-shell/model/fixtures";
import { ProjectTreeIcon } from "@/modules/workbench-shell/ui/project-tree-icon";

export function ProjectPanel() {
  const [filterValue, setFilterValue] = useState("");
  const [isRefreshing, setRefreshing] = useState(false);
  const deferredFilterValue = useDeferredValue(filterValue);
  const refreshTimeoutRef = useRef<number | null>(null);
  const normalizedFilter = deferredFilterValue.trim().toLowerCase();
  const visibleItems = normalizedFilter
    ? PROJECT_TREE_ITEMS.filter((item) => item.name.toLowerCase().includes(normalizedFilter))
    : PROJECT_TREE_ITEMS;

  useEffect(() => {
    return () => {
      if (refreshTimeoutRef.current) {
        window.clearTimeout(refreshTimeoutRef.current);
      }
    };
  }, []);

  const handleRefresh = () => {
    setFilterValue("");
    setRefreshing(true);

    if (refreshTimeoutRef.current) {
      window.clearTimeout(refreshTimeoutRef.current);
    }

    refreshTimeoutRef.current = window.setTimeout(() => {
      setRefreshing(false);
      refreshTimeoutRef.current = null;
    }, 700);
  };

  return (
    <div className="flex h-full min-h-0 flex-col px-4 pb-5 pt-2">
      <div className="shrink-0 bg-app-drawer">
        <div className="flex items-center justify-between gap-3 px-1 pr-1 text-[15px] font-medium">
          <div className="flex min-w-0 items-center gap-3">
            <FolderOpen className="size-4 shrink-0 text-app-subtle" />
            <span className="truncate text-app-foreground">tiy-desktop</span>
          </div>
          <button
            type="button"
            aria-label="刷新文件树"
            title="刷新文件树"
            className="flex size-7 shrink-0 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
            onClick={handleRefresh}
          >
            <RefreshCw className={cn("size-3.5", isRefreshing && "animate-spin")} />
          </button>
        </div>

        <div className="relative mt-2.5 pl-5 pr-1 pb-2.5">
          <div className="absolute bottom-0 left-[6px] top-0 w-px bg-app-border" />
          <Input
            value={filterValue}
            onChange={(event) => setFilterValue(event.target.value)}
            placeholder="Filter files"
            aria-label="Filter files"
            className="h-8 rounded-lg border-app-border bg-app-surface-muted px-2.5 text-[13px] text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0"
          />
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto overscroll-none pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <div className="relative pl-5">
          <div className="absolute bottom-0 left-[6px] top-0 w-px bg-app-border" />
          <div className={DRAWER_LIST_STACK_CLASS}>
            {visibleItems.map((item) => (
              <button
                key={item.id}
                type="button"
                className={cn(
                  `${DRAWER_LIST_ROW_CLASS} relative flex items-center gap-2`,
                  item.ignored
                    ? "text-app-subtle/70 hover:bg-app-surface-hover/60 hover:text-app-muted"
                    : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                )}
              >
                <ProjectTreeIcon icon={item.icon} muted={Boolean(item.ignored)} />
                <span className={DRAWER_LIST_LABEL_CLASS}>{item.name}</span>
              </button>
            ))}

            {visibleItems.length === 0 ? (
              <div className="px-2.5 py-2 text-[13px] text-app-subtle">No matching files</div>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
