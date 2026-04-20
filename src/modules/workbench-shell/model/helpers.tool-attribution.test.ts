import { describe, expect, it } from "vitest";
import {
  buildSnapshotHelperToolSummary,
  isHelperOwnedTool,
} from "@/modules/workbench-shell/model/helpers";

// ---------------------------------------------------------------------------
// isHelperOwnedTool
// ---------------------------------------------------------------------------

describe("isHelperOwnedTool", () => {
  it("matches a tool ID with the new short-prefix format (8-char prefix)", () => {
    const helperIds = new Set(["019abcd1-2ef0-7abc-8def-123456789012"]);
    // New format: first 8 chars of helper UUID + ":" + provider call ID
    expect(isHelperOwnedTool("019abcd1:toolu_01A09q90qq", helperIds)).toBe(true);
  });

  it("matches a tool ID with the legacy full-UUID format", () => {
    const helperIds = new Set(["019abcd1-2ef0-7abc-8def-123456789012"]);
    // Legacy format: full UUID + ":" + provider call ID
    expect(isHelperOwnedTool("019abcd1-2ef0-7abc-8def-123456789012:toolu_01A09q90qq", helperIds)).toBe(true);
  });

  it("rejects a tool ID that does not belong to any helper", () => {
    const helperIds = new Set(["019abcd1-2ef0-7abc-8def-123456789012"]);
    expect(isHelperOwnedTool("ffffffff:toolu_01A09q90qq", helperIds)).toBe(false);
  });

  it("rejects a tool ID with a matching prefix but missing colon separator", () => {
    const helperIds = new Set(["019abcd1-2ef0-7abc-8def-123456789012"]);
    // No colon after prefix — should not match
    expect(isHelperOwnedTool("019abcd1toolu_01A09q90qq", helperIds)).toBe(false);
  });

  it("matches when multiple helpers are present", () => {
    const helperIds = new Set([
      "019abcd1-2ef0-7abc-8def-123456789012",
      "ffffffff-0000-0000-0000-000000000000",
    ]);
    expect(isHelperOwnedTool("ffffffff:call_abc123", helperIds)).toBe(true);
    expect(isHelperOwnedTool("019abcd1:call_abc123", helperIds)).toBe(true);
  });

  it("returns false for empty helper set", () => {
    const helperIds = new Set<string>();
    expect(isHelperOwnedTool("019abcd1:call_abc123", helperIds)).toBe(false);
  });

  it("does not produce false positives when two helpers share the same 8-char prefix", () => {
    // Two helpers with same first 8 chars but different full IDs
    const helperIds = new Set([
      "019abcd1-1111-1111-1111-111111111111",
      "019abcd1-2222-2222-2222-222222222222",
    ]);
    // Both should match with the short prefix format
    expect(isHelperOwnedTool("019abcd1:call_abc123", helperIds)).toBe(true);
    // Full legacy format should also match for each
    expect(isHelperOwnedTool("019abcd1-1111-1111-1111-111111111111:call_abc123", helperIds)).toBe(true);
    expect(isHelperOwnedTool("019abcd1-2222-2222-2222-222222222222:call_abc123", helperIds)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// buildSnapshotHelperToolSummary
// ---------------------------------------------------------------------------

describe("buildSnapshotHelperToolSummary", () => {
  const helperId = "019abcd1-2ef0-7abc-8def-123456789012";

  it("counts tools matching the short-prefix format", () => {
    const toolCalls = [
      { id: "019abcd1:toolu_01", toolName: "search", status: "completed" },
      { id: "019abcd1:toolu_02", toolName: "read", status: "failed" },
      { id: "ffffffff:toolu_03", toolName: "edit", status: "completed" },
    ];
    const result = buildSnapshotHelperToolSummary(helperId, toolCalls);
    expect(result.totalToolCalls).toBe(2);
    expect(result.completedSteps).toBe(2); // completed + failed count
    expect(result.toolCounts.search).toBe(1);
    expect(result.toolCounts.read).toBe(1);
  });

  it("counts tools matching the legacy full-UUID format", () => {
    const toolCalls = [
      { id: "019abcd1-2ef0-7abc-8def-123456789012:toolu_01", toolName: "search", status: "completed" },
      { id: "ffffffff:toolu_02", toolName: "edit", status: "completed" },
    ];
    const result = buildSnapshotHelperToolSummary(helperId, toolCalls);
    expect(result.totalToolCalls).toBe(1);
    expect(result.completedSteps).toBe(1);
  });

  it("returns zero counts when no tools match", () => {
    const toolCalls = [
      { id: "ffffffff:toolu_01", toolName: "search", status: "completed" },
    ];
    const result = buildSnapshotHelperToolSummary(helperId, toolCalls);
    expect(result.totalToolCalls).toBe(0);
    expect(result.completedSteps).toBe(0);
    expect(Object.keys(result.toolCounts)).toHaveLength(0);
  });

  it("counts completed, failed, denied, and cancelled steps", () => {
    const toolCalls = [
      { id: "019abcd1:toolu_01", toolName: "search", status: "completed" },
      { id: "019abcd1:toolu_02", toolName: "read", status: "failed" },
      { id: "019abcd1:toolu_03", toolName: "edit", status: "denied" },
      { id: "019abcd1:toolu_04", toolName: "bash", status: "cancelled" },
      { id: "019abcd1:toolu_05", toolName: "write", status: "running" },
    ];
    const result = buildSnapshotHelperToolSummary(helperId, toolCalls);
    expect(result.totalToolCalls).toBe(5);
    expect(result.completedSteps).toBe(4); // completed + failed + denied + cancelled
  });

  it("handles mixed new and legacy format tool IDs for the same helper", () => {
    const toolCalls = [
      { id: "019abcd1:toolu_01", toolName: "search", status: "completed" },
      { id: "019abcd1-2ef0-7abc-8def-123456789012:toolu_02", toolName: "read", status: "completed" },
    ];
    const result = buildSnapshotHelperToolSummary(helperId, toolCalls);
    expect(result.totalToolCalls).toBe(2);
    expect(result.completedSteps).toBe(2);
  });
});