import { CodeBlock } from "@/components/ai-elements/code-block";
import type { SurfaceToolState } from "@/modules/workbench-shell/ui/runtime-thread-surface-state";

export type RuntimeSurfaceToolEntry = {
  error?: string;
  input?: unknown;
  name: string;
  result?: unknown;
  state: SurfaceToolState;
};

function asToolDataRecord(value: unknown) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

function getToolDataString(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "string" ? value : null;
}

function getToolDataNumber(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "number" ? value : null;
}

export type ReadToolPresentation = {
  error: string | null;
  fileName: string;
  path: string;
  rangeLabel: string | null;
};

export type QueryToolPresentation = {
  actionLabel: "Find" | "Search";
  countLabel: string | null;
  error: string | null;
  primaryLabel: string;
  scopeLabel: string | null;
};

export type ListToolPresentation = {
  countLabel: string | null;
  directoryLabel: string;
  error: string | null;
  path: string;
};

export type CommandOutputToolPresentation = {
  actionLabel: string;
  command: string;
  commandLanguage: "bash" | "log" | "shell";
  detailLabel: string | null;
  output: string | null;
  outputLanguage: "log";
  summaryLabel: string;
  showCommandBlock?: boolean;
  showOutputLabel?: boolean;
};

export function getReadToolPresentation(tool: RuntimeSurfaceToolEntry): ReadToolPresentation | null {
  if (tool.name !== "read") {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const path = getToolDataString(result, "path") ?? getToolDataString(input, "path");

  if (!path) {
    return null;
  }

  const shownLines = getToolDataNumber(result, "shownLines");
  const lineCount = getToolDataNumber(result, "lineCount");
  const startLine = getToolDataNumber(result, "offset") ?? 1;

  let rangeLabel: string | null;
  if (shownLines && shownLines > 0) {
    rangeLabel = `[${startLine}-${startLine + shownLines - 1}]`;
  } else if (lineCount && lineCount > 0) {
    rangeLabel = `[${startLine}-${lineCount}]`;
  } else {
    rangeLabel = null;
  }

  const error = tool.state === "output-error" || tool.state === "output-denied"
    ? (tool.error ?? getToolDataString(result, "error") ?? null)
    : null;

  return {
    error,
    fileName: path.split(/[\\/]/).filter(Boolean).pop() ?? path,
    path,
    rangeLabel,
  };
}

function formatToolScopeLabel(scope: string | null) {
  if (!scope) {
    return null;
  }

  const normalized = scope.replace(/\\/g, "/").replace(/\/$/, "");
  if (!normalized) {
    return null;
  }

  const leaf = normalized.split("/").filter(Boolean).pop();
  return leaf ?? normalized;
}

function normalizeSearchFilePatternLabel(pattern: string | null): string | null {
  if (!pattern) {
    return null;
  }

  const trimmed = pattern.trim();
  if (
    trimmed === "*"
    || trimmed === "**"
    || trimmed === "**/*"
    || trimmed === "./*"
    || trimmed === "./**/*"
  ) {
    return null;
  }

  return trimmed;
}

export function getQueryToolPresentation(tool: RuntimeSurfaceToolEntry): QueryToolPresentation | null {
  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);

  if (tool.name === "find") {
    const pattern = getToolDataString(input, "pattern") ?? getToolDataString(result, "pattern");
    if (!pattern) {
      return null;
    }

    const count = getToolDataNumber(result, "count");
    const scope = formatToolScopeLabel(
      getToolDataString(result, "directory") ?? getToolDataString(input, "path"),
    );

    return {
      actionLabel: "Find",
      countLabel: typeof count === "number" ? `${count} result${count === 1 ? "" : "s"}` : null,
      error: tool.state === "output-error" || tool.state === "output-denied"
        ? (tool.error ?? getToolDataString(result, "error") ?? null)
        : null,
      primaryLabel: pattern,
      scopeLabel: scope,
    };
  }

  if (tool.name === "search") {
    const query = getToolDataString(input, "query") ?? getToolDataString(result, "query");
    if (!query) {
      return null;
    }

    const count = getToolDataNumber(result, "count");
    const scope = formatToolScopeLabel(
      getToolDataString(result, "directory") ?? getToolDataString(input, "directory"),
    );
    const filePattern = normalizeSearchFilePatternLabel(getToolDataString(input, "filePattern"));

    return {
      actionLabel: "Search",
      countLabel: typeof count === "number" ? `${count} match${count === 1 ? "" : "es"}` : null,
      error: tool.state === "output-error" || tool.state === "output-denied"
        ? (tool.error ?? getToolDataString(result, "error") ?? null)
        : null,
      primaryLabel: filePattern ? `${query} · ${filePattern}` : query,
      scopeLabel: scope,
    };
  }

  return null;
}

