# SubAgent Minimal Collapsible Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current bordered SubAgent helper cards in the thread timeline with a minimal one-line collapsible presentation, and add an expanded execution-summary footnote using currently available helper metrics.

**Architecture:** Introduce one reusable compact collapsible shell for lightweight runtime artifacts, then migrate helper rendering in `RuntimeThreadSurface` to use it. Keep V1 frontend-only by deriving elapsed time and tool-use counts from existing helper state, while designing the execution-summary formatter to accept future token metrics without UI restructuring.

**Tech Stack:** React 19, TypeScript, Tauri 2, Vite, Radix Collapsible, Tailwind CSS

---

### Task 1: Add a reusable compact collapsible shell

**Files:**
- Create: `src/components/ai-elements/compact-collapsible.tsx`
- Modify: `src/components/ai-elements/reasoning.tsx`
- Modify: `src/components/ai-elements/chain-of-thought.tsx`

**Step 1: Create the compact collapsible component**

Add `src/components/ai-elements/compact-collapsible.tsx` with a small composable API:

```tsx
"use client";

import { useControllableState } from "@radix-ui/react-use-controllable-state";
import { ChevronDownIcon } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { createContext, memo, useContext, useMemo } from "react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/shared/ui/collapsible";
import { cn } from "@/shared/lib/utils";

type CompactCollapsibleContextValue = {
  isOpen: boolean;
  setIsOpen: (open: boolean) => void;
};

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

export type CompactCollapsibleProps = ComponentProps<"div"> & {
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
};

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

    const value = useMemo(() => ({ isOpen, setIsOpen }), [isOpen, setIsOpen]);

    return (
      <CompactCollapsibleContext.Provider value={value}>
        <div className={cn("not-prose w-full", className)} {...props}>
          {children}
        </div>
      </CompactCollapsibleContext.Provider>
    );
  },
);

export const CompactCollapsibleHeader = memo(
  ({
    className,
    children,
    trailing,
    ...props
  }: ComponentProps<typeof CollapsibleTrigger> & {
    trailing?: ReactNode;
  }) => {
    const { isOpen, setIsOpen } = useCompactCollapsible();

    return (
      <Collapsible onOpenChange={setIsOpen} open={isOpen}>
        <CollapsibleTrigger
          className={cn(
            "flex w-full items-center gap-2 text-sm text-muted-foreground transition-colors hover:text-foreground",
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
      </Collapsible>
    );
  },
);

export const CompactCollapsibleContent = memo(
  ({ className, children, ...props }: ComponentProps<typeof CollapsibleContent>) => {
    const { isOpen } = useCompactCollapsible();

    return (
      <Collapsible open={isOpen}>
        <CollapsibleContent
          className={cn(
            "mt-2 outline-none data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:animate-in data-[state=open]:slide-in-from-top-2",
            className,
          )}
          {...props}
        >
          {children}
        </CollapsibleContent>
      </Collapsible>
    );
  },
);

export const CompactCollapsibleFootnote = memo(
  ({ className, ...props }: ComponentProps<"p">) => (
    <p className={cn("mt-3 text-xs text-muted-foreground", className)} {...props} />
  ),
);
```

**Step 2: Run typecheck to verify the new component compiles**

Run: `npm run typecheck`

Expected: PASS. If there is an unrelated baseline failure, record it in the working notes before moving on.

**Step 3: Refactor existing Thought-style components to align with the new shell**

Do not rewrite `Reasoning` or `ChainOfThought` to use the new component internally in this task. Instead, compare their trigger/content class patterns and adjust the new shell so it matches their spacing and animation language:

- keep `text-sm`
- keep muted text-first styling
- keep the same chevron rotation behavior
- keep the same slide/fade content animation family

This task is complete when the new shell clearly matches the visual grammar of the existing folded runtime UI.

**Step 4: Run typecheck again**

Run: `npm run typecheck`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/components/ai-elements/compact-collapsible.tsx
git commit -m "feat(thread): add compact collapsible runtime shell"
```

### Task 2: Add helper formatting utilities for summary, status, and execution footer

**Files:**
- Modify: `src/modules/workbench-shell/ui/runtime-thread-surface.tsx`

**Step 1: Add status and elapsed-time helpers near the existing helper formatting functions**

Add helpers near `formatHelperKind`, `formatHelperSummary`, and
`formatHelperToolCounts`:

```tsx
function formatHelperStatusLabel(
  status: SurfaceHelperEntry["status"],
): "running" | "done" | "failed" {
  switch (status) {
    case "completed":
      return "done";
    case "failed":
      return "failed";
    default:
      return "running";
  }
}

function getHelperElapsedSeconds(helper: SurfaceHelperEntry, now = Date.now()) {
  const startedAt = new Date(helper.startedAt).getTime();
  const finishedAt = helper.finishedAt ? new Date(helper.finishedAt).getTime() : now;

  if (Number.isNaN(startedAt) || Number.isNaN(finishedAt) || finishedAt < startedAt) {
    return null;
  }

  return (finishedAt - startedAt) / 1000;
}

