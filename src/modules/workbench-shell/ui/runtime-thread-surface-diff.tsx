import { useMemo } from "react";
import { cn } from "@/shared/lib/utils";

type DiffPreviewRow = {
  kind: "add" | "context" | "hunk" | "remove";
  lineNumber: number | null;
  text: string;
};

function parseDiffStart(value: string | undefined) {
  if (!value) {
    return 0;
  }

  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function buildDiffPreviewRows(diff: string): Array<DiffPreviewRow> {
  const rows: Array<DiffPreviewRow> = [];
  let oldLine = 0;
  let newLine = 0;

  for (const line of diff.split("\n")) {
    if (!line) {
      continue;
    }

    if (line.startsWith("--- ") || line.startsWith("+++ ")) {
      continue;
    }

    if (line.startsWith("@@")) {
      const match = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      oldLine = parseDiffStart(match?.[1]);
      newLine = parseDiffStart(match?.[2]);
      rows.push({
        kind: "hunk",
        lineNumber: null,
        text: line,
      });
      continue;
    }

    if (line.startsWith("+")) {
      rows.push({
        kind: "add",
        lineNumber: newLine || null,
        text: line.slice(1),
      });
      newLine += 1;
      continue;
    }

    if (line.startsWith("-")) {
      rows.push({
        kind: "remove",
        lineNumber: oldLine || null,
        text: line.slice(1),
      });
      oldLine += 1;
      continue;
    }

    if (line.startsWith(" ")) {
      rows.push({
        kind: "context",
        lineNumber: newLine || null,
        text: line.slice(1),
      });
      oldLine += 1;
      newLine += 1;
    }
  }

  return rows;
}

export function buildPlainPreviewRows(content: string): Array<DiffPreviewRow> {
  return content.split("\n").map((line, index) => ({
    kind: "context",
    lineNumber: index + 1,
    text: line,
  }));
}

export function FileMutationDiffPreview({
  contentPreview,
  diff,
}: {
  contentPreview: string | null;
  diff: string | null;
}) {
  const rows = useMemo(
    () => (diff ? buildDiffPreviewRows(diff) : buildPlainPreviewRows(contentPreview ?? "")),
    [contentPreview, diff],
  );

  if (rows.length === 0) {
    return null;
  }

  return (
    <div className="max-h-[22rem] overflow-auto bg-app-drawer font-mono text-[12px] leading-6 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {rows.map((row, index) => (
        <div
          className={cn(
            "grid grid-cols-[56px_1fr] border-b border-app-border/55",
            row.kind === "add"
              ? "bg-app-success/10"
              : row.kind === "remove"
                ? "bg-app-danger/10"
                : row.kind === "hunk"
                  ? "bg-app-surface-muted/55"
                  : "bg-transparent",
          )}
          key={`${row.kind}-${row.lineNumber ?? "h"}-${index}`}
        >
          <span className="select-none border-r border-app-border/60 px-3 text-right text-app-subtle">
            {row.lineNumber ?? ""}
          </span>
          <span
            className={cn(
              "overflow-x-auto whitespace-pre px-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
              row.kind === "add"
                ? "text-app-success"
                : row.kind === "remove"
                  ? "text-app-danger"
                  : row.kind === "hunk"
                    ? "text-app-subtle"
                    : "text-app-foreground",
            )}
          >
            {row.text || " "}
          </span>
        </div>
      ))}
    </div>
  );
}
