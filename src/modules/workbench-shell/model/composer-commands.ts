import type { PromptInputMessage } from "@/components/ai-elements/prompt-input";
import type { CommandEntry } from "@/modules/settings-center/model/types";
import type { MessageAttachmentDto, RunMode } from "@/shared/types/api";

export type ComposerReferencedFile = {
  name: string;
  path: string;
  parentPath: string;
};

export const SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS = [
  ".md",
  ".txt",
  ".json",
  ".js",
  ".jsx",
  ".ts",
  ".tsx",
  ".py",
  ".go",
  ".rs",
  ".java",
  ".c",
  ".cc",
  ".cpp",
  ".cxx",
  ".h",
  ".hpp",
  ".hh",
  ".yaml",
  ".yml",
  ".toml",
  ".ini",
  ".conf",
  ".cfg",
  ".env",
  ".properties",
  ".xml",
  ".html",
  ".css",
  ".scss",
  ".less",
  ".sql",
  ".sh",
  ".bash",
  ".zsh",
] as const;

export const SUPPORTED_COMPOSER_ATTACHMENT_ACCEPT = [
  "image/*",
  ...SUPPORTED_COMPOSER_ATTACHMENT_EXTENSIONS,
].join(",");

export const SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS = [
  {
    name: "Images",
    extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"],
  },
  {
    name: "Source files",
    extensions: [
      "md",
      "txt",
      "json",
      "js",
      "jsx",
      "ts",
      "tsx",
      "py",
      "go",
      "rs",
      "java",
      "c",
      "cc",
      "cpp",
      "cxx",
      "h",
      "hpp",
      "hh",
      "sh",
      "bash",
      "zsh",
      "sql",
    ],
  },
  {
    name: "Config files",
    extensions: [
      "yaml",
      "yml",
      "toml",
      "ini",
      "conf",
      "cfg",
      "env",
      "properties",
      "xml",
      "html",
      "css",
      "scss",
      "less",
    ],
  },
] as const;

export type BuiltinComposerCommandName = "init" | "clear" | "compact";

export type ComposerCommandSource = "builtin" | "settings";
export type ComposerSubmissionKind = "plain" | "command";
export type ComposerCommandBehavior = "prompt" | "clear" | "compact";

export type ComposerCommandDescriptor = {
  source: ComposerCommandSource;
  name: string;
  path: string;
  description: string;
  argumentHint: string;
  prompt: string;
  behavior: ComposerCommandBehavior;
  smartSend: "always" | "never";
};

export type ComposerCommandInvocation = {
  source: ComposerCommandSource;
  name: string;
  path: string;
  description: string;
  argumentHint: string;
  argumentsText: string;
  prompt: string;
  behavior: ComposerCommandBehavior;
};

export type ComposerSubmission = {
  kind: ComposerSubmissionKind;
  displayText: string;
  effectivePrompt: string;
  rawMessage: PromptInputMessage;
  attachments: MessageAttachmentDto[];
  command?: ComposerCommandInvocation;
  metadata?: Record<string, unknown> | null;
  runMode?: RunMode;
};

