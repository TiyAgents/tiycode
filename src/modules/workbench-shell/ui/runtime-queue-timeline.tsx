"use client";

import {
  ArrowRightIcon,
  CheckCircle2Icon,
  ChevronDownIcon,
  CornerDownRightIcon,
  ListPlusIcon,
  Loader2Icon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import type { ComponentProps } from "react";
import {
  CompactCollapsible,
  CompactCollapsibleContent,
  CompactCollapsibleHeader,
} from "@/components/ai-elements/compact-collapsible";
import { useT } from "@/i18n";
import type {
  RuntimeQueueMessageDto,
  RuntimeQueueMessageKind,
  RuntimeQueueSnapshotDto,
} from "@/shared/types/api";
import { cn } from "@/shared/lib/utils";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/shared/ui/collapsible";
import { parseCommandComposerMetadata } from "./runtime-thread-surface-metadata";

export type RuntimeQueueTimelineProps = ComponentProps<"div"> & {
  queue: RuntimeQueueSnapshotDto | null;
  onCancelMessage?: (messageId: string) => void;
  cancellingMessageIds?: ReadonlySet<string>;
};

function kindLabel(kind: RuntimeQueueMessageKind, t: ReturnType<typeof useT>) {
  return kind === "follow_up" ? t("queue.followUp") : t("queue.steer");
}

function statusIcon(message: RuntimeQueueMessageDto) {
  if (message.status === "consumed") {
    return <CheckCircle2Icon className="size-3.5 text-success" />;
  }
  if (message.status === "cleared") {
    return <Trash2Icon className="size-3.5 text-app-subtle" />;
  }
  if (message.status === "cancelled") {
    return <XIcon className="size-3.5 text-app-subtle" />;
  }
  return message.kind === "follow_up"
    ? <CornerDownRightIcon className="size-3.5 text-app-info" />
    : <ArrowRightIcon className="size-3.5 text-app-warning" />;
}

function statusLabel(message: RuntimeQueueMessageDto, t: ReturnType<typeof useT>) {
  if (message.status === "consumed") {
    return t("queue.status.consumed");
  }
  if (message.status === "cleared") {
    return t("queue.status.cleared");
  }
  if (message.status === "cancelled") {
    return t("queue.status.cancelled");
  }
  return t("queue.status.pending");
}

function RuntimeQueueMessageCard({
  message,
  t,
  onCancelMessage,
  isCancelling,
}: {
  message: RuntimeQueueMessageDto;
  t: ReturnType<typeof useT>;
  onCancelMessage?: (messageId: string) => void;
  isCancelling?: boolean;
}) {
  const commandComposer = parseCommandComposerMetadata(message.metadata);
  const commandDisplayText = commandComposer?.kind === "command"
    ? commandComposer.displayText
    : null;
  const displayText = commandDisplayText?.trim() ? commandDisplayText : message.content;
  const expandedPrompt = commandComposer?.kind === "command"
    ? ((commandComposer.effectivePrompt ?? message.content).trim())
    : "";
  const shouldShowExpandedPrompt = Boolean(
    expandedPrompt && expandedPrompt !== displayText.trim(),
  );
  const canCancel = message.status === "pending" && Boolean(onCancelMessage);
  const cancelLabel = isCancelling ? t("queue.cancellingMessage") : t("queue.cancelMessage");

  return (
    <div
      className={cn(
        "group/queue-card rounded-lg border border-app-border/30 bg-app-surface/30 p-3",
        message.kind === "steer" && message.status === "pending" && "border-app-warning/20 bg-app-warning/5",
        message.kind === "follow_up" && message.status === "pending" && "border-app-info/20 bg-app-info/5",
      )}
    >
      <div className="flex items-start gap-2">
        <div className="mt-0.5 flex-shrink-0">{statusIcon(message)}</div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2 text-xs text-app-subtle">
            <span className="font-medium text-app-foreground">{kindLabel(message.kind, t)}</span>
            <span className="rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px]">
              {statusLabel(message, t)}
            </span>
          </div>
          <p className="mt-1 whitespace-pre-wrap break-words text-sm leading-6 text-app-muted">
            {displayText}
          </p>
          {shouldShowExpandedPrompt ? (
            <CompactCollapsible className="mt-2" defaultOpen={false}>
              <CompactCollapsibleHeader className="items-start gap-3 text-left text-app-subtle hover:text-app-foreground">
                <div className="min-w-0">
                  <div className="text-[11px] font-medium uppercase tracking-[0.08em] text-app-subtle">
                    Expanded prompt
                  </div>
                  <div className="truncate text-xs text-app-muted">
                    {expandedPrompt}
                  </div>
                </div>
              </CompactCollapsibleHeader>
              <CompactCollapsibleContent className="pl-0">
                <div className="whitespace-pre-wrap rounded-xl border border-app-border/25 bg-app-surface/35 px-3 py-2 text-xs leading-5 text-app-muted">
                  {expandedPrompt}
                </div>
              </CompactCollapsibleContent>
            </CompactCollapsible>
          ) : null}
        </div>
        {canCancel ? (
          <button
            type="button"
            aria-label={cancelLabel}
            title={cancelLabel}
            disabled={isCancelling}
            className={cn(
              "-mr-1 -mt-1 flex size-7 flex-shrink-0 items-center justify-center rounded-full text-app-subtle opacity-70 transition-all hover:bg-app-surface-hover hover:text-app-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-app-primary/40 disabled:cursor-not-allowed disabled:opacity-50 sm:opacity-0 sm:group-hover/queue-card:opacity-100",
              isCancelling && "opacity-100 sm:opacity-100",
            )}
            onClick={(event) => {
              event.stopPropagation();
              if (!isCancelling) {
                onCancelMessage?.(message.id);
              }
            }}
          >
            {isCancelling ? <Loader2Icon className="size-3.5 animate-spin" /> : <XIcon className="size-3.5" />}
          </button>
        ) : null}
      </div>
    </div>
  );
}

export const RuntimeQueueTimeline = ({
  queue,
  onCancelMessage,
  cancellingMessageIds,
  className,
  ...props
}: RuntimeQueueTimelineProps) => {
  const t = useT();

  if (!queue) {
    return null;
  }

  const pendingMessages = queue.messages.filter((message) => message.status === "pending");

  if (pendingMessages.length === 0) {
    return null;
  }

  const pendingCount = pendingMessages.length;
  const steeringMessages = pendingMessages.filter((message) => message.kind === "steer");
  const followUpMessages = pendingMessages.filter((message) => message.kind === "follow_up");

  return (
    <div className={cn("space-y-2", className)} {...props}>
      <Collapsible defaultOpen>
        <CollapsibleTrigger className="group flex w-full items-center justify-between gap-2 rounded-md text-xs text-muted-foreground transition-colors hover:text-foreground">
          <div className="flex items-center gap-2">
            <ListPlusIcon className="size-3" />
            <span>{t("queue.title")}</span>
            <span className="rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
              {t("queue.pendingCount", { count: pendingCount })}
            </span>
            {queue.isDeferringSteering ? (
              <span className="rounded-md bg-app-warning/10 px-1.5 py-0.5 text-[11px] text-app-warning">
                {t("queue.deferringSteer")}
              </span>
            ) : null}
          </div>
          <ChevronDownIcon className="size-4 transition-transform group-data-[state=open]:rotate-180" />
        </CollapsibleTrigger>
        <CollapsibleContent className="space-y-3 pt-2 outline-none data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 data-[state=closed]:animate-out data-[state=open]:animate-in">
          {steeringMessages.length > 0 ? (
            <div className="flex flex-col gap-2">
              <div className="text-[11px] font-medium uppercase tracking-wide text-app-subtle">
                {t("queue.steeringQueued", { count: queue.steeringDepth })}
              </div>
              {steeringMessages.map((message) => (
                <RuntimeQueueMessageCard
                  key={message.id}
                  message={message}
                  t={t}
                  onCancelMessage={onCancelMessage}
                  isCancelling={cancellingMessageIds?.has(message.id)}
                />
              ))}
            </div>
          ) : null}
          {followUpMessages.length > 0 ? (
            <div className="flex flex-col gap-2">
              <div className="text-[11px] font-medium uppercase tracking-wide text-app-subtle">
                {t("queue.followUpQueued", { count: queue.followUpDepth })}
              </div>
              {followUpMessages.map((message) => (
                <RuntimeQueueMessageCard
                  key={message.id}
                  message={message}
                  t={t}
                  onCancelMessage={onCancelMessage}
                  isCancelling={cancellingMessageIds?.has(message.id)}
                />
              ))}
            </div>
          ) : null}
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
};
