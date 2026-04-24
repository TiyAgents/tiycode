import { describe, it, expect } from "vitest";
import {
  isReservedBuiltinCommandName,
  buildComposerCommandRegistry,
  parseSlashCommandInput,
  filterComposerCommands,
  shouldSmartSendCommand,
  buildCommandEffectivePrompt,
  buildComposerSubmission,
  SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS,
  SUPPORTED_COMPOSER_ATTACHMENT_ACCEPT,
  SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS,
} from "@/modules/workbench-shell/model/composer-commands";
import type { CommandEntry } from "@/modules/settings-center/model/types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeCommandEntry(overrides: Partial<CommandEntry> = {}): CommandEntry {
  return {
    id: "cmd-1",
    name: "test",
    path: "/test",
    argumentHint: "",
    description: "Test command",
    prompt: "Do a test",
    ...overrides,
  };
}

function makeMessage(text: string, files: Array<{ filename?: string; url?: string; mediaType?: string }> = []) {
  return {
    text,
    files: files.map((f) => ({
      filename: f.filename ?? "file.txt",
      url: f.url ?? "blob:http://test/abc",
      mediaType: f.mediaType ?? "text/plain",
    })),
  };
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

describe("SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS", () => {
  it("contains common extensions", () => {
    expect(SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS).toContain(".ts");
    expect(SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS).toContain(".py");
    expect(SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS).toContain(".rs");
    expect(SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS).toContain(".md");
  });
});

describe("SUPPORTED_COMPOSER_ATTACHMENT_ACCEPT", () => {
  it("starts with image/* and includes extensions", () => {
    expect(SUPPORTED_COMPOSER_ATTACHMENT_ACCEPT).toMatch(/^image\/\*/);
    expect(SUPPORTED_COMPOSER_ATTACHMENT_ACCEPT).toContain(".ts");
  });
});

describe("SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS", () => {
  it("has three filter groups", () => {
    expect(SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS).toHaveLength(3);
    expect(SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS[0].name).toBe("Images");
    expect(SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS[1].name).toBe("Source files");
    expect(SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS[2].name).toBe("Config files");
  });
});

// ---------------------------------------------------------------------------
// isReservedBuiltinCommandName
// ---------------------------------------------------------------------------

describe("isReservedBuiltinCommandName", () => {
  it("returns true for builtin names", () => {
    expect(isReservedBuiltinCommandName("init")).toBe(true);
    expect(isReservedBuiltinCommandName("clear")).toBe(true);
    expect(isReservedBuiltinCommandName("compact")).toBe(true);
  });

  it("is case-insensitive", () => {
    expect(isReservedBuiltinCommandName("INIT")).toBe(true);
    expect(isReservedBuiltinCommandName("Clear")).toBe(true);
  });

  it("trims and strips slashes", () => {
    expect(isReservedBuiltinCommandName("  /init  ")).toBe(true);
    expect(isReservedBuiltinCommandName("///clear")).toBe(true);
  });

  it("returns false for non-builtin names", () => {
    expect(isReservedBuiltinCommandName("custom")).toBe(false);
    expect(isReservedBuiltinCommandName("")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// buildComposerCommandRegistry
// ---------------------------------------------------------------------------

describe("buildComposerCommandRegistry", () => {
  it("includes builtin commands", () => {
    const registry = buildComposerCommandRegistry([]);
    const names = registry.map((c) => c.name);
    expect(names).toContain("init");
    expect(names).toContain("clear");
    expect(names).toContain("compact");
  });

  it("appends custom settings commands", () => {
    const custom = makeCommandEntry({ name: "deploy", path: "/deploy", prompt: "Deploy now" });
    const registry = buildComposerCommandRegistry([custom]);
    expect(registry.some((c) => c.name === "deploy")).toBe(true);
    expect(registry.find((c) => c.name === "deploy")?.source).toBe("settings");
  });

  it("filters out reserved names", () => {
    const custom = makeCommandEntry({ name: "init", path: "/init", prompt: "My init" });
    const registry = buildComposerCommandRegistry([custom]);
    const initCommands = registry.filter((c) => c.name === "init");
    expect(initCommands).toHaveLength(1);
    expect(initCommands[0].source).toBe("builtin");
  });

  it("filters out empty names", () => {
    const custom = makeCommandEntry({ name: "", path: "/empty" });
    const registry = buildComposerCommandRegistry([custom]);
    expect(registry.every((c) => c.name !== "")).toBe(true);
  });

  it("normalizes custom command names to lowercase", () => {
    const custom = makeCommandEntry({ name: "Deploy", path: "/Deploy" });
    const registry = buildComposerCommandRegistry([custom]);
    expect(registry.some((c) => c.name === "deploy")).toBe(true);
  });

  it("sets smartSend to 'never' when argumentHint is present", () => {
    const custom = makeCommandEntry({ name: "mycommand", argumentHint: "<target>" });
    const registry = buildComposerCommandRegistry([custom]);
    const cmd = registry.find((c) => c.name === "mycommand");
    expect(cmd?.smartSend).toBe("never");
  });

  it("sets smartSend to 'always' when argumentHint is empty", () => {
    const custom = makeCommandEntry({ name: "mycommand", argumentHint: "" });
    const registry = buildComposerCommandRegistry([custom]);
    const cmd = registry.find((c) => c.name === "mycommand");
    expect(cmd?.smartSend).toBe("always");
  });
});

// ---------------------------------------------------------------------------
// parseSlashCommandInput
// ---------------------------------------------------------------------------

describe("parseSlashCommandInput", () => {
  const registry = buildComposerCommandRegistry([
    makeCommandEntry({ name: "deploy", path: "/deploy", prompt: "Deploy now", argumentHint: "<env>" }),
  ]);

  it("returns null for non-slash input", () => {
    expect(parseSlashCommandInput("hello world", registry)).toBeNull();
  });

  it("returns null for empty input", () => {
    expect(parseSlashCommandInput("", registry)).toBeNull();
  });

  it("parses builtin command", () => {
    const result = parseSlashCommandInput("/init", registry);
    expect(result).not.toBeNull();
    expect(result!.command?.name).toBe("init");
    expect(result!.query).toBe("init");
    expect(result!.argumentsText).toBe("");
  });

  it("parses custom command with path-based trigger", () => {
    // Custom commands use normalizeCommandName(path) as trigger, so /prompts:deploy -> prompts:deploy
    const result = parseSlashCommandInput("/prompts:deploy staging", registry);
    expect(result).not.toBeNull();
    expect(result!.command?.name).toBe("deploy");
    expect(result!.argumentsText).toBe("staging");
  });

  it("returns null command for unknown slash", () => {
    const result = parseSlashCommandInput("/unknown", registry);
    expect(result).not.toBeNull();
    expect(result!.command).toBeNull();
    expect(result!.query).toBe("unknown");
  });

  it("handles leading whitespace", () => {
    const result = parseSlashCommandInput("   /init", registry);
    expect(result).not.toBeNull();
    expect(result!.command?.name).toBe("init");
  });

  it("handles multiline input (only first line matters)", () => {
    const result = parseSlashCommandInput("/init\nsome extra text", registry);
    expect(result).not.toBeNull();
    expect(result!.command?.name).toBe("init");
    expect(result!.argumentsText).toBe("");
  });
});

// ---------------------------------------------------------------------------
// filterComposerCommands
// ---------------------------------------------------------------------------

describe("filterComposerCommands", () => {
  const registry = buildComposerCommandRegistry([
    makeCommandEntry({ name: "deploy", path: "/deploy", description: "Deploy to production" }),
    makeCommandEntry({ name: "lint", path: "/lint", description: "Run linter" }),
  ]);

  it("returns all commands for empty query", () => {
    expect(filterComposerCommands(registry, "")).toEqual(registry);
    expect(filterComposerCommands(registry, "  ")).toEqual(registry);
  });

  it("filters by name substring", () => {
    const result = filterComposerCommands(registry, "dep");
    expect(result.some((c) => c.name === "deploy")).toBe(true);
    expect(result.some((c) => c.name === "lint")).toBe(false);
  });

  it("filters by description substring", () => {
    const result = filterComposerCommands(registry, "production");
    expect(result.some((c) => c.name === "deploy")).toBe(true);
  });

  it("is case-insensitive", () => {
    const result = filterComposerCommands(registry, "DEPLOY");
    expect(result.some((c) => c.name === "deploy")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// shouldSmartSendCommand
// ---------------------------------------------------------------------------

describe("shouldSmartSendCommand", () => {
  it("returns true when smartSend is 'always'", () => {
    const registry = buildComposerCommandRegistry([]);
    const initCmd = registry.find((c) => c.name === "init")!;
    expect(shouldSmartSendCommand(initCmd, "")).toBe(true);
  });

  it("returns false when smartSend is 'never'", () => {
    const registry = buildComposerCommandRegistry([
      makeCommandEntry({ name: "ask", argumentHint: "<question>" }),
    ]);
    const askCmd = registry.find((c) => c.name === "ask")!;
    expect(shouldSmartSendCommand(askCmd, "some args")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// buildCommandEffectivePrompt
// ---------------------------------------------------------------------------

describe("buildCommandEffectivePrompt", () => {
  it("replaces {{arguments}} template variable", () => {
    const registry = buildComposerCommandRegistry([
      makeCommandEntry({ name: "ask", prompt: "Please answer: {{arguments}}" }),
    ]);
    const cmd = registry.find((c) => c.name === "ask")!;
    const result = buildCommandEffectivePrompt(cmd, "What is 1+1?");
    expect(result).toBe("Please answer: What is 1+1?");
  });

  it("replaces {{command}} template variable", () => {
    const registry = buildComposerCommandRegistry([
      makeCommandEntry({ name: "foo", prompt: "Running {{command}}" }),
    ]);
    const cmd = registry.find((c) => c.name === "foo")!;
    const result = buildCommandEffectivePrompt(cmd, "");
    expect(result).toBe("Running foo");
  });

  it("returns prompt as-is when no template vars", () => {
    const registry = buildComposerCommandRegistry([]);
    const initCmd = registry.find((c) => c.name === "init")!;
    const result = buildCommandEffectivePrompt(initCmd, "");
    expect(result).toBe(initCmd.prompt.trim());
  });

  it("trims result", () => {
    const registry = buildComposerCommandRegistry([
      makeCommandEntry({ name: "trimtest", prompt: "  hello {{arguments}}  " }),
    ]);
    const cmd = registry.find((c) => c.name === "trimtest")!;
    const result = buildCommandEffectivePrompt(cmd, "  ");
    expect(result).toBe("hello");
  });
});

// ---------------------------------------------------------------------------
// buildComposerSubmission
// ---------------------------------------------------------------------------

describe("buildComposerSubmission", () => {
  const registry = buildComposerCommandRegistry([
    makeCommandEntry({ name: "deploy", prompt: "Deploy {{arguments}}" }),
  ]);

  it("returns null for empty text and no files", () => {
    const result = buildComposerSubmission(makeMessage("", []), registry);
    expect(result).toBeNull();
  });

  it("returns null for whitespace-only text", () => {
    const result = buildComposerSubmission(makeMessage("   ", []), registry);
    expect(result).toBeNull();
  });

  it("builds plain submission for non-slash text", () => {
    const result = buildComposerSubmission(makeMessage("Hello world"), registry);
    expect(result).not.toBeNull();
    expect(result!.kind).toBe("plain");
    expect(result!.displayText).toBe("Hello world");
    expect(result!.effectivePrompt).toBe("Hello world");
  });

  it("builds command submission for slash input", () => {
    // Custom commands are triggered via their path-based trigger: /prompts:deploy
    const result = buildComposerSubmission(makeMessage("/prompts:deploy staging"), registry);
    expect(result).not.toBeNull();
    expect(result!.kind).toBe("command");
    expect(result!.command?.name).toBe("deploy");
    expect(result!.command?.argumentsText).toBe("staging");
    expect(result!.effectivePrompt).toBe("Deploy staging");
  });

  it("falls back to plain when slash command is unknown", () => {
    const result = buildComposerSubmission(makeMessage("/unknown arg"), registry);
    expect(result).not.toBeNull();
    expect(result!.kind).toBe("plain");
  });

  it("builds plain submission when only files are present", () => {
    const result = buildComposerSubmission(
      makeMessage("", [{ filename: "test.png", mediaType: "image/png" }]),
      registry,
    );
    expect(result).not.toBeNull();
    expect(result!.kind).toBe("plain");
    expect(result!.attachments).toHaveLength(1);
  });

  it("maps attachments correctly", () => {
    const result = buildComposerSubmission(
      makeMessage("hello", [
        { filename: "a.png", url: "blob:1", mediaType: "image/png" },
        { filename: "b.txt", url: "blob:2", mediaType: "text/plain" },
      ]),
      registry,
    );
    expect(result!.attachments).toHaveLength(2);
    expect(result!.attachments[0].name).toBe("a.png");
    expect(result!.attachments[1].mediaType).toBe("text/plain");
  });

  it("passes runMode through", () => {
    const result = buildComposerSubmission(makeMessage("hello"), registry, "plan");
    expect(result!.runMode).toBe("plan");
  });

  it("includes metadata for command submissions", () => {
    // Use builtin command for guaranteed match
    const result = buildComposerSubmission(makeMessage("/init"), registry);
    expect(result!.kind).toBe("command");
    expect(result!.metadata).not.toBeNull();
    expect((result!.metadata as Record<string, unknown>).composer).toBeDefined();
  });
});
