import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/shared/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-all disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 outline-none focus-visible:ring-2 focus-visible:ring-app-info/50 aria-invalid:ring-app-danger/20 dark:aria-invalid:ring-app-danger/40 aria-invalid:border-app-danger",
  {
    variants: {
      variant: {
        default: "bg-app-foreground text-app-canvas shadow-xs hover:bg-app-foreground/90",
        destructive: "bg-app-danger text-white shadow-xs hover:bg-app-danger/90",
        outline: "border border-app-border bg-app-surface shadow-xs hover:bg-app-surface-hover hover:text-app-foreground",
        secondary: "bg-app-surface-muted text-app-foreground shadow-xs hover:bg-app-surface-hover",
        ghost: "hover:bg-app-surface-hover hover:text-app-foreground",
        link: "text-app-info underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md gap-1.5 px-3 text-[13px]",
        lg: "h-10 rounded-md px-6",
        icon: "size-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
);

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  }) {
  const Comp = asChild ? Slot : "button";

  return (
    <Comp
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
