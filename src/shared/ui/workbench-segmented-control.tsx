import type { ReactNode } from "react";
import { cn } from "@/shared/lib/utils";
import { ToggleGroup, ToggleGroupItem } from "@/shared/ui/toggle-group";

type WorkbenchSegmentedControlOption<TValue extends string> = {
  content?: ReactNode;
  disabled?: boolean;
  label: string;
  title?: string;
  value: TValue;
};

type WorkbenchSegmentedControlProps<TValue extends string> = {
  className?: string;
  itemClassName?: string;
  onValueChange: (value: TValue) => void;
  options: ReadonlyArray<WorkbenchSegmentedControlOption<TValue>>;
  value: TValue;
};

export function WorkbenchSegmentedControl<TValue extends string>({
  className,
  itemClassName,
  onValueChange,
  options,
  value,
}: WorkbenchSegmentedControlProps<TValue>) {
  return (
    <ToggleGroup
      type="single"
      value={value}
      size="sm"
      variant="default"
      spacing={1}
      className={cn(
        "flex items-center gap-1 rounded-xl border border-app-border bg-app-surface-muted p-0.5",
        className,
      )}
      onValueChange={(nextValue) => {
        if (nextValue) {
          onValueChange(nextValue as TValue);
        }
      }}
    >
      {options.map((option) => (
        <ToggleGroupItem
          key={option.value}
          value={option.value}
          disabled={option.disabled}
          aria-label={option.label}
          title={option.title ?? option.label}
          className={cn(
            "flex h-8 flex-1 items-center justify-center rounded-lg border-transparent bg-transparent px-3.5 text-[12px] text-app-muted shadow-none transition-colors hover:bg-app-surface-hover hover:text-app-foreground focus-visible:ring-0 data-[state=on]:bg-app-surface data-[state=on]:text-app-foreground data-[state=on]:shadow-[0_1px_2px_rgba(15,23,42,0.08)]",
            itemClassName,
          )}
        >
          {option.content ?? option.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
