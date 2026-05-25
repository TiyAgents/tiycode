import { describe, expect, it } from "vitest";

import {
  formatHelperKind,
  formatHelperName,
  formatHelperSummary,
} from "./runtime-thread-surface-helpers";
import type { RuntimeSurfaceHelperEntry } from "./runtime-thread-surface-helpers";

function makeHelper(
  overrides: Partial<RuntimeSurfaceHelperEntry> = {},
): RuntimeSurfaceHelperEntry {
  return {
    completedSteps: 0,
    currentAction: null,
    error: undefined,
    finishedAt: null,
    id: "h1",
    inputSummary: null,
    kind: "helper_explore",
    latestMessage: undefined,
    recentActions: [],
    runId: "run-1",
    startedAt: "2026-05-25T00:00:00Z",
    status: "completed",
    summary: null,
    toolCounts: {},
    totalToolCalls: 0,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// formatHelperKind
// ---------------------------------------------------------------------------

describe("formatHelperKind", () => {
  it("returns 'Explore Agent' for helper_explore", () => {
    expect(formatHelperKind("helper_explore")).toBe("Explore Agent");
  });

  it("returns 'Review Agent' for helper_review", () => {
    expect(formatHelperKind("helper_review")).toBe("Review Agent");
  });

  it("returns custom name from slugToNameMap when slug is present", () => {
    const map = new Map([["refactor", "Refactor Agent"]]);
    expect(formatHelperKind("helper_custom_refactor", map)).toBe(
      "Refactor Agent",
    );
  });

  it("falls back to capitalized slug when slug is not in map", () => {
    expect(formatHelperKind("helper_custom_code-review")).toBe(
      "Code Review",
    );
  });

  it("falls back to capitalized single-word slug", () => {
    expect(formatHelperKind("helper_custom_refactor")).toBe("Refactor");
  });

  it("returns raw kind for non-custom unknown kinds", () => {
    expect(formatHelperKind("helper_unknown")).toBe("helper_unknown");
  });

  it("returns raw kind for unrelated strings", () => {
    expect(formatHelperKind("anything_else")).toBe("anything_else");
  });

  it("handles empty slug after prefix gracefully", () => {
    // helper_custom_ with no slug — should return empty string from capitalizeSlug
    expect(formatHelperKind("helper_custom_")).toBe("");
  });

  it("prefers map name over capitalized slug", () => {
    const map = new Map([["my-agent", "My Custom Name"]]);
    expect(formatHelperKind("helper_custom_my-agent", map)).toBe(
      "My Custom Name",
    );
  });
});

// ---------------------------------------------------------------------------
// formatHelperName
// ---------------------------------------------------------------------------

describe("formatHelperName", () => {
  it("delegates to formatHelperKind without slugToNameMap", () => {
    const helper = makeHelper({ kind: "helper_explore" });
    expect(formatHelperName(helper)).toBe("Explore Agent");
  });

  it("passes slugToNameMap through to formatHelperKind", () => {
    const map = new Map([["refactor", "Refactor Helper"]]);
    const helper = makeHelper({ kind: "helper_custom_refactor" });
    expect(formatHelperName(helper, map)).toBe("Refactor Helper");
  });
});

// ---------------------------------------------------------------------------
// formatHelperSummary
// ---------------------------------------------------------------------------

describe("formatHelperSummary", () => {
  it("joins name and detail summary with middle dot", () => {
    const helper = makeHelper({
      kind: "helper_explore",
      inputSummary: "scanning files",
      totalToolCalls: 3,
    });
    expect(formatHelperSummary(helper)).toBe(
      "Explore Agent · scanning files · 3 tool calls",
    );
  });

  it("includes custom name from slugToNameMap", () => {
    const map = new Map([["refactor", "Refactor Agent"]]);
    const helper = makeHelper({
      kind: "helper_custom_refactor",
      inputSummary: "restructuring module",
      totalToolCalls: 1,
    });
    expect(formatHelperSummary(helper, map)).toBe(
      "Refactor Agent · restructuring module · 1 tool call",
    );
  });

  it("omits inputSummary when null", () => {
    const helper = makeHelper({
      kind: "helper_review",
      inputSummary: null,
      totalToolCalls: 5,
    });
    expect(formatHelperSummary(helper)).toBe(
      "Review Agent · 5 tool calls",
    );
  });

  it("returns only name when no detail summary parts", () => {
    const helper = makeHelper({
      kind: "helper_explore",
      inputSummary: null,
      totalToolCalls: 0,
    });
    expect(formatHelperSummary(helper)).toBe("Explore Agent");
  });
});
