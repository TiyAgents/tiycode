import type { ReactNode } from "react";
import { Separator } from "@/shared/ui/separator";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import { cn } from "@/shared/lib/utils";
import { useT } from "@/i18n";

export function PageHeading({
  description,
  title,
}: {
  description: string;
  title: string;
}) {
  return (
    <div>
      <h1 className="text-[19px] font-semibold text-app-foreground">{title}</h1>
      <p className="mt-1 text-[12px] leading-5 text-app-muted">{description}</p>
    </div>
  );
}

export function SettingsSection({
  action,
  children,
  headerClassName,
  title,
}: {
  action?: ReactNode;
  children: ReactNode;
  headerClassName?: string;
  title: string;
}) {
  return (
    <section>
      <div className={cn("mb-2 flex items-center justify-between px-1", headerClassName)}>
        <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">{title}</h2>
        {action ?? null}
      </div>
      <div className="overflow-hidden rounded-2xl border border-app-border bg-app-surface">{children}</div>
    </section>
  );
}

export function SettingsRow({
  control,
  description,
  label,
  optional,
}: {
  control: ReactNode;
  description: string;
  label: string;
  optional?: boolean;
}) {
  const t = useT();
  return (
    <div className="grid gap-3 bg-app-surface px-4 py-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-app-foreground">
          {label}
          {optional && (
            <span className="ml-1.5 inline-flex items-center rounded-full border border-app-border bg-app-surface px-2 py-0.5 text-[11px] font-medium text-app-muted">
              {t("settings.general.optionalBadge")}
            </span>
          )}
        </p>
        <p className="mt-1 text-[12px] leading-5 text-app-muted">{description}</p>
      </div>
      <div className="min-w-0 md:justify-self-end">{control}</div>
    </div>
  );
}

export function ChoiceGroup<TValue extends string>({
  onValueChange,
  options,
  value,
}: {
  onValueChange: (value: TValue) => void;
  options: ReadonlyArray<{ label: string; value: TValue }>;
  value: TValue;
}) {
  return (
    <WorkbenchSegmentedControl
      value={value}
      options={options}
      className="w-full md:w-auto"
      onValueChange={onValueChange}
    />
  );
}

export function SectionDivider() {
  return <Separator />;
}
