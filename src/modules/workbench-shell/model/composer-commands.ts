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
  return replaceTemplateVariables(command.prompt, {
    arguments: argumentsText.trim(),
    command: command.name,
  }).trim();
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
