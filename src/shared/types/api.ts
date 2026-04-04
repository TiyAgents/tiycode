/**
 * TypeScript types aligned with Rust camelCase DTOs.
 *
 * These types mirror the Rust `#[serde(rename_all = "camelCase")]` structs
 * exactly as they appear on the wire via Tauri invoke.
 */

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

export type WorkspaceStatus = "ready" | "missing" | "inaccessible" | "invalid";

export interface WorkspaceDto {
  id: string;
  name: string;
  path: string;
  canonicalPath: string;
  displayPath: string;
  isDefault: boolean;
  isGit: boolean;
  autoWorkTree: boolean;
  status: WorkspaceStatus;
  lastValidatedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

// ---------------------------------------------------------------------------
// Settings & Policies (KV)
// ---------------------------------------------------------------------------

export interface SettingDto {
  key: string;
  value: unknown;
  updatedAt: string;
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

export type ProviderSettingsKind = "builtin" | "custom";

export interface ProviderCatalogEntryDto {
  providerKey: string;
  providerType: string;
  displayName: string;
  builtin: boolean;
  supportsCustom: boolean;
  defaultBaseUrl: string;
}

export interface ProviderModelSettingsDto {
  id: string;
  providerId: string;
  modelId: string;
  sortIndex: number;
  displayName: string | null;
  enabled: boolean;
  contextWindow: string | null;
  maxOutputTokens: string | null;
  capabilityOverrides: Record<string, boolean> | null;
  providerOptions: Record<string, unknown> | null;
  isManual: boolean;
}

export interface ProviderSettingsDto {
  id: string;
  kind: ProviderSettingsKind;
  providerKey: string;
  providerType: string;
  displayName: string;
  enabled: boolean;
  lockedMapping: boolean;
  baseUrl: string;
  hasApiKey: boolean;
  customHeaders: Record<string, string> | null;
  models: ProviderModelSettingsDto[];
  createdAt: string;
  updatedAt: string;
}

export interface ProviderModelConnectionTestResultDto {
  success: boolean;
  unsupported: boolean;
  message: string;
  detail?: string | null;
}

export interface ProviderModelInput {
  id?: string;
  modelId: string;
  displayName?: string;
  enabled?: boolean;
  contextWindow?: string;
  maxOutputTokens?: string;
  capabilityOverrides?: Record<string, boolean>;
  providerOptions?: Record<string, unknown>;
  isManual?: boolean;
}

export interface ProviderSettingsUpdateInput {
  displayName?: string;
  providerType?: string;
  baseUrl?: string;
  apiKey?: string;
  enabled?: boolean;
  customHeaders?: Record<string, string>;
  models?: ProviderModelInput[];
}

export interface CustomProviderCreateInput {
  displayName: string;
  providerType: string;
  baseUrl: string;
  apiKey?: string;
  enabled?: boolean;
  customHeaders?: Record<string, string>;
  models?: ProviderModelInput[];
}

// ---------------------------------------------------------------------------
// Agent Profile
// ---------------------------------------------------------------------------

export interface AgentProfileDto {
  id: string;
  name: string;
  customInstructions: string | null;
  commitMessagePrompt: string | null;
  responseStyle: string | null;
  responseLanguage: string | null;
  commitMessageLanguage: string | null;
  primaryProviderId: string | null;
  primaryModelId: string | null;
  auxiliaryProviderId: string | null;
  auxiliaryModelId: string | null;
  lightweightProviderId: string | null;
  lightweightModelId: string | null;
  isDefault: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface AgentProfileInput {
  name: string;
  customInstructions?: string;
  commitMessagePrompt?: string;
  responseStyle?: string;
  responseLanguage?: string;
  commitMessageLanguage?: string;
  primaryProviderId?: string;
  primaryModelId?: string;
  auxiliaryProviderId?: string;
  auxiliaryModelId?: string;
  lightweightProviderId?: string;
  lightweightModelId?: string;
  isDefault?: boolean;
}

export interface RunModelPlanRoleDto {
  providerId: string;
  modelRecordId: string;
  provider: string;
  providerKey: string;
  providerType: string;
  providerName: string;
  model: string;
  modelId: string;
  modelDisplayName: string;
  baseUrl: string;
  contextWindow?: string | null;
  maxOutputTokens?: string | null;
  supportsImageInput?: boolean | null;
  customHeaders?: Record<string, string> | null;
  providerOptions?: Record<string, unknown> | null;
}

export interface RunModelPlanDto {
  profileId?: string | null;
  profileName?: string | null;
  customInstructions?: string | null;
  responseStyle?: string | null;
  responseLanguage?: string | null;
  primary?: RunModelPlanRoleDto | null;
  auxiliary?: RunModelPlanRoleDto | null;
  lightweight?: RunModelPlanRoleDto | null;
  toolProfileByMode?: Partial<Record<RunMode, string>> | null;
}

// ---------------------------------------------------------------------------
// Thread
// ---------------------------------------------------------------------------

export type ThreadStatus =
  | "idle"
  | "running"
  | "waiting_approval"
  | "needs_reply"
  | "interrupted"
  | "failed"
  | "archived";

export interface ThreadSummaryDto {
  id: string;
  workspaceId: string;
  title: string;
  status: ThreadStatus;
  lastActiveAt: string;
  createdAt: string;
}

export type MessageType =
  | "plain_message"
  | "plan"
  | "reasoning"
  | "tool_request"
  | "tool_result"
  | "approval_prompt"
  | "sources"
  | "summary_marker";

export type MessageStatus = "streaming" | "completed" | "failed" | "discarded";

export type RunMode = "default" | "plan";

export type RunStatus =
  | "created"
  | "dispatching"
  | "running"
  | "waiting_approval"
  | "needs_reply"
  | "waiting_tool_result"
  | "cancelling"
  | "completed"
  | "limit_reached"
  | "failed"
  | "denied"
  | "interrupted"
  | "cancelled";

export interface MessageAttachmentDto {
  id: string;
  name: string;
  mediaType: string | null;
  url: string | null;
}

export interface MessageDto {
  id: string;
  threadId: string;
  runId: string | null;
  role: "user" | "assistant" | "system";
  contentMarkdown: string;
  messageType: MessageType;
  status: MessageStatus;
  metadata: unknown | null;
  attachments: MessageAttachmentDto[];
  createdAt: string;
}

export interface RunSummaryDto {
  id: string;
  threadId: string;
  runMode: RunMode;
  status: RunStatus;
  modelId: string | null;
  modelDisplayName: string | null;
  contextWindow: string | null;
  errorMessage: string | null;
  startedAt: string;
  usage: RunUsageDto;
}

export interface RunUsageDto {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  totalTokens: number;
}

export interface ToolCallDto {
  id: string;
  runId: string;
  threadId: string;
  toolName: string;
  toolInput: unknown;
  toolOutput: unknown | null;
  status: string;
  approvalStatus: string | null;
  startedAt: string;
  finishedAt: string | null;
}

export interface RunHelperDto {
  id: string;
  runId: string;
  threadId: string;
  helperKind: string;
  parentToolCallId: string | null;
  status: string;
  inputSummary: string | null;
  outputSummary: string | null;
  errorSummary: string | null;
  startedAt: string;
  finishedAt: string | null;
  usage: RunUsageDto;
}

// ---------------------------------------------------------------------------
// Task Tracking
// ---------------------------------------------------------------------------

export type TaskBoardStatus = "active" | "completed" | "abandoned";
export type TaskStage = "pending" | "in_progress" | "completed" | "failed";

export interface TaskItemDto {
  id: string;
  taskBoardId: string;
  description: string;
  stage: TaskStage;
  sortOrder: number;
  errorDetail: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface TaskBoardDto {
  id: string;
  threadId: string;
  title: string;
  status: TaskBoardStatus;
  activeTaskId: string | null;
  tasks: TaskItemDto[];
  createdAt: string;
  updatedAt: string;
}

export interface ThreadSnapshotDto {
  thread: ThreadSummaryDto;
  messages: MessageDto[];
  hasMoreMessages: boolean;
  activeRun: RunSummaryDto | null;
  latestRun: RunSummaryDto | null;
  toolCalls: ToolCallDto[];
  helpers: RunHelperDto[];
  taskBoards: TaskBoardDto[];
  activeTaskBoardId: string | null;
}

export interface AddMessageInput {
  role: "user" | "assistant" | "system";
  content: string;
  messageType?: MessageType;
  metadata?: unknown;
}

// ---------------------------------------------------------------------------
// Thread Stream Events (from Rust channels)
// ---------------------------------------------------------------------------

export type SubagentActivityStatus = "started" | "succeeded" | "failed";

export interface SubagentProgressSnapshot {
  totalToolCalls: number;
  completedSteps: number;
  currentAction: string | null;
  toolCounts: Record<string, number>;
  recentActions: string[];
  usage: RunUsageDto;
}

export type ThreadStreamEvent =
  | { type: "run_started"; runId: string; runMode: string }
  | { type: "stream_resync_required"; runId: string; droppedEvents: number }
  | {
      type: "run_retrying";
      runId: string;
      attempt: number;
      maxAttempts: number;
      delayMs: number;
      reason: string;
    }
  | { type: "message_delta"; runId: string; messageId: string; delta: string }
  | {
      type: "message_completed";
      runId: string;
      messageId: string;
      content: string;
    }
  | {
      type: "message_discarded";
      runId: string;
      messageId: string;
      reason: string;
    }
  | { type: "plan_updated"; runId: string; plan: unknown }
  | { type: "reasoning_updated"; runId: string; messageId: string; reasoning: string }
  | { type: "queue_updated"; runId: string; queue: unknown }
  | {
      type: "subagent_started";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      type: "subagent_progress";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      activity: SubagentActivityStatus;
      message: string;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      type: "subagent_usage_updated";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      type: "subagent_completed";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      summary: string | null;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      type: "subagent_failed";
      runId: string;
      subtaskId: string;
      helperKind: string;
      startedAt: string;
      error: string;
      snapshot: SubagentProgressSnapshot;
    }
  | {
      type: "tool_requested";
      runId: string;
      toolCallId: string;
      toolName: string;
      toolInput: unknown;
    }
  | {
      type: "approval_required";
      runId: string;
      toolCallId: string;
      toolName: string;
      toolInput: unknown;
      reason: string;
    }
  | {
      type: "clarify_required";
      runId: string;
      toolCallId: string;
      toolName: string;
      toolInput: unknown;
    }
  | {
      type: "approval_resolved";
      runId: string;
      toolCallId: string;
      approved: boolean;
    }
  | {
      type: "clarify_resolved";
      runId: string;
      toolCallId: string;
      response: unknown;
    }
  | { type: "tool_running"; runId: string; toolCallId: string }
  | {
      type: "tool_completed";
      runId: string;
      toolCallId: string;
      result: unknown;
    }
  | { type: "tool_failed"; runId: string; toolCallId: string; error: string }
  | { type: "thread_title_updated"; runId: string; threadId: string; title: string }
  | {
      type: "thread_usage_updated";
      runId: string;
      modelDisplayName: string | null;
      contextWindow: string | null;
      usage: RunUsageDto;
    }
  | { type: "run_checkpointed"; runId: string }
  | { type: "run_completed"; runId: string }
  | { type: "run_limit_reached"; runId: string; error: string; maxTurns: number }
  | { type: "run_failed"; runId: string; error: string }
  | { type: "run_cancelled"; runId: string }
  | { type: "run_interrupted"; runId: string }
  | { type: "task_board_updated"; runId: string; taskBoard: TaskBoardDto };

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

export type GitFileState = "tracked" | "modified" | "untracked" | "ignored";

export type GitChangeKind =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "typechange"
  | "unmerged";

export interface GitRepoCapabilitiesDto {
  repoAvailable: boolean;
  gitCliAvailable: boolean;
}

export interface GitFileChangeDto {
  path: string;
  previousPath: string | null;
  status: GitChangeKind;
  additions: number;
  deletions: number;
}

export interface GitCommitSummaryDto {
  id: string;
  shortId: string;
  summary: string;
  authorName: string;
  committedAt: string;
  refs: string[];
  isHead: boolean;
}

export interface GitSnapshotDto {
  workspaceId: string;
  repoRoot: string | null;
  capabilities: GitRepoCapabilitiesDto;
  headRef: string | null;
  headOid: string | null;
  isDetached: boolean;
  aheadCount: number;
  behindCount: number;
  stagedFiles: GitFileChangeDto[];
  unstagedFiles: GitFileChangeDto[];
  untrackedFiles: GitFileChangeDto[];
  recentCommits: GitCommitSummaryDto[];
  lastRefreshedAt: string;
}

export type GitDiffLineKind = "context" | "add" | "remove";

export interface GitDiffLineDto {
  kind: GitDiffLineKind;
  oldNumber: number | null;
  newNumber: number | null;
  text: string;
}

export interface GitDiffHunkDto {
  header: string;
  lines: GitDiffLineDto[];
}

export interface GitDiffDto {
  path: string;
  staged: boolean;
  status: GitChangeKind;
  oldPath: string | null;
  newPath: string | null;
  additions: number;
  deletions: number;
  isBinary: boolean;
  truncated: boolean;
  hunks: GitDiffHunkDto[];
}

export interface GitFileStatusDto {
  path: string;
  stagedStatus: GitChangeKind | null;
  unstagedStatus: GitChangeKind | null;
  isUntracked: boolean;
  isIgnored: boolean;
}

export type GitMutationAction = "commit" | "fetch" | "pull" | "push";

export interface GitCommandResultDto {
  action: GitMutationAction;
  summary: string;
  stdout: string | null;
  stderr: string | null;
}

export type GitMutationResponseDto =
  | {
      type: "completed";
      result: GitCommandResultDto;
      snapshot: GitSnapshotDto;
    }
  | {
      type: "approval_required";
      action: GitMutationAction;
      reason: string;
    };

export type GitStreamEvent =
  | { type: "refresh_started"; workspaceId: string }
  | { type: "snapshot_updated"; workspaceId: string; snapshot: GitSnapshotDto }
  | { type: "refresh_completed"; workspaceId: string };

// ---------------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------------

export type TerminalSessionStatus = "starting" | "running" | "exited";

export interface TerminalSessionDto {
  sessionId: string;
  threadId: string;
  workspaceId: string;
  shell: string;
  cwd: string;
  cols: number;
  rows: number;
  status: TerminalSessionStatus;
  hasUnreadOutput: boolean;
  lastOutputAt: string | null;
  exitCode: number | null;
  createdAt: string;
}

export interface TerminalAttachDto {
  session: TerminalSessionDto;
  replay: string;
}

export type TerminalStreamEvent =
  | { type: "session_created"; threadId: string; session: TerminalSessionDto }
  | { type: "stdout_chunk"; threadId: string; data: string }
  | { type: "stderr_chunk"; threadId: string; data: string }
  | { type: "status_changed"; threadId: string; status: TerminalSessionStatus }
  | { type: "session_exited"; threadId: string; exitCode: number | null };
