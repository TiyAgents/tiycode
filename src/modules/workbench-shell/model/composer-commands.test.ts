import { describe, expect, it } from "vitest";
import type { PromptInputMessage } from "@/components/ai-elements/prompt-input";
import type { RunMode } from "@/shared/types/api";
import {
  buildComposerCommandRegistry,
  buildComposerSubmission,
} from "@/modules/workbench-shell/model/composer-commands";

const RUN_MODE: RunMode = "default";

function createMessage(text: string): PromptInputMessage {
  return {
    text,
    files: [],
  };
}

describe("buildComposerSubmission", () => {
  it("preserves plain multi-line Markdown exactly", () => {
    const text = "  1. First\n2. Second\n\n- Bullet\n```ts\nconst value = 1;\n```\n  ";
    const submission = buildComposerSubmission(createMessage(text), [], RUN_MODE);

    expect(submission).not.toBeNull();
    expect(submission?.kind).toBe("plain");
    expect(submission?.displayText).toBe(text);
    expect(submission?.effectivePrompt).toBe(text);
  });

  it("rejects whitespace-only messages without attachments", () => {
    const submission = buildComposerSubmission(createMessage(" \n\t  "), [], RUN_MODE);

    expect(submission).toBeNull();
  });

  it("parses slash commands from trimmed text while preserving the original display text", () => {
    const registry = buildComposerCommandRegistry([]);
    const text = "  /init  \n";
    const submission = buildComposerSubmission(createMessage(text), registry, RUN_MODE);

    expect(submission).not.toBeNull();
    expect(submission?.kind).toBe("command");
    expect(submission?.displayText).toBe(text);
    expect(submission?.command?.name).toBe("init");
    expect(submission?.effectivePrompt).toContain("Generate or update a file named AGENTS.md");
  });
});