const BUILTIN_COMMANDS: ReadonlyArray<ComposerCommandDescriptor> = [
  {
    source: "builtin",
    name: "init",
    path: "/init",
    description: "Generate or update AGENTS.md based on current project",
    argumentHint: "",
    prompt: [
      "Generate or update a file named AGENTS.md that serves as a contributor guide for this repository.",
      "If AGENTS.md already exists, update it in place instead of replacing it with a generic rewrite.",
      "Preserve existing repository-specific conventions and instructions when they are still accurate.",
      "Your goal is to produce a clear, concise, and well-structured document with descriptive headings and actionable explanations for each section.",
      "Follow the outline below, but adapt as needed -- add sections if relevant, and omit those that do not apply to this project.",
      "",
      "Document Requirements",
      "",
      '- Title the document "Repository Guidelines".',
      "- Use Markdown headings (#, ##, etc.) for structure.",
      "- Keep the document concise. 200-400 words is optimal.",
      "- Keep explanations short, direct, and specific to this repository.",
      "- Provide examples where helpful (commands, directory paths, naming patterns).",
      "- Maintain a professional, instructional tone.",
      "",
      "Recommended Sections",
      "",
      "Project Structure & Module Organization",
      "",
      "- Outline the project structure, including where the source code, tests, and assets are located.",
      "",
      "Build, Test, and Development Commands",
      "",
      "- List key commands for building, testing, and running locally (e.g., npm test, make build).",
      "- Briefly explain what each command does.",
      "",
      "Coding Style & Naming Conventions",
      "",
      "- Specify indentation rules, language-specific style preferences, and naming patterns.",
      "- Include any formatting or linting tools used.",
      "",
      "Testing Guidelines",
      "",
      "- Identify testing frameworks and coverage requirements.",
      "- State test naming conventions and how to run tests.",
      "",
      "Commit & Pull Request Guidelines",
      "",
      "- Summarize commit message conventions found in the project's Git history.",
      "- Outline pull request requirements (descriptions, linked issues, screenshots, etc.).",
      "",
      "(Optional) Add other sections if relevant, such as Security & Configuration Tips, Architecture Overview, or Agent-Specific Instructions.",
    ].join("\n"),
    behavior: "prompt",
    smartSend: "always",
  },
  {
    source: "builtin",
    name: "clear",
    path: "/clear",
    description: "Clear current session history and free context",
    argumentHint: "",
    prompt: "Clear conversation history and free up context.",
    behavior: "clear",
    smartSend: "always",
  },
  {
    source: "builtin",
    name: "compact",
    path: "/compact",
    description: "Clear history but keep summary in context",
    argumentHint: "",
    prompt: "Compact the current conversation history and preserve a continuation summary before clearing prior turns.",
    behavior: "compact",
    smartSend: "always",
  },
];

const RESERVED_BUILTIN_NAMES = new Set(BUILTIN_COMMANDS.map((command) => command.name));

function normalizeCommandName(value: string) {
  return value.trim().replace(/^\/+/, "").toLowerCase();
}

function replaceTemplateVariables(template: string, values: Record<string, string>) {
  return template.replace(/{{\s*(\w+)\s*}}/g, (_, key: string) => values[key] ?? "");
}

// ---------------------------------------------------------------------------
// Structured argument parsing
// ---------------------------------------------------------------------------

export type ParsedArguments = {
  named: Record<string, string>;
  positional: string[];
  raw: string;
};

/**
 * Tokenize an arguments string respecting quoted values.
 * Supports double and single quotes. Unclosed quotes treat the remainder as a single token.
 */
function tokenizeArguments(input: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quoteChar: string | null = null;

  for (let i = 0; i < input.length; i++) {
    const ch = input[i]!;

    if (quoteChar) {
      if (ch === quoteChar) {
        quoteChar = null;
      } else {
        current += ch;
      }
    } else if (ch === "\"" || ch === "'") {
      quoteChar = ch;
    } else if (ch === " " || ch === "\t") {
      if (current) {
        tokens.push(current);
        current = "";
      }
    } else {
      current += ch;
    }
  }
  if (current) {
    tokens.push(current);
  }
  return tokens;
}

/**
 * Normalize common Unicode dashes to ASCII hyphens so that flag detection works
 * regardless of whether the user typed em-dashes (—), en-dashes (–), or other variants.
 * A single em-dash (—) is treated as equivalent to double-hyphen (--) since it visually
 * represents the same intent when used as a flag prefix.
 */
function normalizeUnicodeDashes(input: string): string {
  // U+2014 em-dash → "--" (users type — intending --)
  // U+2015 horizontal bar → "--"
  // U+2013 en-dash → "-"
  // U+2012 figure-dash → "-"
  return input
    .replace(/[\u2014\u2015]/g, "--")
    .replace(/[\u2012\u2013]/g, "-");
}

/**
 * Parse an arguments string into structured named (--key=value) and positional parts.
 *
 * Supported formats:
 * - `--key=value`        → named["key"] = "value"
 * - `--key value`        → named["key"] = "value" (when next token is not a flag)
 * - `--flag`             → named["flag"] = "true" (when followed by another flag or end)
 * - positional tokens    → positional[0], positional[1], ...
 * - Quoted values        → `--msg="hello world"` or `--msg 'hello world'`
 */
