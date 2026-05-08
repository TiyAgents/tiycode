import { memo, useMemo, useState } from "react";
import { ChevronDownIcon, ChevronUpIcon } from "lucide-react";
import type { useT } from "@/i18n";
import { MessageResponse } from "@/components/ai-elements/message";
import { Button } from "@/shared/ui/button";
import {
  getLongMessagePreview,
  shouldUseLongMessagePreview,
  type RuntimeSurfaceMessagePreviewInput,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-logic";
import type {
  SurfaceChartMessagePart,
  SurfaceDataMessagePart,
  SurfaceMessagePart,
  SurfaceTextMessagePart,
  SurfaceUnknownMessagePart,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-state";
import { ThreadChartArtifactCard } from "@/modules/workbench-shell/ui/thread-chart-artifact-card";

type LongMessageBodyProps = {
  message: RuntimeSurfaceMessagePreviewInput & { id: string; parts?: SurfaceMessagePart[] };
  t: ReturnType<typeof useT>;
};

function isTextPart(part: SurfaceMessagePart): part is SurfaceTextMessagePart {
  return part.type === "text";
}

function isChartPart(part: SurfaceMessagePart): part is SurfaceChartMessagePart {
  return part.type === "chart";
}

function isDataPart(part: SurfaceMessagePart): part is SurfaceDataMessagePart {
  return part.type.startsWith("data-") && "data" in part;
}

function isUnknownPart(part: SurfaceMessagePart): part is SurfaceUnknownMessagePart {
  return !isTextPart(part) && !isChartPart(part) && !isDataPart(part);
}

function renderMessagePart(part: SurfaceMessagePart, key: string) {
  if (isTextPart(part)) {
    return <MessageResponse key={key}>{part.text}</MessageResponse>;
  }

  if (isChartPart(part)) {
    return <ThreadChartArtifactCard key={key} part={part} />;
  }

  if (isDataPart(part)) {
    return (
      <div className="rounded-xl border border-app-border/25 bg-app-surface/25 px-3 py-3" key={key}>
        <div className="text-xs font-semibold uppercase tracking-[0.08em] text-app-subtle">{part.type}</div>
        <div className="mt-2 text-sm text-app-muted">
          <MessageResponse>{`\`\`\`json\n${JSON.stringify(part.data, null, 2)}\n\`\`\``}</MessageResponse>
        </div>
      </div>
    );
  }

  if (isUnknownPart(part)) {
    return (
      <div className="rounded-xl border border-app-border/25 bg-app-surface/25 px-3 py-3" key={key}>
        <div className="text-xs font-semibold uppercase tracking-[0.08em] text-app-subtle">Unsupported part</div>
        <div className="mt-2 text-sm text-app-muted">
          <MessageResponse>{`\`\`\`json\n${JSON.stringify(part.value, null, 2)}\n\`\`\``}</MessageResponse>
        </div>
      </div>
    );
  }

  return null;
}

/**
 * Body renderer for a single message that may need the long-message preview UI.
 * Kept as a memoized child component so toggling one message does not force all
 * other thread messages to recompute their previews.
 */
export const LongMessageBody = memo(function LongMessageBody({
  message,
  t,
}: LongMessageBodyProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const parts: SurfaceMessagePart[] = message.parts ?? [{ type: "text", text: message.content || (message.status === "streaming" ? "…" : "") }];
  const hasNonTextParts = parts.some((part) => !isTextPart(part));
  const textContent = useMemo(
    () => parts.filter(isTextPart).map((part) => part.text).join("\n\n") || (message.status === "streaming" ? "…" : ""),
    [message.status, parts],
  );
  const preview = !hasNonTextParts && shouldUseLongMessagePreview(message)
    ? getLongMessagePreview(textContent)
    : null;
  const canPreview = preview !== null && preview.isLong;
  const usePreview = canPreview && !isExpanded;

  if (hasNonTextParts) {
    return <div className="space-y-3">{parts.map((part, index) => renderMessagePart(part, `${message.id}-part-${index}`))}</div>;
  }

  if (!usePreview) {
    return (
      <div className="space-y-3">
        {parts.map((part, index) => renderMessagePart(part, `${message.id}-part-${index}`))}
        {canPreview ? (
          <div className="flex justify-end">
            <Button
              className="h-7 px-2.5 text-xs"
              onClick={() => setIsExpanded(false)}
              size="sm"
              type="button"
              variant="ghost"
            >
              <ChevronUpIcon className="size-3.5" />
              {t("longMessage.collapseAll")}
            </Button>
          </div>
        ) : null}
      </div>
    );
  }

  return (
    <div className="space-y-3 rounded-2xl border border-app-border/18 bg-app-surface/14 px-4 py-3">
      <pre className="max-h-[28rem] overflow-hidden whitespace-pre-wrap break-words text-sm leading-6 text-app-foreground">
        {preview!.previewText}
        {preview!.previewText.length < textContent.length ? "\n…" : ""}
      </pre>
      <div className="flex items-center justify-between gap-3 text-xs text-app-subtle">
        <span>
          {preview!.hiddenLineCount > 0
            ? t("longMessage.hiddenLines", { count: preview!.hiddenLineCount })
            : t("longMessage.hiddenContent")}
        </span>
        <Button
          className="h-7 px-2.5 text-xs"
          onClick={() => setIsExpanded(true)}
          size="sm"
          type="button"
          variant="outline"
        >
          <ChevronDownIcon className="size-3.5" />
          {t("longMessage.expandAll")}
        </Button>
      </div>
    </div>
  );
});
