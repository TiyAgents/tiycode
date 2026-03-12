import * as React from "react";
import { cn } from "@/shared/lib/utils";

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "flex h-9 w-full min-w-0 select-text rounded-lg border border-app-border bg-app-surface-muted px-3 py-1 text-[13px] text-app-foreground shadow-xs transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-app-subtle selection:bg-app-info selection:text-white disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 focus-visible:border-app-border-strong focus-visible:ring-0 aria-invalid:border-app-danger aria-invalid:ring-app-danger/20 dark:aria-invalid:ring-app-danger/40",
        className
      )}
      {...props}
    />
  );
}

export { Input };