export function getListToolPresentation(tool: RuntimeSurfaceToolEntry): ListToolPresentation | null {
  if (tool.name !== "list") {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const path = getToolDataString(result, "path") ?? getToolDataString(input, "path");

  if (!path) {
    return null;
  }

  const count = getToolDataNumber(result, "count");

  return {
    countLabel: typeof count === "number" ? `${count} item${count === 1 ? "" : "s"}` : null,
    directoryLabel: formatToolScopeLabel(path) ?? path,
    error: tool.state === "output-error" || tool.state === "output-denied"
      ? (tool.error ?? getToolDataString(result, "error") ?? null)
      : null,
    path,
  };
}

function quoteShellValue(value: string) {
  if (!value.length) {
    return "''";
  }

  if (/^[\w./:@%+=,-]+$/u.test(value)) {
    return value;
  }

  return `'${value.replace(/'/g, "'\\''")}'`;
}

function formatJoinedArgs(values: ReadonlyArray<string>) {
  return values.map((value) => quoteShellValue(value)).join(" ");
}

function getToolDataStringArray(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((entry): entry is string => typeof entry === "string" && entry.length > 0);
}

function joinTextSections(sections: Array<{ label: string; value: string | null | undefined }>) {
  const normalized = sections
    .map(({ label, value }) => {
      const trimmed = value?.trim();
      return trimmed ? `${label}:\n${trimmed}` : null;
    })
    .filter((value): value is string => Boolean(value));

  return normalized.length > 0 ? normalized.join("\n\n") : null;
}

function summarizeInlineText(value: string | null, fallback: string) {
  if (!value) {
    return fallback;
  }

  const normalized = value.replace(/\s+/g, " ").trim();
  if (!normalized) {
    return fallback;
  }

  return normalized.length > 80 ? `${normalized.slice(0, 77)}...` : normalized;
}

function formatTerminalSessionSummary(record: Record<string, unknown> | null) {
  if (!record) {
    return null;
  }

  const status = getToolDataString(record, "status");
  const shell = getToolDataString(record, "shell");
  const cwd = getToolDataString(record, "cwd");
  const cols = getToolDataNumber(record, "cols");
  const rows = getToolDataNumber(record, "rows");
  const exitCode = getToolDataNumber(record, "exitCode");

  return joinTextSections([
    { label: "status", value: status },
    { label: "shell", value: shell },
    { label: "cwd", value: cwd },
    {
      label: "size",
      value:
        typeof cols === "number" && typeof rows === "number"
          ? `${cols} x ${rows}`
          : null,
    },
    {
      label: "exit code",
      value: typeof exitCode === "number" ? String(exitCode) : null,
    },
  ]);
}

function formatTerminalDetailLabel(record: Record<string, unknown> | null) {
  if (!record) {
    return null;
  }

  const status = getToolDataString(record, "status");
  const cwd = formatToolScopeLabel(getToolDataString(record, "cwd"));

  return [status, cwd].filter(Boolean).join(" · ") || null;
}

function getShellToolPresentation(tool: RuntimeSurfaceToolEntry): CommandOutputToolPresentation | null {
  if (tool.name !== "shell") {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const command = getToolDataString(result, "command") ?? getToolDataString(input, "command");
  if (!command) {
    return null;
  }

  const stdout = getToolDataString(result, "stdout");
  const stderr = getToolDataString(result, "stderr");
  const exitCode = getToolDataNumber(result, "exitCode");
  const cwd = getToolDataString(input, "cwd");
  const stdoutTruncated = result?.stdoutTruncated === true;
  const stderrTruncated = result?.stderrTruncated === true;

  const output = joinTextSections([
    { label: "stdout", value: stdout },
    { label: "stderr", value: stderr ?? tool.error },
    {
      label: "exit code",
      value:
        typeof exitCode === "number" && (exitCode !== 0 || (!stdout && !stderr))
          ? String(exitCode)
          : null,
    },
    {
      label: "note",
      value:
        stdoutTruncated || stderrTruncated
          ? "Output was truncated to keep the latest lines visible."
          : null,
    },
  ]);

  return {
    actionLabel: "Shell",
    command,
    commandLanguage: "bash",
    detailLabel: cwd ? `cwd · ${cwd}` : null,
    output,
    outputLanguage: "log",
    summaryLabel: summarizeInlineText(command, "shell"),
  };
}

function buildGitCommand(toolName: string, input: Record<string, unknown> | null) {
  const paths = getToolDataStringArray(input, "paths");

  switch (toolName) {
    case "git_add":
    case "git_stage":
      return paths.length > 0
        ? `git add -- ${formatJoinedArgs(paths)}`
        : "git add";
    case "git_unstage":
      return paths.length > 0
        ? `git restore --staged -- ${formatJoinedArgs(paths)}`
        : "git restore --staged";
    case "git_commit": {
      const message = getToolDataString(input, "message");
      return message ? `git commit -m ${quoteShellValue(message)}` : "git commit";
    }
    case "git_fetch":
      return "git fetch --prune";
    case "git_pull":
      return "git pull --ff-only";
    case "git_push":
      return "git push";
    case "git_status":
      return "git status --short";
    case "git_diff":
      return "git diff";
    case "git_log":
      return "git log --oneline";
    default:
      return toolName;
  }
}

function buildGitFallbackOutput(
  toolName: string,
  input: Record<string, unknown> | null,
  result: Record<string, unknown> | null,
) {
  const paths = getToolDataStringArray(result, "paths");
  const resolvedPaths = paths.length > 0 ? paths : getToolDataStringArray(input, "paths");

  switch (toolName) {
    case "git_add":
    case "git_stage":
      return resolvedPaths.length > 0
        ? `staged paths:\n${resolvedPaths.join("\n")}`
        : "Staged changes.";
    case "git_unstage":
      return resolvedPaths.length > 0
        ? `unstaged paths:\n${resolvedPaths.join("\n")}`
        : "Unstaged changes.";
    default:
      return null;
  }
}

function getGitToolPresentation(tool: RuntimeSurfaceToolEntry): CommandOutputToolPresentation | null {
  if (!tool.name.startsWith("git_")) {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const command = buildGitCommand(tool.name, input);
  const summary = getToolDataString(result, "summary");
  const stdout = getToolDataString(result, "stdout");
  const stderr = getToolDataString(result, "stderr");

  const output =
    joinTextSections([
      { label: "summary", value: summary },
      { label: "stdout", value: stdout },
      { label: "stderr", value: stderr ?? tool.error },
    ])
    ?? buildGitFallbackOutput(tool.name, input, result)
    ?? tool.error
    ?? null;

  return {
    actionLabel: "Git",
    command,
    commandLanguage: "bash",
    detailLabel: summary,
    output,
    outputLanguage: "log",
    summaryLabel: summarizeInlineText(command, tool.name),
  };
}

function buildTerminalCommand(tool: RuntimeSurfaceToolEntry, input: Record<string, unknown> | null) {
  switch (tool.name) {
    case "term_write": {
      const data = getToolDataString(input, "data") ?? getToolDataString(input, "input");
      return data ?? "term_write";
    }
    case "term_restart": {
      const cols = getToolDataNumber(input, "cols");
      const rows = getToolDataNumber(input, "rows");
      const sizeArgs = [
        typeof cols === "number" ? `--cols ${cols}` : null,
        typeof rows === "number" ? `--rows ${rows}` : null,
      ].filter(Boolean);
      return sizeArgs.length > 0 ? `term_restart ${sizeArgs.join(" ")}` : "term_restart";
    }
    default:
      return tool.name;
  }
}

function buildTerminalSummaryLabel(tool: RuntimeSurfaceToolEntry, command: string) {
  switch (tool.name) {
    case "term_write":
      return summarizeInlineText(command, "terminal input");
    case "term_output":
      return "Recent terminal output";
    case "term_status":
      return "Terminal status";
    case "term_restart":
      return "Restart terminal";
    case "term_close":
      return "Close terminal";
    default:
      return summarizeInlineText(command, tool.name);
  }
}

function buildTerminalOutput(tool: RuntimeSurfaceToolEntry, result: Record<string, unknown> | null) {
  if (tool.name === "term_output") {
    return getToolDataString(result, "output") ?? tool.error ?? null;
  }

  return formatTerminalSessionSummary(result) ?? tool.error ?? null;
}

function getTerminalToolPresentation(tool: RuntimeSurfaceToolEntry): CommandOutputToolPresentation | null {
  if (!tool.name.startsWith("term_")) {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const command = buildTerminalCommand(tool, input);

  return {
    actionLabel: "Terminal",
    command,
    commandLanguage: tool.name === "term_write" ? "bash" : "shell",
    detailLabel:
      formatTerminalDetailLabel(result)
      ?? formatToolScopeLabel(getToolDataString(input, "cwd")),
    output: buildTerminalOutput(tool, result),
    outputLanguage: "log",
    summaryLabel: buildTerminalSummaryLabel(tool, command),
    showCommandBlock:
      tool.name !== "term_status" && tool.name !== "term_output" && tool.name !== "term_close",
    showOutputLabel:
      tool.name !== "term_status" && tool.name !== "term_output" && tool.name !== "term_close",
  };
}

export function getCommandOutputToolPresentation(tool: RuntimeSurfaceToolEntry) {
  return (
    getShellToolPresentation(tool)
    ?? getGitToolPresentation(tool)
    ?? getTerminalToolPresentation(tool)
  );
}

export const TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS =
  "max-h-[min(50vh,28rem)]";

export function ToolCommandOutputBlocks({
  presentation,
}: {
  presentation: CommandOutputToolPresentation;
}) {
  return (
    <div className="space-y-3">
      {presentation.showCommandBlock !== false ? (
        <div className="space-y-1.5">
          <h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
            Command
          </h4>
          <CodeBlock
            code={presentation.command}
            contentClassName={TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS}
            language={presentation.commandLanguage}
          />
        </div>
      ) : null}
      {presentation.output ? (
        <div className="space-y-1.5">
          {presentation.showOutputLabel !== false ? (
            <h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
              Output
            </h4>
          ) : null}
          <CodeBlock
            code={presentation.output}
            contentClassName={TOOL_DETAIL_CODE_BLOCK_CONTENT_CLASS}
            language={presentation.outputLanguage}
          />
        </div>
      ) : null}
    </div>
  );
}

