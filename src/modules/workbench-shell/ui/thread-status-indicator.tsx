import { cn } from "@/shared/lib/utils";
import { THREAD_STATUS_META } from "@/modules/workbench-shell/model/fixtures";
import type { ThreadStatus } from "@/modules/workbench-shell/model/types";
import { useT } from "@/i18n";

export function ThreadStatusIndicator({
  status,
  emphasis = "default",
}: {
  status: ThreadStatus;
  emphasis?: "default" | "subtle";
}) {
  const t = useT();
  const meta = THREAD_STATUS_META[status];
  const Icon = meta.icon;
  const label = t(meta.labelKey);
  const isSubtle = emphasis === "subtle";
  const containerClassName = cn(
    "flex size-[1.15rem] shrink-0 items-center justify-center rounded-md border",
    status === "failed"
      ? isSubtle
        ? "border-app-danger/10 bg-app-danger/8 text-app-danger/80 dark:border-app-danger/16 dark:bg-app-danger/12 dark:text-app-danger/82"
        : "border-app-danger/15 bg-app-danger/12 text-app-danger dark:border-app-danger/20 dark:bg-app-danger/16"
      : status === "interrupted"
        ? isSubtle
          ? "border-app-warning/16 bg-app-warning/10 text-app-warning/90 dark:border-app-warning/20 dark:bg-app-warning/14 dark:text-app-warning/88"
          : "border-app-warning/22 bg-app-warning/14 text-app-warning dark:border-app-warning/24 dark:bg-app-warning/18"
      : isSubtle
        ? "border-app-border/70 bg-app-surface-muted/65 text-app-subtle dark:border-app-border dark:bg-app-surface-muted/55 dark:text-app-muted"
        : status === "running"
          ? "border-app-info/15 bg-app-info/12 text-app-info dark:border-app-info/20 dark:bg-app-info/16"
          : status === "completed"
            ? "border-app-success/15 bg-app-success/12 text-app-success dark:border-app-success/20 dark:bg-app-success/16"
            : "border-app-warning/20 bg-app-warning/14 text-app-warning dark:border-app-warning/22 dark:bg-app-warning/18",
  );

  return (
    <span className={containerClassName} title={label} aria-label={label}>
      <Icon className={cn("size-3.5", meta.spin && "animate-spin")} />
    </span>
  );
}
