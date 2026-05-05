import { describe, expect, it } from "vitest";
import type { PromptInputMessage } from "@/components/ai-elements/prompt-input";
import type { RunMode } from "@/shared/types/api";
import {
  buildCommandEffectivePrompt,
  buildComposerCommandRegistry,
  buildComposerSubmission,
  extractArgumentNames,
  parseCommandArguments,
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

  it("replaces named argument placeholders from --key=value", () => {
    const cmd = makeCommand("Review PR #{{pr}} on branch {{branch}}.");
    const result = buildCommandEffectivePrompt(cmd, "--pr=123 --branch=main");
    expect(result).toBe("Review PR #123 on branch main.");
  });

  it("replaces named argument placeholders from --key value format", () => {
    const cmd = makeCommand("Deploy {{env}} with tag {{tag}}.");
    const result = buildCommandEffectivePrompt(cmd, "--env production --tag v1.0");
    expect(result).toBe("Deploy production with tag v1.0.");
  });

  it("maps positional arguments to declared names via argumentHint", () => {
    const cmd = makeCommand("Review PR #{{pr}} on {{branch}}.", "review");
    cmd.argumentHint = "[pr] [branch]";
    const result = buildCommandEffectivePrompt(cmd, "123 main");
    expect(result).toBe("Review PR #123 on main.");
  });

  it("replaces indexed positional placeholders {{0}} {{1}}", () => {
    const cmd = makeCommand("First: {{0}}, Second: {{1}}.");
    const result = buildCommandEffectivePrompt(cmd, "alpha beta");
    expect(result).toBe("First: alpha, Second: beta.");
  });

  it("does not append fallback when named placeholders were consumed", () => {
    const cmd = makeCommand("PR: {{pr}}");
    const result = buildCommandEffectivePrompt(cmd, "--pr=456");
    expect(result).toBe("PR: 456");
    expect(result).not.toContain("Command arguments:");
  });

  it("appends fallback when no placeholders match the provided arguments", () => {
    const cmd = makeCommand("Do something useful.");
    const result = buildCommandEffectivePrompt(cmd, "--pr=123");
    expect(result).toBe("Do something useful.\n\nCommand arguments: --pr=123");
  });

  it("handles mixed named and positional with argumentHint mapping", () => {
    const cmd = makeCommand("File: {{file}}, Style: {{style}}.", "test");
    cmd.argumentHint = "[file] [--style=full]";
    const result = buildCommandEffectivePrompt(cmd, "readme.md --style=compact");
    expect(result).toBe("File: readme.md, Style: compact.");
  });

  it("named flags do not overwrite positional mapping when name already in named", () => {
    const cmd = makeCommand("PR: {{pr}}.", "test");
    cmd.argumentHint = "[pr]";
    // --pr=999 takes precedence over positional "123" for the "pr" key
    const result = buildCommandEffectivePrompt(cmd, "--pr=999 123");
    expect(result).toBe("PR: 999.");
  });

  it("replaces unfilled named placeholders with empty string", () => {
    const cmd = makeCommand("Value: [{{missing}}].");
    const result = buildCommandEffectivePrompt(cmd, "--other=x");
    // {{missing}} replaced with "" (no matching arg), fallback appends since {{other}} not in template
    expect(result).toBe("Value: [].\n\nCommand arguments: --other=x");
  });

  it("does not append fallback when template uses a matching named placeholder", () => {
    const cmd = makeCommand("Value: [{{other}}].");
    const result = buildCommandEffectivePrompt(cmd, "--other=x");
    expect(result).toBe("Value: [x].");
    expect(result).not.toContain("Command arguments:");
  });
});