function formatElapsedSeconds(seconds: number | null) {
  if (seconds === null) {
    return null;
  }
  return `${seconds.toFixed(1)}s elapsed`;
}
```

**Step 2: Add a helper execution-summary formatter with optional future metrics**

Add a formatter that only renders available metrics:

```tsx
type HelperExecutionSummaryMetrics = {
  elapsedText?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  toolUses?: number | null;
};

function formatExecutionSummary({
  elapsedText,
  inputTokens,
  outputTokens,
  toolUses,
}: HelperExecutionSummaryMetrics) {
  const fragments = [
    typeof toolUses === "number" && toolUses > 0
      ? `${toolUses} tool use${toolUses === 1 ? "" : "s"}`
      : null,
    elapsedText ?? null,
    typeof inputTokens === "number" && inputTokens > 0
      ? `input tokens ${formatCompactNumber(inputTokens)}`
      : null,
    typeof outputTokens === "number" && outputTokens > 0
      ? `output tokens ${formatCompactNumber(outputTokens)}`
      : null,
  ].filter(Boolean);

  return fragments.length > 0
    ? `Execution Summary: ${fragments.join(", ")}`
    : null;
}
```

Also add a tiny `formatCompactNumber()` helper for future token fields:

```tsx
function formatCompactNumber(value: number) {
  return new Intl.NumberFormat("en", {
    maximumFractionDigits: 1,
    notation: "compact",
  }).format(value);
}
```

**Step 3: Keep V1 metrics frontend-only**

Do not add fake token values and do not change `RunHelperDto` or
`SubagentProgressSnapshot` in this task. Build the summary using:

- `helper.totalToolCalls`
- derived elapsed time

Pass `inputTokens` and `outputTokens` as `undefined` for now so the formatter
proves the graceful-degradation path.

**Step 4: Run typecheck**

Run: `npm run typecheck`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/modules/workbench-shell/ui/runtime-thread-surface.tsx
git commit -m "refactor(thread): add helper summary formatting utilities"
```

### Task 3: Migrate SubAgent rows to the compact collapsible shell

**Files:**
- Modify: `src/modules/workbench-shell/ui/runtime-thread-surface.tsx`
- Modify: `src/components/ai-elements/compact-collapsible.tsx`

**Step 1: Replace the helper `Collapsible` import and render path**

In `runtime-thread-surface.tsx`, replace direct helper usage of:

```tsx
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/shared/ui/collapsible";
```

for the helper branch with:

```tsx
import {
  CompactCollapsible,
  CompactCollapsibleContent,
  CompactCollapsibleFootnote,
  CompactCollapsibleHeader,
} from "@/components/ai-elements/compact-collapsible";
```

Keep the base Radix collapsible import only if it is still needed for completed
tool groups.

**Step 2: Render the helper collapsed row as one line**

Replace the current bordered helper card block with:

```tsx
<CompactCollapsible
  defaultOpen={helper.status !== "completed"}
  className="w-full"
>
  <CompactCollapsibleHeader
    className="items-start gap-3"
    trailing={
      <span
        className={cn(
          "shrink-0 text-xs",
          helper.status === "failed"
            ? "text-app-danger"
            : helper.status === "completed"
              ? "text-app-muted"
              : "text-app-info",
        )}
      >
        {formatHelperStatusLabel(helper.status)}
      </span>
    }
  >
    <div className="flex min-w-0 items-start gap-3">
      <BotIcon
        className={cn(
          "mt-0.5 size-4 shrink-0",
          helper.status === "failed"
            ? "text-app-danger"
            : helper.status === "completed"
              ? "text-app-subtle"
              : "text-app-info",
        )}
      />
      <span
        className="truncate text-sm text-app-foreground"
        title={formatHelperSummary(helper)}
      >
        {formatHelperSummary(helper)}
      </span>
    </div>
  </CompactCollapsibleHeader>
  {/* content goes in the next step */}
</CompactCollapsible>
```

Rules:

- no border classes
- no background classes
- no `Badge`
- keep summary and status on the same row
- keep the row visually aligned with Thought/Reasoning triggers

**Step 3: Add the fixed-height expanded content and scrollable detail body**

Inside `CompactCollapsibleContent`, render:

