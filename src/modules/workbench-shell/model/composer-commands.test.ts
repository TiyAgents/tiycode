import { describe, expect, it } from "vitest";
import type { PromptInputMessage } from "@/components/ai-elements/prompt-input";
import type { RunMode } from "@/shared/types/api";
import {
  buildCommandEffectivePrompt,
  buildComposerCommandRegistry,
  buildComposerSubmission,
  parseSlashCommandInput,
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

describe("parseSlashCommandInput", () => {
  const registry = buildComposerCommandRegistry([]);

  // --- LARGE_TEXT_THRESHOLD boundary (10_240) ---

  it("returns null for input exceeding LARGE_TEXT_THRESHOLD", () => {
    const value = "/" + "a".repeat(10_241);
    expect(parseSlashCommandInput(value, registry)).toBeNull();
  });

  it("parses input at exactly LARGE_TEXT_THRESHOLD length", () => {
    // 10_240 chars: "/" + 10_239 padding — should NOT be short-circuited (> not >=)
    const value = "/init" + " ".repeat(10_240 - 5);
    expect(value.length).toBe(10_240);
    const result = parseSlashCommandInput(value, registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
  });

  it("parses input one char below LARGE_TEXT_THRESHOLD", () => {
    const value = "/init" + " ".repeat(10_239 - 5);
    expect(value.length).toBe(10_239);
    const result = parseSlashCommandInput(value, registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
  });

  // --- Leading whitespace handling ---

  it("skips leading spaces before slash", () => {
    const result = parseSlashCommandInput("   /init", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
  });

  it("skips leading tabs before slash", () => {
    const result = parseSlashCommandInput("\t\t/init", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
  });

  it("skips leading newlines and carriage returns before slash", () => {
    const result = parseSlashCommandInput("\n\r\n/init", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
  });

  it("skips mixed whitespace (space, tab, LF, CR) before slash", () => {
    const result = parseSlashCommandInput(" \t\n\r /init", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
  });

  // --- Non-slash inputs ---

  it("returns null for empty string", () => {
    expect(parseSlashCommandInput("", registry)).toBeNull();
  });

  it("returns null for whitespace-only string", () => {
    expect(parseSlashCommandInput("   \t\n  ", registry)).toBeNull();
  });

  it("returns null when first non-whitespace is not a slash", () => {
    expect(parseSlashCommandInput("  hello /init", registry)).toBeNull();
  });

  // --- Multi-line inputs ---

  it("only considers the first line for command detection", () => {
    const result = parseSlashCommandInput("/init\nsecond line content", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
    expect(result?.argumentsText).toBe("");
  });

  // --- Command matching and arguments ---

  it("returns null command when query does not match any registry entry", () => {
    const result = parseSlashCommandInput("/nonexistent", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("nonexistent");
    expect(result?.command).toBeNull();
  });

  it("extracts arguments after the command name", () => {
    const result = parseSlashCommandInput("/init  some arguments here", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("init");
    expect(result?.argumentsText).toBe("some arguments here");
  });

  it("handles slash with no command name", () => {
    const result = parseSlashCommandInput("/", registry);
    expect(result).not.toBeNull();
    expect(result?.query).toBe("");
  });
});

describe("buildCommandEffectivePrompt", () => {
  const makeCommand = (prompt: string, name = "test-cmd") => ({
    name,
    prompt,
    source: "settings" as const,
    path: `/prompts:${name}`,
    description: "Test command",
    argumentHint: "",
    behavior: "prompt" as const,
    smartSend: "always" as const,
  });

  it("appends arguments when template has no {{arguments}} placeholder", () => {
    const cmd = makeCommand("Do something useful for the user.");
    const result = buildCommandEffectivePrompt(cmd, "--style=full");
    expect(result).toBe("Do something useful for the user.\n\nCommand arguments: --style=full");
  });

  it("uses placeholder substitution when {{arguments}} exists", () => {
    const cmd = makeCommand("Run with args: {{arguments}} now.");
    const result = buildCommandEffectivePrompt(cmd, "--draft");
    expect(result).toBe("Run with args: --draft now.");
    expect(result).not.toContain("Command arguments:");
  });

  it("does not append when argumentsText is empty", () => {
    const cmd = makeCommand("Do something.");
    const result = buildCommandEffectivePrompt(cmd, "");
    expect(result).toBe("Do something.");
    expect(result).not.toContain("Command arguments:");
  });

  it("does not append when argumentsText is whitespace only", () => {
    const cmd = makeCommand("Do something.");
    const result = buildCommandEffectivePrompt(cmd, "   \t  ");
    expect(result).toBe("Do something.");
    expect(result).not.toContain("Command arguments:");
  });

  it("replaces {{command}} placeholder with command name", () => {
    const cmd = makeCommand("Running {{command}} command.", "my-cmd");
    const result = buildCommandEffectivePrompt(cmd, "");
    expect(result).toBe("Running my-cmd command.");
  });
});
