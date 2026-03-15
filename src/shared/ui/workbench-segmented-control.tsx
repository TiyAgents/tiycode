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
            "flex h-8 flex-1 items-center justify-center rounded-lg border border-transparent bg-transparent px-3.5 text-[12px] text-app-muted shadow-none transition-[background-color,border-color,color,box-shadow] duration-150 hover:bg-app-surface-hover hover:text-app-foreground focus-visible:ring-0 data-[state=on]:border-app-border-strong data-[state=on]:bg-app-surface-active data-[state=on]:text-app-foreground data-[state=on]:shadow-[inset_0_1px_0_rgba(255,255,255,0.42),0_5px_12px_rgba(15,23,42,0.08)] dark:data-[state=on]:shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_7px_16px_rgba(0,0,0,0.22)]",
            itemClassName,
          )}
        >
          {option.content ?? option.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
