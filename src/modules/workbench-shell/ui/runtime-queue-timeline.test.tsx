import { renderToStaticMarkup } from "react-dom/server";
import type { ComponentProps, ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";

import { LanguageProvider } from "@/app/providers/language-provider";
import type { RuntimeQueueMessageDto, RuntimeQueueSnapshotDto } from "@/shared/types/api";
import { RuntimeQueueMessageCard, RuntimeQueueTimeline } from "./runtime-queue-timeline";

function message(overrides: Partial<RuntimeQueueMessageDto> = {}): RuntimeQueueMessageDto {
  return {
    id: "queue-message-1",
    kind: "steer",
    content: "Expanded prompt sent to the model",
    metadata: null,
    status: "pending",
    createdAt: "2026-05-20T00:00:00Z",
    updatedAt: "2026-05-20T00:00:00Z",
    ...overrides,
  };
}

function queue(messages: RuntimeQueueMessageDto[]): RuntimeQueueSnapshotDto {
  return {
    steeringDepth: messages.filter((entry) => entry.kind === "steer").length,
    followUpDepth: messages.filter((entry) => entry.kind === "follow_up").length,
    isDeferringSteering: false,
    messages,
    events: [],
  };
}

function renderQueue(
  messages: RuntimeQueueMessageDto[],
  props: Partial<ComponentProps<typeof RuntimeQueueTimeline>> = {},
) {
  return renderToStaticMarkup(
    <LanguageProvider>
      <RuntimeQueueTimeline queue={queue(messages)} {...props} />
    </LanguageProvider>,
  );
}

function t(key: string) {
  const labels: Record<string, string> = {
    "queue.cancelMessage": "Cancel queued message",
    "queue.cancellingMessage": "Cancelling queued message",
    "queue.followUp": "Follow-up",
    "queue.steer": "Steering",
    "queue.status.cancelled": "Cancelled",
    "queue.status.cleared": "Cleared",
    "queue.status.consumed": "Consumed",
    "queue.status.pending": "Pending",
  };
  return labels[key] ?? key;
}

function findButton(element: ReactElement): ReactElement<ComponentProps<"button">> {
  const found = findButtonOrNull(element);
  if (!found) throw new Error("button not found");
  return found;
}

function findButtonOrNull(element: ReactElement): ReactElement<ComponentProps<"button">> | null {
  if (element.type === "button") {
    return element as ReactElement<ComponentProps<"button">>;
  }

  const props = element.props as { children?: unknown };
  const children = Array.isArray(props.children) ? props.children : [props.children];

  for (const child of children) {
    if (!child || typeof child !== "object" || !("type" in child)) continue;
    const nested = findButtonOrNull(child as ReactElement);
    if (nested) return nested;
  }

  return null;
}

describe("RuntimeQueueTimeline", () => {
  it("renders slash command display text and folds the expanded prompt", () => {
    const html = renderQueue([
      message({
        content: "Generate or update a file named AGENTS.md with repository instructions.",
        metadata: {
          composer: {
            kind: "command",
            displayText: "  /init  ",
            effectivePrompt: "Generate or update a file named AGENTS.md with repository instructions.",
          },
        },
      }),
    ]);

    expect(html).toContain("/init");
    expect(html).toContain("Expanded prompt");
    expect(html).toContain("Generate or update a file named AGENTS.md");
  });

  it("renders ordinary queued messages without an expanded prompt section", () => {
    const html = renderQueue([
      message({
        content: "Continue checking edge cases.",
        metadata: null,
      }),
    ]);

    expect(html).toContain("Continue checking edge cases.");
    expect(html).not.toContain("Expanded prompt");
  });

  it("falls back to content when command metadata does not provide display text", () => {
    const html = renderQueue([
      message({
        content: "Model prompt only",
        metadata: {
          composer: {
            kind: "command",
            effectivePrompt: "Model prompt only",
          },
        },
      }),
    ]);

    expect(html).toContain("Model prompt only");
    expect(html).not.toContain("Expanded prompt");
  });

  it("uses visible pending messages for grouped counts when backend depth is stale", () => {
    const queueWithStaleDepth: RuntimeQueueSnapshotDto = {
      ...queue([message({ kind: "steer", status: "pending" })]),
      steeringDepth: 0,
    };

    const html = renderToStaticMarkup(
      <LanguageProvider>
        <RuntimeQueueTimeline queue={queueWithStaleDepth} />
      </LanguageProvider>,
    );

    expect(html).toMatch(/Steering|引导/);
    expect(html).toMatch(/1 (queued|个排队)/);
    expect(html).not.toMatch(/0 (queued|个排队)/);
  });

  it("renders a cancel action only for pending messages when a handler is provided", () => {
    const pendingHtml = renderQueue([
      message({ id: "pending-message", status: "pending" }),
    ], {
      onCancelMessage: () => undefined,
    });
    const consumedHtml = renderQueue([
      message({ id: "consumed-message", status: "consumed" }),
    ], {
      onCancelMessage: () => undefined,
    });
    const noHandlerHtml = renderQueue([
      message({ id: "pending-without-handler", status: "pending" }),
    ]);

    expect(pendingHtml).toMatch(/Cancel queued message|取消排队消息/);
    expect(consumedHtml).not.toMatch(/Cancel queued message|取消排队消息/);
    expect(noHandlerHtml).not.toMatch(/Cancel queued message|取消排队消息/);
  });

  it("invokes the cancel handler with the message id and stops event propagation", () => {
    const onCancelMessage = vi.fn();
    const stopPropagation = vi.fn();
    const card = RuntimeQueueMessageCard({
      message: message({ id: "pending-message", status: "pending" }),
      t,
      onCancelMessage,
    });

    const button = findButton(card);
    button.props.onClick?.({ stopPropagation } as never);

    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(onCancelMessage).toHaveBeenCalledWith("pending-message");
  });

  it("disables the cancel button and ignores clicks while cancellation is in progress", () => {
    const onCancelMessage = vi.fn();
    const stopPropagation = vi.fn();
    const card = RuntimeQueueMessageCard({
      message: message({ id: "pending-message", status: "pending" }),
      t,
      onCancelMessage,
      isCancelling: true,
    });

    const button = findButton(card);
    button.props.onClick?.({ stopPropagation } as never);

    expect(button.props.disabled).toBe(true);
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(onCancelMessage).not.toHaveBeenCalled();
  });
});