export function parseCommandArguments(argumentsText: string): ParsedArguments {
  const raw = argumentsText.trim();
  if (!raw) {
    return { named: {}, positional: [], raw: "" };
  }

  const normalized = normalizeUnicodeDashes(raw);
  const tokens = tokenizeArguments(normalized);
  const named: Record<string, string> = {};
  const positional: string[] = [];

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i]!;

    if (token.startsWith("--")) {
      const withoutDashes = token.slice(2);
      const eqIndex = withoutDashes.indexOf("=");

      if (eqIndex >= 0) {
        // --key=value
        const key = withoutDashes.slice(0, eqIndex);
        const value = withoutDashes.slice(eqIndex + 1);
        if (key) {
          named[key] = value;
        }
      } else if (withoutDashes) {
        // --key followed by value or another flag
        const next = tokens[i + 1];
        if (next !== undefined && !next.startsWith("--")) {
          named[withoutDashes] = next;
          i++; // consume the value token
        } else {
          named[withoutDashes] = "true";
        }
      }
    } else {
      positional.push(token);
    }
  }

  return { named, positional, raw };
}

/**
 * Extract declared argument names from an argumentHint string.
 *
 * Recognizes patterns like:
 * - `[--verify=yes|no]` → "verify"
 * - `[--style=simple|full]` → "style"
 * - `[file]` → "file"
 * - `[pr] [branch]` → ["pr", "branch"]
 */
export function extractArgumentNames(argumentHint: string): string[] {
  if (!argumentHint) {
    return [];
  }
  const names: string[] = [];
  const pattern = /\[-{0,2}(\w+)/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(argumentHint)) !== null) {
    const name = match[1];
    if (name && !names.includes(name)) {
      names.push(name);
    }
  }
  return names;
}

function normalizeCommandPath(value: string) {
  const normalized = normalizeCommandName(value);
  return normalized ? `/${normalized}` : "";
}

function getCommandTriggerToken(command: ComposerCommandDescriptor) {
  return command.source === "builtin" ? command.name : normalizeCommandName(command.path);
}

export function isReservedBuiltinCommandName(name: string) {
  return RESERVED_BUILTIN_NAMES.has(normalizeCommandName(name));
}

export function buildComposerCommandRegistry(settingsCommands: ReadonlyArray<CommandEntry>) {
  const customCommands = settingsCommands
    .map<ComposerCommandDescriptor | null>((command) => {
      const normalizedName = normalizeCommandName(command.name);
      if (!normalizedName || isReservedBuiltinCommandName(normalizedName)) {
        return null;
      }

      const commandPath = `/prompts:${normalizedName}`;
      return {
        source: "settings",
        name: normalizedName,
        path: commandPath,
        description: command.description.trim(),
        argumentHint: command.argumentHint.trim(),
        prompt: command.prompt.trim(),
        behavior: "prompt",
        smartSend: command.argumentHint.trim() ? "never" : "always",
      };
    })
    .filter((command): command is ComposerCommandDescriptor => command !== null);

  return [...BUILTIN_COMMANDS, ...customCommands];
}

/**
 * Text length above which slash-command parsing is skipped.
 * Matches the threshold used in workbench-prompt-composer.tsx.
 */
const LARGE_TEXT_THRESHOLD = 10_240;

export function parseSlashCommandInput(
  value: string,
  registry: ReadonlyArray<ComposerCommandDescriptor>,
): { activeToken: string; command: ComposerCommandDescriptor | null; query: string; argumentsText: string } | null {
  // Short-circuit for large text — no slash command needs this much input.
  if (value.length > LARGE_TEXT_THRESHOLD) {
    return null;
  }

  // Find the first non-whitespace character without allocating a trimmed copy.
  let start = 0;
  while (start < value.length) {
    const ch = value.charCodeAt(start);
    if (ch === 32 || ch === 9 || ch === 10 || ch === 13) {
      start++;
    } else {
      break;
    }
  }
  if (start >= value.length || value.charCodeAt(start) !== 47 /* '/' */) {
    return null;
  }

  // Extract the first line from the trimmed-start position without split().
  const nlIndex = value.indexOf("\n", start);
  const firstLine = nlIndex >= 0 ? value.slice(start, nlIndex).trimEnd() : value.slice(start).trimEnd();
  const commandToken = firstLine;
  if (!commandToken.startsWith("/")) {
    return null;
  }

  const withoutSlash = commandToken.slice(1);
  const firstSpaceIndex = withoutSlash.search(/\s/);
  const query = (firstSpaceIndex >= 0 ? withoutSlash.slice(0, firstSpaceIndex) : withoutSlash).trim().toLowerCase();
  const argumentsText = firstSpaceIndex >= 0 ? withoutSlash.slice(firstSpaceIndex + 1).trim() : "";
  const command = registry.find((entry) => getCommandTriggerToken(entry) === query) ?? null;

  return {
    activeToken: commandToken,
    command,
    query,
    argumentsText,
  };
}