describe("parseCommandArguments", () => {
  it("returns empty result for empty string", () => {
    const result = parseCommandArguments("");
    expect(result).toEqual({ named: {}, positional: [], raw: "" });
  });

  it("returns empty result for whitespace only", () => {
    const result = parseCommandArguments("   ");
    expect(result).toEqual({ named: {}, positional: [], raw: "" });
  });

  it("parses --key=value flags", () => {
    const result = parseCommandArguments("--pr=123 --branch=main");
    expect(result.named).toEqual({ pr: "123", branch: "main" });
    expect(result.positional).toEqual([]);
  });

  it("parses --key value flags", () => {
    const result = parseCommandArguments("--env production --tag v1.0");
    expect(result.named).toEqual({ env: "production", tag: "v1.0" });
    expect(result.positional).toEqual([]);
  });

  it("parses positional arguments", () => {
    const result = parseCommandArguments("123 main feature");
    expect(result.named).toEqual({});
    expect(result.positional).toEqual(["123", "main", "feature"]);
  });

  it("handles mixed named and positional", () => {
    const result = parseCommandArguments("readme.md --style=full --verbose");
    expect(result.named).toEqual({ style: "full", verbose: "true" });
    expect(result.positional).toEqual(["readme.md"]);
  });

  it("handles quoted values with spaces in --key=value", () => {
    const result = parseCommandArguments("--msg=\"hello world\" --tag=v1");
    expect(result.named).toEqual({ msg: "hello world", tag: "v1" });
  });

  it("handles quoted values with spaces in --key value", () => {
    const result = parseCommandArguments("--msg 'hello world' --count 5");
    expect(result.named).toEqual({ msg: "hello world", count: "5" });
  });

  it("treats --flag at end as boolean true", () => {
    const result = parseCommandArguments("--verbose --dry-run");
    expect(result.named).toEqual({ verbose: "true", "dry-run": "true" });
  });

  it("treats --flag followed by another flag as boolean true", () => {
    const result = parseCommandArguments("--verbose --name test");
    expect(result.named).toEqual({ verbose: "true", name: "test" });
  });

  it("preserves raw text", () => {
    const result = parseCommandArguments("  --pr=123  hello  ");
    expect(result.raw).toBe("--pr=123  hello");
  });

  it("treats unclosed double quotes as literal characters without swallowing following flags", () => {
    const result = parseCommandArguments("--msg \"hello --tag v1");
    expect(result.named).toEqual({ msg: "\"hello", tag: "v1" });
    expect(result.positional).toEqual([]);
  });

  it("treats unclosed single quotes as literal characters without swallowing following flags", () => {
    const result = parseCommandArguments("--msg 'hello --tag v1");
    expect(result.named).toEqual({ msg: "'hello", tag: "v1" });
    expect(result.positional).toEqual([]);
  });

  it("normalizes em-dash (—) to ASCII double-hyphen for flag detection", () => {
    const result = parseCommandArguments("\u2014style=full \u2014language=chinese");
    expect(result.named).toEqual({ style: "full", language: "chinese" });
    expect(result.positional).toEqual([]);
  });

  it("normalizes en-dash (–) to ASCII hyphen for flag detection", () => {
    // Two en-dashes (– –) each become single hyphen → "--pr=123"
    const result = parseCommandArguments("\u2013\u2013pr=123");
    expect(result.named).toEqual({ pr: "123" });
  });
});

describe("extractArgumentNames", () => {
  it("returns empty array for empty string", () => {
    expect(extractArgumentNames("")).toEqual([]);
  });

  it("extracts names from [--flag=options] format", () => {
    expect(extractArgumentNames("[--verify=yes|no] [--style=simple|full]"))
      .toEqual(["verify", "style"]);
  });

  it("extracts names from [name] format", () => {
    expect(extractArgumentNames("[pr] [branch]"))
      .toEqual(["pr", "branch"]);
  });

  it("extracts names from mixed formats", () => {
    expect(extractArgumentNames("[file] [--style=full] [--verbose]"))
      .toEqual(["file", "style", "verbose"]);
  });

  it("deduplicates names", () => {
    expect(extractArgumentNames("[--name=x] [--name=y]"))
      .toEqual(["name"]);
  });

  it("handles complex hint with types and defaults", () => {
    expect(extractArgumentNames("[--type=feat|fix|docs|style|refactor|perf|test|chore|ci|build|revert] [--language=english|chinese]"))
      .toEqual(["type", "language"]);
  });
});
