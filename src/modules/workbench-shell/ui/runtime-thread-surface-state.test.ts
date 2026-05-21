import { describe, expect, it } from "vitest";

import { compareTimelineEntries, removeRequestRetryEntriesForRun, type SurfaceRequestRetryEntry, type TimelineEntry } from "./runtime-thread-surface-state";

const occurredAt = "2026-05-19T00:00:00.000Z";

type EntryLabel = "user" | "reasoning" | "assistant" | "helper" | "request_retry" | "tool";

function makeMessageEntry(label: "user" | "reasoning" | "assistant"): TimelineEntry {
  const role = label === "user" ? "user" : "assistant";
  const messageType = label === "reasoning" ? "reasoning" : "plain_message";

  return {
    kind: "message",
    key: `message:${label}`,
    occurredAt,
    message: {
      attachments: [],
      content: label,
      createdAt: occurredAt,
      id: label,
      messageType,
      parts: [{ type: "text", text: label }],
      role,
      runId: role === "user" ? null : "run-1",
      status: "completed",
    },
  };
}

function makeEntry(kind: EntryLabel): TimelineEntry {
  switch (kind) {
    case "user":
    case "reasoning":
    case "assistant":
      return makeMessageEntry(kind);
    case "helper":
      return {
        kind: "helper",
        key: "helper:helper-1",
        occurredAt,
        helper: {
          completedSteps: 0,
          currentAction: null,
          finishedAt: null,
          id: "helper-1",
          inputSummary: null,
          kind: "helper_explore",
          latestMessage: undefined,
          recentActions: [],
          runId: "run-1",
          startedAt: occurredAt,
          status: "running",
          summary: null,
          toolCounts: {},
          totalToolCalls: 0,
        },
      };
    case "request_retry":
      return {
        kind: "request_retry",
        key: "request-retry:run-1",
        occurredAt,
        requestRetry: {
          attempt: 2,
          createdAt: occurredAt,
          delayMs: 750,
          id: "request-retry-run-1",
          maxRetries: 5,
          reason: "stream disconnected",
          runId: "run-1",
          status: null,
          updatedAt: occurredAt,
        },
      };
    case "tool":
      return {
        kind: "tool",
        key: "tool:tool-1",
        occurredAt,
        tool: {
          id: "tool-1",
          name: "read",
          runId: "run-1",
          startedAt: occurredAt,
          state: "input-streaming",
        },
      };
  }
}

function labelEntry(entry: TimelineEntry) {
  if (entry.kind === "message") {
    return entry.message.messageType === "reasoning" ? "reasoning" : entry.message.role;
  }
  return entry.kind;
}

function makeRequestRetryEntry(runId: string): SurfaceRequestRetryEntry {
  return {
    attempt: 2,
    createdAt: occurredAt,
    delayMs: 750,
    id: `request-retry-${runId}`,
    maxRetries: 5,
    reason: "stream disconnected",
    runId,
    status: null,
    updatedAt: occurredAt,
  };
}

describe("removeRequestRetryEntriesForRun", () => {
  it("removes only request retry entries for the matching run", () => {
    const run1 = makeRequestRetryEntry("run-1");
    const run2 = makeRequestRetryEntry("run-2");

    expect(removeRequestRetryEntriesForRun([run1, run2], "run-1")).toEqual([run2]);
  });

  it("keeps the original array reference when no entry matches", () => {
    const entries = [makeRequestRetryEntry("run-1")];

    expect(removeRequestRetryEntriesForRun(entries, "run-2")).toBe(entries);
  });

  it("returns the original empty array", () => {
    const entries: SurfaceRequestRetryEntry[] = [];

    expect(removeRequestRetryEntriesForRun(entries, "run-1")).toBe(entries);
  });
});

describe("compareTimelineEntries", () => {
  it("orders request retry between helper and tool for matching timestamps", () => {
    const entries = [
      makeEntry("assistant"),
      makeEntry("tool"),
      makeEntry("request_retry"),
      makeEntry("helper"),
      makeEntry("reasoning"),
      makeEntry("user"),
    ];

    expect(entries.sort(compareTimelineEntries).map(labelEntry)).toEqual([
      "user",
      "reasoning",
      "helper",
      "request_retry",
      "assistant",
      "tool",
    ]);
  });
});