export function filterComposerCommands(
  registry: ReadonlyArray<ComposerCommandDescriptor>,
  query: string,
) {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return registry;
  }

  return registry.filter((command) =>
    command.name.includes(normalizedQuery)
    || normalizeCommandPath(command.path).includes(`/${normalizedQuery}`)
    || command.path.toLowerCase().includes(normalizedQuery)
    || command.description.toLowerCase().includes(normalizedQuery),
  );
}

export function shouldSmartSendCommand(
  command: ComposerCommandDescriptor,
  _argumentsText: string,
) {
  return command.smartSend === "always";
}

export function buildCommandEffectivePrompt(
  command: ComposerCommandDescriptor,
  argumentsText: string,
) {
  const trimmedArgs = argumentsText.trim();
  const hasArgumentsPlaceholder = /{{\s*arguments\s*}}/.test(command.prompt);

  // Parse structured arguments and extract declared names from hint
  const parsed = parseCommandArguments(trimmedArgs);
  const declaredNames = extractArgumentNames(command.argumentHint);

  // Map positional arguments to declared names (by order)
  for (let i = 0; i < declaredNames.length && i < parsed.positional.length; i++) {
    const name = declaredNames[i]!;
    if (!(name in parsed.named)) {
      parsed.named[name] = parsed.positional[i]!;
    }
  }

  // Build the full values map for template substitution
  const values: Record<string, string> = {
    arguments: trimmedArgs,
    command: command.name,
    ...parsed.named,
  };
  // Add positional arguments as indexed keys ({{0}}, {{1}}, ...)
  for (let i = 0; i < parsed.positional.length; i++) {
    values[String(i)] = parsed.positional[i]!;
  }

  const originalPrompt = command.prompt;
  let result = replaceTemplateVariables(originalPrompt, values).trim();

  // Fallback: append raw arguments if no argument-related placeholder was consumed.
  // A placeholder is "consumed" if the template contains {{key}} for any key in our
  // parsed arguments (named keys or positional indices), or {{arguments}}.
  const argumentKeys = [
    ...Object.keys(parsed.named),
    ...parsed.positional.map((_, i) => String(i)),
  ];
  const hasAnyArgPlaceholder = hasArgumentsPlaceholder
    || argumentKeys.some((key) => new RegExp(`\\{\\{\\s*${key}\\s*\\}\\}`).test(originalPrompt));
  if (trimmedArgs && !hasAnyArgPlaceholder) {
    result += `\n\nCommand arguments: ${trimmedArgs}`;
  }
  return result;
}

export function buildComposerSubmission(
  message: PromptInputMessage,
  registry: ReadonlyArray<ComposerCommandDescriptor>,
  runMode?: RunMode,
): ComposerSubmission | null {
  const rawText = message.text ?? "";
  const trimmedText = rawText.trim();
  if (!trimmedText && message.files.length === 0) {
    return null;
  }

  const attachments = message.files.map((file, index) => ({
    id: file.url || `${file.filename || "attachment"}-${index}`,
    mediaType: file.mediaType ?? null,
    name: file.filename?.trim() || `附件 ${index + 1}`,
    url: file.url ?? null,
  }));
  const parsedCommand = trimmedText ? parseSlashCommandInput(trimmedText, registry) : null;
  if (!parsedCommand?.command) {
    return {
      kind: "plain",
      displayText: rawText,
      effectivePrompt: rawText,
      rawMessage: message,
      attachments,
      metadata: null,
      runMode,
    };
  }

  const effectivePrompt = buildCommandEffectivePrompt(parsedCommand.command, parsedCommand.argumentsText);
  const invocation: ComposerCommandInvocation = {
    source: parsedCommand.command.source,
    name: parsedCommand.command.name,
    path: parsedCommand.command.path,
    description: parsedCommand.command.description,
    argumentHint: parsedCommand.command.argumentHint,
    argumentsText: parsedCommand.argumentsText,
    prompt: effectivePrompt,
    behavior: parsedCommand.command.behavior,
  };

  return {
    kind: "command",
    displayText: rawText,
    effectivePrompt,
    rawMessage: message,
    attachments,
    command: invocation,
    metadata: {
      composer: {
        kind: "command",
        displayText: rawText,
        effectivePrompt,
        command: invocation,
      },
    },
    runMode,
  };
}
