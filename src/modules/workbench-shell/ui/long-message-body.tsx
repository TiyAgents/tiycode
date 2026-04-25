import { memo, useState } from "react";
import { ChevronDownIcon, ChevronUpIcon } from "lucide-react";
import type { useT } from "@/i18n";
import { MessageResponse } from "@/components/ai-elements/message";
import { Button } from "@/shared/ui/button";
import {
  getLongMessagePreview,
  shouldUseLongMessagePreview,
  type RuntimeSurfaceMessagePreviewInput,
} from "@/modules/workbench-shell/ui/runtime-thread-surface-logic";

type LongMessageBodyProps = {
  message: RuntimeSurfaceMessagePreviewInput;
  t: ReturnType<typeof useT>;
};

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
  const content = message.content || (message.status === "streaming" ? "…" : "");
  const preview = shouldUseLongMessagePreview(message)
    ? getLongMessagePreview(content)
    : null;
  const canPreview = preview !== null && preview.isLong;
  const usePreview = canPreview && !isExpanded;

  if (!usePreview) {
    return (
      <div className="space-y-3">
        <MessageResponse>{content}</MessageResponse>
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
        {preview!.previewText.length < content.length ? "\n…" : ""}
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
