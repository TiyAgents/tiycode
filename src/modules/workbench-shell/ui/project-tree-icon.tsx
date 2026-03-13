import { BookOpen, Braces, Code2, Folder, GitBranch } from "lucide-react";
import { cn } from "@/shared/lib/utils";
import type { ProjectTreeItem } from "@/modules/workbench-shell/model/types";

export function ProjectTreeIcon({
  icon,
  muted = false,
}: {
  icon: ProjectTreeItem["icon"];
  muted?: boolean;
}) {
  const iconClassName = muted ? "size-4 shrink-0 text-app-subtle/70" : "size-4 shrink-0 text-app-subtle";

  if (icon === "folder") {
    return <Folder className={iconClassName} />;
  }

  if (icon === "git") {
    return <GitBranch className={iconClassName} />;
  }

  if (icon === "html" || icon === "css") {
    return <Code2 className={iconClassName} />;
  }

  if (icon === "readme") {
    return <BookOpen className={iconClassName} />;
  }

  if (icon === "json") {
    return <Braces className={iconClassName} />;
  }

  if (icon === "license") {
    return <span className={cn("text-base leading-none", muted ? "text-app-subtle/70" : "text-app-subtle")}>=</span>;
  }

  return (
    <span
      className={cn(
        "flex h-[18px] min-w-[18px] items-center justify-center rounded-[4px] px-1 text-[9px] font-semibold uppercase tracking-[0.02em]",
        muted ? "bg-app-surface-muted/60 text-app-subtle/70" : "bg-app-surface-muted text-app-subtle",
      )}
    >
      TS
    </span>
  );
}
