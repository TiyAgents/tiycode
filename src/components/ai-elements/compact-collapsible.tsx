"use client";

import { useControllableState } from "@radix-ui/react-use-controllable-state";
import type { ComponentProps, ReactNode } from "react";
import { createContext, memo, useContext, useMemo } from "react";
import { ChevronDownIcon } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/shared/ui/collapsible";
import { cn } from "@/shared/lib/utils";

interface CompactCollapsibleContextValue {
  isOpen: boolean;
}

const CompactCollapsibleContext =
  createContext<CompactCollapsibleContextValue | null>(null);

function useCompactCollapsible() {
  const context = useContext(CompactCollapsibleContext);
  if (!context) {
    throw new Error(
      "CompactCollapsible components must be used within CompactCollapsible",
    );
  }

  return context;
}

export type CompactCollapsibleProps = ComponentProps<typeof Collapsible>;

export const CompactCollapsible = memo(
  ({
    className,
    open,
    defaultOpen = false,
    onOpenChange,
    children,
    ...props
  }: CompactCollapsibleProps) => {
    const [isOpen, setIsOpen] = useControllableState({
      defaultProp: defaultOpen,
      onChange: onOpenChange,
      prop: open,
    });
    const contextValue = useMemo(() => ({ isOpen }), [isOpen]);

    return (
      <CompactCollapsibleContext.Provider value={contextValue}>
        <Collapsible
          className={cn("not-prose w-full", className)}
          onOpenChange={setIsOpen}
          open={isOpen}
          {...props}
        >
          {children}
        </Collapsible>
      </CompactCollapsibleContext.Provider>
    );
  },
);

export type CompactCollapsibleHeaderProps = ComponentProps<
  typeof CollapsibleTrigger
> & {
  trailing?: ReactNode;
};

export const CompactCollapsibleHeader = memo(
  ({
    className,
    children,
    trailing,
    ...props
  }: CompactCollapsibleHeaderProps) => {
    const { isOpen } = useCompactCollapsible();

    return (
      <CollapsibleTrigger
        className={cn(
          "flex w-full items-center gap-2 text-muted-foreground text-sm transition-colors hover:text-foreground",
          className,
        )}
        {...props}
      >
        <div className="min-w-0 flex-1">{children}</div>
        {trailing}
        <ChevronDownIcon
          className={cn(
            "size-4 shrink-0 transition-transform",
            isOpen ? "rotate-180" : "rotate-0",
          )}
        />
      </CollapsibleTrigger>
    );
  },
);

export type CompactCollapsibleContentProps = ComponentProps<
  typeof CollapsibleContent
>;

export const CompactCollapsibleContent = memo(
  ({ className, ...props }: CompactCollapsibleContentProps) => (
    <CollapsibleContent
      className={cn(
        "mt-2 outline-none data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 data-[state=closed]:animate-out data-[state=open]:animate-in",
        className,
      )}
      {...props}
    />
  ),
);

export type CompactCollapsibleFootnoteProps = ComponentProps<"p">;

export const CompactCollapsibleFootnote = memo(
  ({ className, ...props }: CompactCollapsibleFootnoteProps) => (
    <p
      className={cn("mt-3 text-muted-foreground text-xs", className)}
      {...props}
    />
  ),
);

CompactCollapsible.displayName = "CompactCollapsible";
CompactCollapsibleHeader.displayName = "CompactCollapsibleHeader";
CompactCollapsibleContent.displayName = "CompactCollapsibleContent";
CompactCollapsibleFootnote.displayName = "CompactCollapsibleFootnote";
