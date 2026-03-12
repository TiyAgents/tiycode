import * as React from "react";
import { cn } from "@/shared/lib/utils";

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "flex min-h-16 w-full field-sizing-content rounded-lg border border-app-border bg-app-surface-muted px-3 py-2 text-[13px] leading-6 text-app-foreground shadow-xs transition-[color,box-shadow] outline-none placeholder:text-app-subtle disabled:cursor-not-allowed disabled:opacity-50 focus-visible:border-app-border-strong focus-visible:ring-0 aria-invalid:border-app-danger aria-invalid:ring-app-danger/20 dark:aria-invalid:ring-app-danger/40",
        className
      )}
      {...props}
    />
  );
}

export { Textarea };