```tsx
<CompactCollapsibleContent className="pl-7">
  <div className="max-h-40 space-y-2 overflow-y-auto pr-3">
    {formatHelperToolCounts(helper.toolCounts).length > 0 ? (
      <p className="whitespace-pre-wrap break-words text-xs text-app-subtle">
        {formatHelperToolCounts(helper.toolCounts).join(" · ")}
      </p>
    ) : null}
    {helper.totalToolCalls > 0 && helper.status !== "completed" ? (
      <p className="text-xs text-app-subtle">
        {`${helper.completedSteps} of ${formatToolCallCount(helper.totalToolCalls)} finished`}
      </p>
    ) : null}
    {helper.currentAction ? (
      <p className="whitespace-pre-wrap break-words text-xs text-app-subtle">
        {`Current: ${helper.currentAction}`}
      </p>
    ) : null}
    {helper.latestMessage ? (
      <p className="whitespace-pre-wrap break-words text-sm text-app-muted">
        {helper.latestMessage}
      </p>
    ) : null}
    {helper.recentActions.length > 0 ? (
      <div className="space-y-1">
        {helper.recentActions.slice(-3).map((action, index) => (
          <p
            className="whitespace-pre-wrap break-words text-sm text-app-muted"
            key={`${helper.id}-action-${index}`}
          >
            {action}
          </p>
        ))}
      </div>
    ) : null}
    {helper.summary ? (
      <p className="whitespace-pre-wrap break-words text-sm text-app-muted">
        {helper.summary}
      </p>
    ) : null}
    {helper.error ? (
      <p className="whitespace-pre-wrap break-words text-sm text-app-danger">
        {helper.error}
      </p>
    ) : null}
  </div>
</CompactCollapsibleContent>
```

Keep the content order consistent with the approved spec. Remove `helper.inputSummary`
from the expanded body if it makes the same text repeat too heavily against the
collapsed row summary. If removing it makes the detail block lose important
context in practice, re-add it below the tool/progress lines.

**Step 4: Add the execution-summary footnote**

Append:

```tsx
const executionSummary = formatExecutionSummary({
  elapsedText: formatElapsedSeconds(getHelperElapsedSeconds(helper)),
  toolUses: helper.totalToolCalls,
});
```

Then render:

```tsx
{executionSummary ? (
  <CompactCollapsibleFootnote className="pl-7">
    {executionSummary}
  </CompactCollapsibleFootnote>
) : null}
```

Make sure the footnote is outside the scrollable detail area so it reads like a
true footer instead of another log line.

**Step 5: Run typecheck**

Run: `npm run typecheck`

Expected: PASS.

**Step 6: Commit**

```bash
git add src/modules/workbench-shell/ui/runtime-thread-surface.tsx src/components/ai-elements/compact-collapsible.tsx
git commit -m "feat(thread): render SubAgent rows as minimal collapsibles"
```

### Task 4: Manually verify thread behavior in the workbench UI

**Files:**
- Verify: `src/modules/workbench-shell/ui/runtime-thread-surface.tsx`
- Verify: `src/components/ai-elements/compact-collapsible.tsx`

**Step 1: Start the web UI**

Run: `npm run dev:web`

Expected: Vite starts successfully and the workbench loads in the browser.

**Step 2: Exercise helper states in a thread with runtime activity**

Use a thread that includes helper/SubAgent entries and verify:

- completed helper rows are collapsed by default
- running helper rows are expanded by default
- failed helper rows are expanded by default
- collapsed rows show one line only: icon, summary, lightweight status, chevron
- helper rows no longer show a border, background card, or badge

**Step 3: Verify expanded layout behavior**

Open a completed helper and verify:

- the detail area uses a fixed visible height
- the content scrolls internally when long
- the execution-summary footer stays below the scroll area
- the footer reads like `Execution Summary: 12 tool uses, 123.4s elapsed`

**Step 4: Regression-check adjacent runtime artifacts**

Verify that:

- Thought/Reasoning blocks still render unchanged
- completed tool groups still use their existing collapsible behavior
- helper ordering in the timeline is unchanged
- helper updates still stream into the correct row while running

**Step 5: Record verification notes**

Write down any visual mismatches or overflow issues before committing the final
polish pass.

**Step 6: Commit**

```bash
git add src/modules/workbench-shell/ui/runtime-thread-surface.tsx src/components/ai-elements/compact-collapsible.tsx
git commit -m "fix(thread): polish SubAgent minimal helper presentation"
```

### Task 5: Leave a clear follow-up seam for backend token metrics

**Files:**
- Modify: `docs/superpowers/specs/2026-03-20-subagent-minimal-collapsible-design.md`
- Modify: `docs/plans/2026-03-20-subagent-minimal-collapsible.md`

**Step 1: Update the plan/spec only if implementation reveals naming or layout drift**

If the implementation ends up changing any of these approved details, document
the final contract clearly:

- status text labels
- footer wording
- whether `inputSummary` remains in the expanded body
- exact fixed-height choice

If implementation matches the approved design exactly, skip spec changes.

**Step 2: Add a short follow-up note for token metrics**

If not already obvious from the merged code, add a short note in the plan or
implementation notes that future backend work should extend helper DTOs with:

```ts
type HelperUsageMetrics = {
  inputTokens?: number | null;
  outputTokens?: number | null;
}
```

This is not a blocking code change for V1.

**Step 3: Commit any doc drift fixes**

```bash
git add docs/superpowers/specs/2026-03-20-subagent-minimal-collapsible-design.md docs/plans/2026-03-20-subagent-minimal-collapsible.md
git commit -m "docs(thread): align SubAgent plan with implementation"
```
