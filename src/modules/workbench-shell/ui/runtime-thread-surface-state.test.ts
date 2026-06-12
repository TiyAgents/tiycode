import { describe, expect, it } from "vitest";

import {
  compareTimelineEntries,
  mapRunSummaryToContextUsage,
  removeRequestRetryEntriesForRun,
  type SurfaceRequestRetryEntry,
  type TimelineEntry,
} from "./runtime-thread-surface-state";
import type { RunSummaryDto } from "@/shared/types/api";

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

describe("mapRunSummaryToContextUsage", () => {
  // Fixtures built from `RunSummaryDto` so we exercise the real shape the
  // bridge pipeline emits, including the optional `modelDisplayName`,
  // `contextWindow`, and `errorMessage` fields.
  function makeRun(overrides: Partial<RunSummaryDto["usage"]> = {}): RunSummaryDto {
    return {
      id: "run-1",
      threadId: "thread-1",
      runMode: "default",
      status: "completed",
      modelId: "model-1",
      modelDisplayName: "Test Model",
      contextWindow: "128000",
      errorMessage: null,
      startedAt: occurredAt,
      usage: {
        inputTokens: 0,
        outputTokens: 0,
        cacheReadTokens: 0,
        cacheWriteTokens: 0,
        totalTokens: 0,
        contextSize: 0,
        ...overrides,
      },
    };
  }

  it("returns null when the run itself is null", () => {
    expect(mapRunSummaryToContextUsage(null)).toBeNull();
  });

  it("prefers the explicit contextSize when the field is non-zero", () => {
    // Explicit field wins even when the per-bucket tokens disagree.
    const run = makeRun({
      inputTokens: 999,
      outputTokens: 999,
      cacheReadTokens: 999,
      cacheWriteTokens: 999,
      totalTokens: 999,
      contextSize: 1234,
    });

    const usage = mapRunSummaryToContextUsage(run);

    expect(usage?.contextSize).toBe(1234);
  });

  it("falls back to the token-bucket sum when contextSize is zero", () => {
    // Mimics an older persisted snapshot written before tiycore 0.2.10-rc.2
    // upgraded the cross-protocol `contextSize` field.
    const run = makeRun({
      inputTokens: 10,
      outputTokens: 20,
      cacheReadTokens: 30,
      cacheWriteTokens: 40,
      totalTokens: 100,
      contextSize: 0,
    });

    const usage = mapRunSummaryToContextUsage(run);

    expect(usage?.contextSize).toBe(10 + 20 + 30 + 40);
  });

  it("falls back to the token-bucket sum when contextSize is missing", () => {
    // Defensive: even if a future payload omits the field entirely, the
    // badge should still report a meaningful occupancy.
    const run = makeRun({
      inputTokens: 5,
      outputTokens: 6,
      cacheReadTokens: 7,
      cacheWriteTokens: 8,
      totalTokens: 26,
      contextSize: 0,
    });

    const usage = mapRunSummaryToContextUsage(run);

    expect(usage?.contextSize).toBe(5 + 6 + 7 + 8);
  });

  it("returns zero contextSize when both the explicit field and the buckets are zero", () => {
    const run = makeRun();
    const usage = mapRunSummaryToContextUsage(run);
    expect(usage?.contextSize).toBe(0);
  });

  it("preserves the per-bucket fields alongside the computed contextSize", () => {
    const run = makeRun({
      inputTokens: 1,
      outputTokens: 2,
      cacheReadTokens: 3,
      cacheWriteTokens: 4,
      totalTokens: 10,
      contextSize: 0,
    });

    const usage = mapRunSummaryToContextUsage(run);

    expect(usage).toEqual({
      contextWindow: "128000",
      inputTokens: 1,
      outputTokens: 2,
      cacheReadTokens: 3,
      cacheWriteTokens: 4,
      contextSize: 1 + 2 + 3 + 4,
      totalTokens: 10,
      modelDisplayName: "Test Model",
      runId: "run-1",
    });
  });
});
