import { useDeferredValue, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, ChevronDown, FolderOpen, LoaderCircle } from "lucide-react";
import { Input } from "@/shared/ui/input";
import { cn } from "@/shared/lib/utils";
import {
  DRAWER_LIST_LABEL_CLASS,
  DRAWER_LIST_ROW_CLASS,
  DRAWER_LIST_STACK_CLASS,
  PROJECT_TREE_ITEMS,
} from "@/modules/workbench-shell/model/fixtures";
import { useWorkspaceOpenApps } from "@/modules/workbench-shell/model/use-workspace-open-apps";
import type { ProjectOption, WorkspaceOpenApp } from "@/modules/workbench-shell/model/types";
import { ProjectTreeIcon } from "@/modules/workbench-shell/ui/project-tree-icon";

const APP_ICON_FALLBACKS: Record<
  string,
  { src?: string; label: string; className: string }
> = {
  finder: { label: "F", className: "bg-linear-to-br from-sky-400 to-blue-500 text-white" },
  explorer: { label: "E", className: "bg-linear-to-br from-amber-300 to-yellow-500 text-slate-900" },
  vscode: { label: "VS", className: "bg-linear-to-br from-sky-500 to-blue-700 text-white" },
  cursor: { src: "/llm-icons/cursor.svg", label: "C", className: "bg-slate-900 text-white" },
  windsurf: { src: "/llm-icons/windsurf.svg", label: "W", className: "bg-cyan-500 text-white" },
  zed: { label: "Z", className: "bg-linear-to-br from-orange-500 to-rose-500 text-white" },
};

function WorkspaceAppIcon({
  app,
  sizeClassName,
  radiusClassName,
}: {
  app: WorkspaceOpenApp;
  sizeClassName: string;
  radiusClassName: string;
}) {
  const fallback = APP_ICON_FALLBACKS[app.id] ?? {
    label: app.name.slice(0, 1).toUpperCase(),
    className: "bg-app-surface-muted text-app-foreground",
  };

  if (app.iconDataUrl) {
    return <img src={app.iconDataUrl} alt="" className={cn(sizeClassName, radiusClassName, "shrink-0 object-cover")} />;
  }

  if (fallback.src) {
    return (
      <span className={cn(sizeClassName, radiusClassName, fallback.className, "inline-flex shrink-0 items-center justify-center")}>
        <img src={fallback.src} alt="" className="size-[70%] object-contain" />
      </span>
    );
  }

  return (
    <span
      className={cn(
        sizeClassName,
        radiusClassName,
        fallback.className,
        "inline-flex shrink-0 items-center justify-center text-[9px] font-semibold tracking-[-0.02em]",
      )}
    >
      {fallback.label}
    </span>
  );
}

