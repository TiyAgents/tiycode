export type SurfaceToolState =
  | "approval-requested"
  | "approval-responded"
  | "clarify-requested"
  | "input-streaming"
  | "input-available"
  | "output-available"
  | "output-denied"
  | "output-error";

export type SurfaceToolEntryLike = {
  input?: unknown;
  name: string;
  result?: unknown;
  state: SurfaceToolState;
};

export type FileMutationPresentation = {
  actionLabel: string;
  contentPreview: string | null;
  diff: string | null;
  fileName: string;
  linesAdded: number | null;
  linesRemoved: number | null;
  path: string;
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

export function countDiffLineChanges(diff: string) {
  let linesAdded = 0;
  let linesRemoved = 0;

  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ ") || line.startsWith("--- ") || line.startsWith("@@ ")) {
      continue;
    }

    if (line.startsWith("+")) {
      linesAdded += 1;
      continue;
    }

    if (line.startsWith("-")) {
      linesRemoved += 1;
    }
  }

  return { linesAdded, linesRemoved };
}

function countTextLines(value: string) {
  if (!value.length) {
    return 0;
  }

  return value.split("\n").length;
}

function buildInputFallbackDiff(path: string, oldValue: string, newValue: string, created: boolean) {
  const oldLines = created ? [] : oldValue.split("\n");
  const newLines = newValue.split("\n");
  const oldPath = created ? "/dev/null" : path;

  return [
    `--- ${oldPath}`,
    `+++ ${path}`,
    `@@ -1,${oldLines.length} +1,${newLines.length} @@`,
    ...oldLines.map((line) => `-${line}`),
    ...newLines.map((line) => `+${line}`),
  ].join("\n");
}

function isFileMutationToolName(toolName: string) {
  return toolName === "edit" || toolName === "patch" || toolName === "write";
}

export function getFileMutationPresentation(tool: SurfaceToolEntryLike): FileMutationPresentation | null {
  if (!isFileMutationToolName(tool.name)) {
    return null;
  }

  const input = asToolDataRecord(tool.input);
  const result = asToolDataRecord(tool.result);
  const path = getToolDataString(result, "path") ?? getToolDataString(input, "path");

  if (!path) {
    return null;
  }

  const oldString = getToolDataString(input, "old_string") ?? "";
  const newString = getToolDataString(input, "new_string");
  const inputContent = getToolDataString(input, "content");
  const created =
    result?.created === true
    || ((tool.name === "edit" || tool.name === "patch") && getToolDataString(input, "old_string") === "");
  const fallbackDiff =
    tool.name === "edit" || tool.name === "patch"
      ? newString !== null
        ? buildInputFallbackDiff(path, oldString, newString, created)
        : null
      : null;
  const diff = getToolDataString(result, "diff") ?? fallbackDiff;
  const derivedDiffCounts = diff ? countDiffLineChanges(diff) : null;
  const fallbackLinesAdded =
    tool.name === "edit" || tool.name === "patch"
      ? newString !== null
        ? countTextLines(newString)
        : null
      : inputContent !== null
        ? countTextLines(inputContent)
        : null;
  const fallbackLinesRemoved =
    tool.name === "edit" || tool.name === "patch"
      ? countTextLines(oldString)
      : created
        ? 0
        : null;
  const linesAdded =
    getToolDataNumber(result, "linesAdded")
    ?? derivedDiffCounts?.linesAdded
    ?? fallbackLinesAdded;
  const linesRemoved =
    getToolDataNumber(result, "linesRemoved")
    ?? derivedDiffCounts?.linesRemoved
    ?? fallbackLinesRemoved;
  const actionLabel = tool.state === "output-available"
    ? created
      ? "Created"
      : "Edited"
    : tool.name === "write"
      ? "Writing"
      : "Editing";

  return {
    actionLabel,
    contentPreview: inputContent ?? newString,
    diff,
    fileName: path.split(/[\\/]/).filter(Boolean).pop() ?? path,
    linesAdded,
    linesRemoved,
    path,
  };
}
