import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { LanguageProvider } from "@/app/providers/language-provider";
import type { RuntimeQueueMessageDto, RuntimeQueueSnapshotDto } from "@/shared/types/api";
import { RuntimeQueueTimeline } from "./runtime-queue-timeline";

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

function renderQueue(messages: RuntimeQueueMessageDto[]) {
  return renderToStaticMarkup(
    <LanguageProvider>
      <RuntimeQueueTimeline queue={queue(messages)} />
    </LanguageProvider>,
  );
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
});
