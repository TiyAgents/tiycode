"use client";

import { useT } from "@/i18n";
import { Badge } from "@/shared/ui/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/shared/ui/collapsible";
import { cn } from "@/shared/lib/utils";
import type { DynamicToolUIPart, ToolUIPart } from "ai";
import {
  CheckCircleIcon,
  ChevronDownIcon,
  CircleIcon,
  ClockIcon,
  WrenchIcon,
  XCircleIcon,
} from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { isValidElement } from "react";

import { CodeBlock } from "./code-block";

export type ToolProps = ComponentProps<typeof Collapsible>;

export const Tool = ({ className, ...props }: ToolProps) => (
  <Collapsible
    className={cn("group not-prose mb-4 w-full rounded-md border", className)}
    {...props}
  />
);

export type ToolPart = ToolUIPart | DynamicToolUIPart;

export type ToolHeaderProps = {
  title?: string;
  className?: string;
} & (
  | { type: ToolUIPart["type"]; state: ToolUIPart["state"]; toolName?: never }
  | {
      type: DynamicToolUIPart["type"];
      state: DynamicToolUIPart["state"];
      toolName: string;
    }
);

const statusIcons: Record<ToolPart["state"], ReactNode> = {
  "approval-requested": <ClockIcon className="size-4 text-yellow-600" />,
  "approval-responded": <CheckCircleIcon className="size-4 text-blue-600" />,
  "input-available": <ClockIcon className="size-4 animate-pulse" />,
  "input-streaming": <CircleIcon className="size-4" />,
  "output-available": <CheckCircleIcon className="size-4 text-green-600" />,
  "output-denied": <XCircleIcon className="size-4 text-orange-600" />,
  "output-error": <XCircleIcon className="size-4 text-red-600" />,
};

function getStatusLabel(status: ToolPart["state"], t: ReturnType<typeof useT>) {
  switch (status) {
    case "approval-requested":
      return t("tool.status.awaitingApproval");
    case "approval-responded":
      return t("tool.status.responded");
    case "input-available":
      return t("tool.status.running");
    case "input-streaming":
      return t("tool.status.pending");
    case "output-available":
      return t("tool.status.completed");
    case "output-denied":
      return t("tool.status.denied");
    case "output-error":
      return t("tool.status.error");
  }
}

function getStatusBadge(
  status: ToolPart["state"],
  t: ReturnType<typeof useT>
) {
  return (
    <Badge className="gap-1.5 rounded-full text-xs" variant="secondary">
      {statusIcons[status]}
      {getStatusLabel(status, t)}
    </Badge>
  );
}

export const ToolHeader = ({
  className,
  title,
  type,
  state,
  toolName,
  ...props
}: ToolHeaderProps) => {
  const t = useT();
  const derivedName =
    type === "dynamic-tool" ? toolName : type.split("-").slice(1).join("-");

  return (
    <CollapsibleTrigger
      className={cn(
        "flex w-full items-center justify-between gap-4 p-3",
        className
      )}
      {...props}
    >
      <div className="flex items-center gap-2">
        <WrenchIcon className="size-4 text-muted-foreground" />
        <span className="font-medium text-sm">{title ?? derivedName}</span>
        {getStatusBadge(state, t)}
      </div>
      <ChevronDownIcon className="size-4 text-muted-foreground transition-transform group-data-[state=open]:rotate-180" />
    </CollapsibleTrigger>
  );
};

export type ToolContentProps = ComponentProps<typeof CollapsibleContent>;

export const ToolContent = ({ className, ...props }: ToolContentProps) => (
  <CollapsibleContent
    className={cn(
      "data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 space-y-4 p-4 text-popover-foreground outline-none data-[state=closed]:animate-out data-[state=open]:animate-in",
      className
    )}
    {...props}
  />
);

export type ToolInputProps = ComponentProps<"div"> & {
  input: ToolPart["input"];
  label?: string;
  codeBlockContentClassName?: string;
};

export const ToolInput = ({
  className,
  input,
  label,
  codeBlockContentClassName,
  ...props
}: ToolInputProps) => {
  const t = useT();
  const resolvedLabel = label ?? t("tool.label.parameters");

  return (
    <div className={cn("space-y-2 overflow-hidden", className)} {...props}>
      <h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {resolvedLabel}
      </h4>
      <CodeBlock
        code={JSON.stringify(input, null, 2)}
        contentClassName={codeBlockContentClassName}
        language="json"
      />
    </div>
  );
};

export type ToolOutputProps = ComponentProps<"div"> & {
  output: ToolPart["output"];
  errorText: ToolPart["errorText"];
  errorLabel?: string;
  label?: string;
  codeBlockContentClassName?: string;
};

export const ToolOutput = ({
  className,
  output,
  errorText,
  errorLabel,
  label,
  codeBlockContentClassName,
  ...props
}: ToolOutputProps) => {
  const t = useT();
  const resolvedErrorLabel = errorLabel ?? t("tool.label.error");
  const resolvedLabel = label ?? t("tool.label.result");

  if (!(output || errorText)) {
    return null;
  }

  const hasCodeBlockOutput =
    (typeof output === "object" && !isValidElement(output)) ||
    typeof output === "string";

  let Output = <div>{output as ReactNode}</div>;

  if (typeof output === "object" && !isValidElement(output)) {
    Output = (
      <CodeBlock
        className={errorText ? "border-app-danger/20 bg-app-danger/6" : undefined}
        code={JSON.stringify(output, null, 2)}
        contentClassName={codeBlockContentClassName}
        language="json"
      />
    );
  } else if (typeof output === "string") {
    Output = (
      <CodeBlock
        className={errorText ? "border-app-danger/20 bg-app-danger/6" : undefined}
        code={output}
        contentClassName={codeBlockContentClassName}
        language="json"
      />
    );
  }

  return (
    <div className={cn("space-y-2", className)} {...props}>
      <h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {errorText ? resolvedErrorLabel : resolvedLabel}
      </h4>
      <div
        className={cn(
          "overflow-x-auto text-xs [&_table]:w-full",
          !hasCodeBlockOutput &&
            errorText
            ? "bg-destructive/10 text-destructive"
            : !hasCodeBlockOutput
              ? "rounded-md bg-muted/50 text-foreground"
              : undefined
        )}
      >
        {errorText && <div>{errorText}</div>}
        {Output}
      </div>
    </div>
  );
};
