import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { Toggle as TogglePrimitive } from "radix-ui";
import { cn } from "@/shared/lib/utils";

const toggleVariants = cva(
  "inline-flex items-center justify-center gap-2 rounded-lg text-[13px] font-medium whitespace-nowrap transition-[color,box-shadow] outline-none hover:bg-app-surface-hover hover:text-app-foreground focus-visible:ring-2 focus-visible:ring-app-info/50 disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-app-danger aria-invalid:ring-app-danger/20 dark:aria-invalid:ring-app-danger/40 data-[state=on]:bg-app-surface data-[state=on]:text-app-foreground [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: "bg-transparent",
        outline:
          "border border-app-border bg-transparent shadow-xs hover:bg-app-surface-hover hover:text-app-foreground",
      },
      size: {
        default: "h-9 min-w-9 px-2",
        sm: "h-8 min-w-8 px-1.5 text-[13px]",
        lg: "h-10 min-w-10 px-2.5",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
);

function Toggle({
  className,
  variant,
  size,
  ...props
}: React.ComponentProps<typeof TogglePrimitive.Root> &
  VariantProps<typeof toggleVariants>) {
  return (
    <TogglePrimitive.Root
      data-slot="toggle"
      className={cn(toggleVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Toggle, toggleVariants };
