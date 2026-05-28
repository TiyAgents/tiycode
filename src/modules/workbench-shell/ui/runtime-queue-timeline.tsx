"use client";

import {
  CheckCircle2Icon,
  ChevronDownIcon,
  ListEnd,
  ListPlusIcon,
  ListStart,
  Loader2Icon,
  PencilIcon,
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

type RuntimeQueueTimelineVariant = "default" | "compact";

export type RuntimeQueueTimelineProps = ComponentProps<"div"> & {
  queue: RuntimeQueueSnapshotDto | null;
  onCancelMessage?: (messageId: string) => void;
  cancellingMessageIds?: ReadonlySet<string>;
  onPromoteMessage?: (messageId: string) => void;
  promotingMessageIds?: ReadonlySet<string>;
  onEditMessage?: (message: RuntimeQueueMessageDto) => void;
  editingMessageIds?: ReadonlySet<string>;
  variant?: RuntimeQueueTimelineVariant;
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
    ? <ListEnd className="size-3.5 text-app-info" />
    : <ListStart className="size-3.5 text-app-warning" />;
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

export function RuntimeQueueMessageCard({
  message,
  t,
  onCancelMessage,
  isCancelling,
  onPromoteMessage,
  isPromoting,
  onEditMessage,
  isEditing,
  variant = "default",
}: {
  message: RuntimeQueueMessageDto;
  t: ReturnType<typeof useT>;
  onCancelMessage?: (messageId: string) => void;
  isCancelling?: boolean;
  onPromoteMessage?: (messageId: string) => void;
  isPromoting?: boolean;
  onEditMessage?: (message: RuntimeQueueMessageDto) => void;
  isEditing?: boolean;
  variant?: RuntimeQueueTimelineVariant;
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
  const canPromote = message.status === "pending" && message.kind === "follow_up" && Boolean(onPromoteMessage);
  const canEdit = message.status === "pending" && Boolean(onEditMessage);
  const isBusy = Boolean(isCancelling || isPromoting || isEditing);
  const cancelLabel = isCancelling ? t("queue.cancellingMessage") : t("queue.cancelMessage");
  const promoteLabel = isPromoting ? t("queue.promotingMessage") : t("queue.promoteMessage");
  const editLabel = isEditing ? t("queue.editingMessage") : t("queue.editMessage");
  const isCompact = variant === "compact";

  return (
    <div
      className={cn(
        "group/queue-card relative rounded-lg border border-app-border/30 bg-app-surface/30",
        isCompact ? "px-2.5 py-2" : "p-3",
        message.kind === "steer" && message.status === "pending" && "border-app-warning/20 bg-app-warning/5",
        message.kind === "follow_up" && message.status === "pending" && "border-app-info/20 bg-app-info/5",
      )}
    >
      {canPromote || canEdit || canCancel ? (
        <div className={cn("absolute right-2 top-2 flex items-center", isCompact ? "gap-0.5" : "gap-1")}>
            {canPromote ? (
              <button
                type="button"
                aria-label={promoteLabel}
                title={promoteLabel}
                disabled={isBusy}
                className={cn(
                  "flex items-center justify-center rounded-full text-app-subtle opacity-70 transition-all hover:bg-app-surface-hover hover:text-app-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-app-primary/40 disabled:cursor-not-allowed disabled:opacity-50 sm:opacity-0 sm:group-hover/queue-card:opacity-100",
                  isCompact ? "size-6" : "size-7",
                  isPromoting && "opacity-100 sm:opacity-100",
                )}
                onClick={(event) => {
                  event.stopPropagation();
                  if (!isBusy) {
                    onPromoteMessage?.(message.id);
                  }
                }}
              >
                {isPromoting ? <Loader2Icon className="size-3.5 animate-spin" /> : <ListStart className="size-3.5" />}
              </button>
            ) : null}
            {canEdit ? (
              <button
                type="button"
                aria-label={editLabel}
                title={editLabel}
                disabled={isBusy}
                className={cn(
                  "flex items-center justify-center rounded-full text-app-subtle opacity-70 transition-all hover:bg-app-surface-hover hover:text-app-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-app-primary/40 disabled:cursor-not-allowed disabled:opacity-50 sm:opacity-0 sm:group-hover/queue-card:opacity-100",
                  isCompact ? "size-6" : "size-7",
                  isEditing && "opacity-100 sm:opacity-100",
                )}
                onClick={(event) => {
                  event.stopPropagation();
                  if (!isBusy) {
                    onEditMessage?.(message);
                  }
                }}
              >
                {isEditing ? <Loader2Icon className="size-3.5 animate-spin" /> : <PencilIcon className="size-3.5" />}
              </button>
            ) : null}
            {canCancel ? (
              <button
                type="button"
                aria-label={cancelLabel}
                title={cancelLabel}
                disabled={isBusy}
                className={cn(
                  "flex items-center justify-center rounded-full text-app-subtle opacity-70 transition-all hover:bg-app-surface-hover hover:text-app-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-app-primary/40 disabled:cursor-not-allowed disabled:opacity-50 sm:opacity-0 sm:group-hover/queue-card:opacity-100",
                  isCompact ? "size-6" : "size-7",
                  isCancelling && "opacity-100 sm:opacity-100",
                )}
                onClick={(event) => {
                  event.stopPropagation();
                  if (!isBusy) {
                    onCancelMessage?.(message.id);
                  }
                }}
              >
                {isCancelling ? <Loader2Icon className="size-3.5 animate-spin" /> : <XIcon className="size-3.5" />}
              </button>
            ) : null}
          </div>
        ) : null}
      <div className={cn("flex items-start", isCompact ? "gap-1.5" : "gap-2")}>
        <div className={cn("flex-shrink-0", isCompact ? "mt-0.5 [&_svg]:size-3" : "mt-0.5")}>{statusIcon(message)}</div>
        <div className="min-w-0 flex-1">
          <div className={cn("flex flex-wrap items-center text-app-subtle", isCompact ? "gap-1.5 text-[11px]" : "gap-2 text-xs")}>
            <span className="font-medium text-app-foreground">{kindLabel(message.kind, t)}</span>
            <span className={cn("rounded-md bg-app-surface-muted", isCompact ? "px-1 py-0.5 text-[10px]" : "px-1.5 py-0.5 text-[11px]")}>
              {statusLabel(message, t)}
            </span>
          </div>
          <p className={cn("mt-1 whitespace-pre-wrap break-words text-app-muted", isCompact ? "text-xs leading-5" : "text-sm leading-6")}>
            {displayText}
          </p>
          {shouldShowExpandedPrompt ? (
            <CompactCollapsible className={isCompact ? "mt-1.5" : "mt-2"} defaultOpen={false}>
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
                <div className={cn("whitespace-pre-wrap rounded-xl border border-app-border/25 bg-app-surface/35 text-xs text-app-muted", isCompact ? "px-2 py-1.5 leading-5" : "px-3 py-2 leading-5")}>
                  {expandedPrompt}
                </div>
              </CompactCollapsibleContent>
            </CompactCollapsible>
          ) : null}
        </div>
      </div>
    </div>
  );
}

export const RuntimeQueueTimeline = ({
  queue,
  onCancelMessage,
  cancellingMessageIds,
  onPromoteMessage,
  promotingMessageIds,
  onEditMessage,
  editingMessageIds,
  className,
  variant = "default",
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
  const isCompact = variant === "compact";

  return (
    <div className={cn(isCompact ? "space-y-1.5" : "space-y-2", className)} {...props}>
      <Collapsible defaultOpen>
        <CollapsibleTrigger className={cn("group flex w-full items-center justify-between rounded-md text-xs text-muted-foreground transition-colors hover:text-foreground", isCompact ? "gap-1.5 py-0.5" : "gap-2")}>
          <div className={cn("flex items-center", isCompact ? "gap-1.5" : "gap-2")}>
            <ListPlusIcon className={isCompact ? "size-2.5" : "size-3"} />
            <span>{t("queue.title")}</span>
            <span className={cn("rounded-md bg-app-surface-muted text-app-subtle", isCompact ? "px-1 py-0.5 text-[10px]" : "px-1.5 py-0.5 text-[11px]")}>
              {t("queue.pendingCount", { count: pendingCount })}
            </span>
            {queue.isDeferringSteering ? (
              <span className={cn("rounded-md bg-app-warning/10 text-app-warning", isCompact ? "px-1 py-0.5 text-[10px]" : "px-1.5 py-0.5 text-[11px]")}>
                {t("queue.deferringSteer")}
              </span>
            ) : null}
          </div>
          <ChevronDownIcon className="size-4 transition-transform group-data-[state=open]:rotate-180" />
        </CollapsibleTrigger>
        <CollapsibleContent className={cn("outline-none data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 data-[state=closed]:animate-out data-[state=open]:animate-in", isCompact ? "space-y-2 pt-1.5" : "space-y-3 pt-2")}>
          {steeringMessages.length > 0 ? (
            <div className={cn("flex flex-col", isCompact ? "gap-1.5" : "gap-2")}>
              <div className={cn("font-medium uppercase tracking-wide text-app-subtle", isCompact ? "text-[10px]" : "text-[11px]")}>
                {t("queue.steeringQueued", { count: steeringMessages.length })}
              </div>
              {steeringMessages.map((message) => (
                <RuntimeQueueMessageCard
                  key={message.id}
                  message={message}
                  t={t}
                  onCancelMessage={onCancelMessage}
                  isCancelling={cancellingMessageIds?.has(message.id)}
                  onPromoteMessage={onPromoteMessage}
                  isPromoting={promotingMessageIds?.has(message.id)}
                  onEditMessage={onEditMessage}
                  isEditing={editingMessageIds?.has(message.id)}
                  variant={variant}
                />
              ))}
            </div>
          ) : null}
          {followUpMessages.length > 0 ? (
            <div className={cn("flex flex-col", isCompact ? "gap-1.5" : "gap-2")}>
              <div className={cn("font-medium uppercase tracking-wide text-app-subtle", isCompact ? "text-[10px]" : "text-[11px]")}>
                {t("queue.followUpQueued", { count: followUpMessages.length })}
              </div>
              {followUpMessages.map((message) => (
                <RuntimeQueueMessageCard
                  key={message.id}
                  message={message}
                  t={t}
                  onCancelMessage={onCancelMessage}
                  isCancelling={cancellingMessageIds?.has(message.id)}
                  onPromoteMessage={onPromoteMessage}
                  isPromoting={promotingMessageIds?.has(message.id)}
                  onEditMessage={onEditMessage}
                  isEditing={editingMessageIds?.has(message.id)}
                  variant={variant}
                />
              ))}
            </div>
          ) : null}
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
};
