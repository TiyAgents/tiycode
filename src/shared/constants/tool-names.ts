/**
 * Shared tool name constants.
 *
 * These mirror the Rust-side constants in `agent_session.rs` to eliminate
 * hardcoded tool name strings scattered across the frontend codebase.
 */

/** Runtime orchestration tools — hidden from UI (never shown in timeline). */
export const RUNTIME_ORCHESTRATION_TOOLS = [
  "agent_explore",
  "agent_review",
] as const;

/** The prefix shared by all runtime orchestration tool names (built-in and custom). */
export const RUNTIME_ORCHESTRATION_TOOL_PREFIX = "agent_";

/** Task board tools. */
export const TASK_BOARD_TOOLS = [
  "create_task",
  "update_task",
  "query_task",
] as const;

/** Tools that are collapsed by default in the timeline. */
export const DEFAULT_COLLAPSED_TOOLS = [
  ...TASK_BOARD_TOOLS,
  "render",
  "web_search",
] as const;

export function isRuntimeOrchestrationToolName(toolName: string): boolean {
  return (
    toolName.startsWith(RUNTIME_ORCHESTRATION_TOOL_PREFIX)
    && toolName.length > RUNTIME_ORCHESTRATION_TOOL_PREFIX.length
  );
}

export function isTaskBoardTool(toolName: string): boolean {
  return (TASK_BOARD_TOOLS as ReadonlyArray<string>).includes(toolName);
}

export function isDefaultCollapsedTool(toolName: string): boolean {
  return (DEFAULT_COLLAPSED_TOOLS as ReadonlyArray<string>).includes(toolName);
}
