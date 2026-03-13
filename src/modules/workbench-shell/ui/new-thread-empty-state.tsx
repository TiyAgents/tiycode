import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, Folder, FolderPlus } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { cn } from "@/shared/lib/utils";
import { buildProjectOptionFromPath, formatProjectPathLabel } from "@/modules/workbench-shell/model/helpers";
import type { ProjectOption } from "@/modules/workbench-shell/model/types";

export function NewThreadEmptyState({
  recentProjects,
  selectedProject,
  onSelectProject,
}: {
  recentProjects: ReadonlyArray<ProjectOption>;
  selectedProject: ProjectOption | null;
  onSelectProject: (project: ProjectOption) => void;
}) {
  const [isMenuOpen, setMenuOpen] = useState(false);
  const projectMenuRef = useRef<HTMLDivElement | null>(null);
  const activeProject = selectedProject ?? recentProjects[0] ?? null;

  useEffect(() => {
    if (!isMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && projectMenuRef.current?.contains(target)) {
        return;
      }

      setMenuOpen(false);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isMenuOpen]);

  const handleChooseFolder = async () => {
    const selectedPath = await open({
      directory: true,
      multiple: false,
      title: "Choose project folder",
    });

    if (typeof selectedPath !== "string") {
      return;
    }

    const nextProject = buildProjectOptionFromPath(selectedPath);

    if (!nextProject) {
      return;
    }

    onSelectProject(nextProject);
    setMenuOpen(false);
  };

  return (
    <div className="relative isolate flex h-full min-h-0 w-full self-stretch items-center justify-center px-4 py-8">
      <div className="relative flex w-full max-w-[28rem] flex-col items-center justify-center gap-4">
        <div className="flex size-11 items-center justify-center rounded-2xl border border-app-border bg-app-surface text-app-foreground shadow-[0_10px_28px_rgba(15,23,42,0.08)] dark:shadow-[0_14px_30px_rgba(0,0,0,0.24)]">
          <img src="/app-icon.png" alt="Tiy Agent logo" className="size-7 object-contain opacity-90" />
        </div>

        <div className="flex flex-col items-center gap-1 text-center">
          <h1 className="text-balance text-[1.45rem] font-medium tracking-[-0.035em] text-app-foreground">
            Anything you need, through conversation.
          </h1>
          <p className="max-w-[30rem] text-sm leading-6 text-app-muted">
            Pick a local workspace first so the next thread can stay grounded in files, commands, and runtime context.
          </p>
        </div>

        <div ref={projectMenuRef} className="relative w-full max-w-[24rem]">
          <button
            type="button"
            aria-haspopup="menu"
            aria-expanded={isMenuOpen}
            className="inline-flex w-full items-center gap-3 rounded-2xl border border-app-border bg-app-surface/85 px-3.5 py-3 text-left shadow-[0_10px_24px_rgba(15,23,42,0.06)] transition-[border-color,background-color,box-shadow,color] duration-200 hover:border-app-border-strong hover:bg-app-surface hover:text-app-foreground hover:shadow-[0_16px_36px_rgba(15,23,42,0.1)] dark:shadow-[0_14px_32px_rgba(0,0,0,0.22)]"
            onClick={() => setMenuOpen((current) => !current)}
          >
            <div className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-border bg-app-surface-muted text-app-subtle">
              <Folder className="size-4 shrink-0" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-[1rem] font-medium tracking-[-0.02em] text-app-foreground">
                  {activeProject?.name ?? "Choose project"}
                </span>
                {activeProject ? (
                  <span className="shrink-0 rounded-full bg-app-surface-muted px-2 py-0.5 text-[10px] font-medium text-app-subtle">
                    {activeProject.lastOpenedLabel}
                  </span>
                ) : null}
              </div>
              <p className="mt-0.5 truncate text-[12px] text-app-subtle" title={activeProject?.path}>
                {activeProject ? formatProjectPathLabel(activeProject.path) : "Select a folder to start a workspace-backed thread"}
              </p>
            </div>
            <ChevronDown
              className={cn(
                "size-4 shrink-0 text-app-subtle transition-transform duration-200",
                isMenuOpen && "rotate-180",
              )}
            />
          </button>

          {isMenuOpen ? (
            <div className="absolute inset-x-0 top-[calc(100%+0.55rem)] z-20 max-h-[15rem] overflow-hidden rounded-[1.1rem] border border-app-border bg-app-menu/98 p-1.5 shadow-[0_18px_40px_-26px_rgba(15,23,42,0.38)] backdrop-blur-xl dark:bg-app-menu/94">
              <div className="flex max-h-[calc(15rem-0.75rem)] flex-col">
                <div className="flex items-center justify-between gap-3 px-2.5 pb-1.5 pt-0.5">
                  <span className="text-[11px] font-medium text-app-subtle">Recent projects</span>
                  {activeProject ? (
                    <span className="rounded-full bg-app-surface-muted px-2 py-0.5 text-[10px] font-medium text-app-subtle">
                      Current
                    </span>
                  ) : null}
                </div>

                <div className="min-h-0 flex-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                  <div className="space-y-0.5">
                    {recentProjects.map((project) => {
                      const isSelected = activeProject?.id === project.id;

                      return (
                        <button
                          key={`${project.id}-${project.path}`}
                          type="button"
                          className={cn(
                            "flex w-full items-start gap-2.5 rounded-xl px-2.5 py-2 text-left transition-colors",
                            isSelected
                              ? "bg-app-surface/75 text-app-foreground"
                              : "text-app-muted hover:bg-app-surface-hover/70 hover:text-app-foreground",
                          )}
                          onClick={() => {
                            onSelectProject(project);
                            setMenuOpen(false);
                          }}
                        >
                          <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface-muted text-app-subtle">
                            <Folder className="size-4 shrink-0" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="min-w-0 flex-1 truncate text-sm font-medium">{project.name}</span>
                              <span className="shrink-0 text-[10px] font-medium text-app-subtle">{project.lastOpenedLabel}</span>
                            </div>
                            <p className="mt-0.5 truncate text-[11px] leading-5 text-app-subtle" title={project.path}>
                              {formatProjectPathLabel(project.path)}
                            </p>
                          </div>
                          {isSelected ? <Check className="mt-0.5 size-4 shrink-0 text-app-foreground" /> : null}
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div className="mx-2 my-1.5 h-px shrink-0 bg-app-border" />

                <button
                  type="button"
                  className="flex w-full shrink-0 items-start gap-2.5 rounded-xl px-2.5 py-2 text-left text-app-foreground transition-colors hover:bg-app-surface-hover/70"
                  onClick={() => void handleChooseFolder()}
                >
                  <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface-muted text-app-subtle">
                    <FolderPlus className="size-4 shrink-0" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium">Choose new folder</div>
                    <p className="mt-0.5 text-[11px] leading-5 text-app-subtle">Browse a local workspace that is not in the recent list</p>
                  </div>
                </button>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