export function ProjectPanel({ currentProject }: { currentProject: ProjectOption | null }) {
  const [filterValue, setFilterValue] = useState("");
  const [isOpenMenuOpen, setOpenMenuOpen] = useState(false);
  const [preferredOpenAppId, setPreferredOpenAppId] = useState<string | null>(null);
  const [activeOpenTargetId, setActiveOpenTargetId] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const deferredFilterValue = useDeferredValue(filterValue);
  const openMenuRef = useRef<HTMLDivElement | null>(null);
  const errorTimeoutRef = useRef<number | null>(null);
  const { data: openApps, error: openAppsError, isLoading: isLoadingOpenApps } = useWorkspaceOpenApps();
  const normalizedFilter = deferredFilterValue.trim().toLowerCase();
  const visibleItems = normalizedFilter
    ? PROJECT_TREE_ITEMS.filter((item) => item.name.toLowerCase().includes(normalizedFilter))
    : PROJECT_TREE_ITEMS;
  const projectName = currentProject?.name ?? "Project";
  const projectPath = currentProject?.path ?? null;
  const preferredOpenApp = openApps.find((app) => app.id === preferredOpenAppId) ?? openApps[0] ?? null;

  useEffect(() => {
    return () => {
      if (errorTimeoutRef.current) {
        window.clearTimeout(errorTimeoutRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!isOpenMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && openMenuRef.current?.contains(target)) {
        return;
      }

      setOpenMenuOpen(false);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isOpenMenuOpen]);

  useEffect(() => {
    if (!preferredOpenAppId || !openApps.some((app) => app.id === preferredOpenAppId)) {
      setPreferredOpenAppId(openApps[0]?.id ?? null);
    }
  }, [openApps, preferredOpenAppId]);

  const getOpenErrorMessage = (error: unknown, fallback: string) => {
    if (typeof error === "string" && error.trim().length > 0) {
      return error;
    }

    if (error instanceof Error && error.message.trim().length > 0) {
      return error.message;
    }

    if (typeof error === "object" && error !== null) {
      const message = Reflect.get(error, "message");
      if (typeof message === "string" && message.trim().length > 0) {
        return message;
      }

      try {
        const serialized = JSON.stringify(error);
        if (serialized && serialized !== "{}") {
          return serialized;
        }
      } catch {
        // no-op
      }
    }

    return fallback;
  };

  const handleOpenInApp = async (app: WorkspaceOpenApp) => {
    if (!projectPath) {
      return;
    }

    setActiveOpenTargetId(app.id);

    try {
      await invoke("open_workspace_in_app", {
        targetPath: projectPath,
        appPath: app.openWith,
      });
      setPreferredOpenAppId(app.id);
      setOpenMenuOpen(false);
      setOpenError(null);
    } catch (error) {
      const message = getOpenErrorMessage(error, `Couldn't open in ${app.name}`);
      setOpenError(message);
      if (errorTimeoutRef.current) {
        window.clearTimeout(errorTimeoutRef.current);
      }
      errorTimeoutRef.current = window.setTimeout(() => {
        setOpenError(null);
        errorTimeoutRef.current = null;
      }, 2200);
    } finally {
      setActiveOpenTargetId(null);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col px-4 pb-5 pt-2">
      <div className="shrink-0 bg-app-drawer">
        <div className="flex items-center justify-between gap-3 px-1 pr-1 text-[15px] font-medium">
          <div className="flex min-w-0 items-center gap-3">
            <FolderOpen className="size-4 shrink-0 text-app-subtle" />
            <span className="truncate text-app-foreground">{projectName}</span>
          </div>
          {isLoadingOpenApps || preferredOpenApp ? (
            <div ref={openMenuRef} className="relative shrink-0">
              <div
                className={cn(
                  "inline-flex h-8 items-stretch overflow-hidden rounded-2xl border border-app-border bg-app-surface/90 text-app-subtle transition-[border-color,background-color,box-shadow]",
                  isOpenMenuOpen && "border-app-border-strong bg-app-surface text-app-foreground shadow-[0_8px_18px_rgba(15,23,42,0.08)]",
                )}
              >
                <button
                  type="button"
                  aria-label={preferredOpenApp ? `Open folder with ${preferredOpenApp.name}` : "Loading supported apps"}
                  title={preferredOpenApp ? `Open folder with ${preferredOpenApp.name}` : "Loading supported apps"}
                  disabled={!projectPath || isLoadingOpenApps || openApps.length === 0 || !preferredOpenApp}
                  className="inline-flex min-w-0 items-center px-2.5 transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:opacity-60"
                  onClick={() => {
                    if (preferredOpenApp) {
                      void handleOpenInApp(preferredOpenApp);
                    }
                  }}
                >
                  {isLoadingOpenApps ? (
                    <LoaderCircle className="size-4 shrink-0 animate-spin text-app-subtle" />
                  ) : preferredOpenApp ? (
                    <WorkspaceAppIcon app={preferredOpenApp} sizeClassName="size-[18px]" radiusClassName="rounded-[5px]" />
                  ) : null}
                </button>

                <div className="w-px bg-app-border/80" />

                <button
                  type="button"
                  aria-label="Choose app to open folder"
                  title="Choose app to open folder"
                  aria-haspopup="menu"
                  aria-expanded={isOpenMenuOpen}
                  disabled={!projectPath || isLoadingOpenApps || openApps.length === 0}
                  className="inline-flex w-7 items-center justify-center transition-colors hover:bg-app-surface-hover disabled:cursor-not-allowed disabled:opacity-60"
                  onClick={() => setOpenMenuOpen((current) => !current)}
                >
                  <ChevronDown
                    className={cn(
                      "size-3.5 shrink-0 transition-transform duration-200",
                      isOpenMenuOpen && "rotate-180",
                    )}
                  />
                </button>
              </div>

              {isOpenMenuOpen ? (
                <div className="absolute right-0 top-[calc(100%+0.45rem)] z-20 min-w-[220px] overflow-hidden rounded-2xl border border-app-border bg-app-menu/98 p-1.5 shadow-[0_18px_40px_-26px_rgba(15,23,42,0.38)] backdrop-blur-xl dark:bg-app-menu/94">
                  <div className="px-2.5 pb-1.5 pt-1">
                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-app-subtle">Open in</div>
                  </div>
                  <div className="space-y-0.5">
                    {openApps.map((app) => {
                      const isPending = activeOpenTargetId === app.id;
                      const isPreferred = preferredOpenApp?.id === app.id;

                      return (
                        <button
                          key={app.id}
                          type="button"
                          className={cn(
                            "flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-left transition-colors disabled:cursor-wait disabled:opacity-70",
                            isPreferred
                              ? "bg-app-surface-hover/80 text-app-foreground"
                              : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                          )}
                          disabled={Boolean(activeOpenTargetId)}
                          onClick={() => void handleOpenInApp(app)}
                        >
                          {isPending ? (
                            <LoaderCircle className="size-4 shrink-0 animate-spin text-app-subtle" />
                          ) : (
                            <WorkspaceAppIcon app={app} sizeClassName="size-5" radiusClassName="rounded-[7px]" />
                          )}
                          <span className="min-w-0 flex-1 truncate text-[12px] font-medium">{app.name}</span>
                          {isPreferred ? <Check className="size-3.5 shrink-0 text-app-foreground" /> : null}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}
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
          {openAppsError ? <p className="mt-2 text-[11px] text-app-danger">{openAppsError}</p> : null}
          {openError ? <p className="mt-2 text-[11px] text-app-danger">{openError}</p> : null}
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
