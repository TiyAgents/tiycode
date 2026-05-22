import { describe, expect, it } from "vitest";

import {
  isDefaultCollapsedTool,
  isRuntimeOrchestrationToolName,
  isTaskBoardTool,
  RUNTIME_ORCHESTRATION_TOOLS,
  TASK_BOARD_TOOLS,
  DEFAULT_COLLAPSED_TOOLS,
} from "./tool-names";

describe("isRuntimeOrchestrationToolName", () => {
  it("returns true for all runtime orchestration tool names", () => {
    for (const name of RUNTIME_ORCHESTRATION_TOOLS) {
      expect(isRuntimeOrchestrationToolName(name)).toBe(true);
    }
  });

  it("returns false for non-orchestration tool names", () => {
    expect(isRuntimeOrchestrationToolName("read")).toBe(false);
    expect(isRuntimeOrchestrationToolName("edit")).toBe(false);
    expect(isRuntimeOrchestrationToolName("shell")).toBe(false);
    expect(isRuntimeOrchestrationToolName("create_task")).toBe(false);
  });

  it("returns false for empty string and case-mismatched names", () => {
    expect(isRuntimeOrchestrationToolName("")).toBe(false);
    expect(isRuntimeOrchestrationToolName("AGENT_EXPLORE")).toBe(false);
    expect(isRuntimeOrchestrationToolName("Agent_Explore")).toBe(false);
  });

  it("returns false for partial or extended matches", () => {
    expect(isRuntimeOrchestrationToolName("agent_")).toBe(false);
    expect(isRuntimeOrchestrationToolName("agent_explore_extra")).toBe(false);
    expect(isRuntimeOrchestrationToolName("agent_review_v2")).toBe(false);
  });
});

describe("isTaskBoardTool", () => {
  it("returns true for all task board tool names", () => {
    for (const name of TASK_BOARD_TOOLS) {
      expect(isTaskBoardTool(name)).toBe(true);
    }
  });

  it("returns false for non-task tool names", () => {
    expect(isTaskBoardTool("read")).toBe(false);
    expect(isTaskBoardTool("agent_explore")).toBe(false);
  });

  it("returns false for empty and edge-case strings", () => {
    expect(isTaskBoardTool("")).toBe(false);
    expect(isTaskBoardTool("CREATE_TASK")).toBe(false);
    expect(isTaskBoardTool("create_task_extra")).toBe(false);
  });
});

describe("isDefaultCollapsedTool", () => {
  it("returns true for all default collapsed tool names", () => {
    for (const name of DEFAULT_COLLAPSED_TOOLS) {
      expect(isDefaultCollapsedTool(name)).toBe(true);
    }
  });

  it("includes task board tools, render, and web_search", () => {
    expect(isDefaultCollapsedTool("create_task")).toBe(true);
    expect(isDefaultCollapsedTool("update_task")).toBe(true);
    expect(isDefaultCollapsedTool("query_task")).toBe(true);
    expect(isDefaultCollapsedTool("render")).toBe(true);
    expect(isDefaultCollapsedTool("web_search")).toBe(true);
  });

  it("returns false for non-collapsed tool names", () => {
    expect(isDefaultCollapsedTool("read")).toBe(false);
    expect(isDefaultCollapsedTool("shell")).toBe(false);
    expect(isDefaultCollapsedTool("edit")).toBe(false);
    expect(isDefaultCollapsedTool("agent_explore")).toBe(false);
  });

  it("returns false for empty string", () => {
    expect(isDefaultCollapsedTool("")).toBe(false);
  });
});
