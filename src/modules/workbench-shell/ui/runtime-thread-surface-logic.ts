import type { RunState } from "@/services/thread-stream";
import type { MessageDto, MessagePartDto, ThreadSnapshotDto } from "@/shared/types/api";

export type RuntimeSurfaceToolState =
  | "approval-requested"
  | "approval-responded"
  | "clarify-requested"
  | "input-streaming"
  | "input-available"
  | "output-available"
  | "output-denied"
  | "output-error";

export type RuntimeSurfaceMessagePreviewInput = Pick<
  MessageDto,
  "messageType"
> & {
  content: string;
  parts?: MessagePartDto[] | Array<{ type: string; [key: string]: unknown }>;
  status: "streaming" | "completed" | "failed" | "discarded";
};

const LONG_MESSAGE_PREVIEW_CHAR_LIMIT = 12_000;
const LONG_MESSAGE_PREVIEW_LINE_LIMIT = 120;

export function isCompletedToolState(state: RuntimeSurfaceToolState) {
  return (
    state === "output-available"
    || state === "output-denied"
    || state === "output-error"
  );
}

export function mapSnapshotToRunState(snapshot: ThreadSnapshotDto): RunState {
  if (snapshot.activeRun) {
    switch (snapshot.activeRun.status) {
      case "waiting_approval":
        return "waiting_approval";
      case "needs_reply":
        return "needs_reply";
      case "created":
      case "dispatching":
      case "running":
      case "waiting_tool_result":
        return "running";
      case "cancelling":
        return "cancelled";
      case "failed":
      case "denied":
        return "failed";
      case "cancelled":
        return "cancelled";
      case "limit_reached":
        return "limit_reached";
      case "interrupted":
        return "interrupted";
      default:
        return "completed";
    }
  }

  switch (snapshot.thread.status) {
    case "running":
      return "running";
    case "waiting_approval":
      return "waiting_approval";
    case "needs_reply":
      return snapshot.latestRun?.status === "limit_reached" ? "limit_reached" : "needs_reply";
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
    default:
      return "completed";
  }
}

export function getLongMessagePreview(content: string) {
  const normalized = content.trimEnd();
  const lines = normalized.split("\n");
  const exceedsCharLimit = normalized.length > LONG_MESSAGE_PREVIEW_CHAR_LIMIT;
  const exceedsLineLimit = lines.length > LONG_MESSAGE_PREVIEW_LINE_LIMIT;
  const isLong = exceedsCharLimit || exceedsLineLimit;

  if (!isLong) {
    return {
      hiddenLineCount: 0,
      isLong: false,
      previewText: normalized,
    };
  }

  const limitedByLines = lines.slice(0, LONG_MESSAGE_PREVIEW_LINE_LIMIT).join("\n");
  const previewText = exceedsCharLimit
    ? limitedByLines.slice(0, LONG_MESSAGE_PREVIEW_CHAR_LIMIT)
    : limitedByLines;
  const previewLineCount = previewText.length === 0 ? 0 : previewText.split("\n").length;

  return {
    hiddenLineCount: Math.max(lines.length - previewLineCount, 0),
    isLong: true,
    previewText: previewText.trimEnd(),
  };
}

export function shouldUseLongMessagePreview(message: RuntimeSurfaceMessagePreviewInput) {
  if (message.status !== "completed" && message.status !== "failed") {
    return false;
  }

  if (message.messageType !== "plain_message") {
    return false;
  }

  return true;
}

export function isTaskBoardTool(toolName: string) {
  return (
    toolName === "create_task"
    || toolName === "update_task"
    || toolName === "query_task"
  );
}

function isDefaultCollapsedTool(toolName: string) {
  return isTaskBoardTool(toolName) || toolName === "render";
}

export function getDefaultToolOpenState(
  toolName: string,
  toolState: RuntimeSurfaceToolState,
  explicitOpen: boolean | undefined,
): boolean {
  if (isDefaultCollapsedTool(toolName)) {
    return explicitOpen ?? false;
  }
  if (!isCompletedToolState(toolState)) {
    return true;
  }
  return explicitOpen ?? true;
}
