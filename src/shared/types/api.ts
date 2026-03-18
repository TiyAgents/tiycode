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

export interface ProviderDto {
  id: string;
  name: string;
  protocolType: string;
  baseUrl: string;
  hasApiKey: boolean;
  enabled: boolean;
  customHeaders: Record<string, string> | null;
  createdAt: string;
  updatedAt: string;
}

export interface ProviderInput {
  name: string;
  protocolType?: string;
  baseUrl: string;
  apiKey?: string;
  enabled?: boolean;
  customHeaders?: Record<string, string>;
}

export interface ProviderModelDto {
  id: string;
  providerId: string;
  modelName: string;
  displayName: string | null;
  enabled: boolean;
  capabilities: Record<string, boolean> | null;
}

export interface ProviderModelInput {
  modelName: string;
  displayName?: string;
  enabled?: boolean;
  capabilities?: Record<string, boolean>;
}

// ---------------------------------------------------------------------------
// Agent Profile
// ---------------------------------------------------------------------------

export interface AgentProfileDto {
  id: string;
  name: string;
  customInstructions: string | null;
  responseStyle: string | null;
  responseLanguage: string | null;
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
  responseStyle?: string;
  responseLanguage?: string;
  primaryProviderId?: string;
  primaryModelId?: string;
  auxiliaryProviderId?: string;
  auxiliaryModelId?: string;
  lightweightProviderId?: string;
  lightweightModelId?: string;
  isDefault?: boolean;
}

// ---------------------------------------------------------------------------
// Thread
// ---------------------------------------------------------------------------

export type ThreadStatus =
  | "idle"
  | "running"
  | "waiting_approval"
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

export type MessageStatus = "streaming" | "completed" | "failed";

export type RunMode = "default" | "plan";

export type RunStatus =
  | "created"
  | "dispatching"
  | "running"
  | "waiting_approval"
  | "waiting_tool_result"
  | "cancelling"
  | "completed"
  | "failed"
  | "denied"
  | "interrupted"
  | "cancelled";

export interface MessageDto {
  id: string;
  threadId: string;
  runId: string | null;
  role: "user" | "assistant" | "system";
  contentMarkdown: string;
  messageType: MessageType;
  status: MessageStatus;
  metadata: unknown | null;
  createdAt: string;
}

export interface RunSummaryDto {
  id: string;
  threadId: string;
  runMode: RunMode;
  status: RunStatus;
  modelId: string | null;
  startedAt: string;
}

export interface ThreadSnapshotDto {
  thread: ThreadSummaryDto;
  messages: MessageDto[];
  hasMoreMessages: boolean;
  activeRun: RunSummaryDto | null;
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

export type ThreadStreamEvent =
  | { type: "run_started"; runId: string; runMode: string }
  | { type: "message_delta"; runId: string; messageId: string; delta: string }
  | {
      type: "message_completed";
      runId: string;
      messageId: string;
      content: string;
    }
  | { type: "plan_updated"; runId: string; plan: unknown }
  | { type: "reasoning_updated"; runId: string; reasoning: string }
  | { type: "queue_updated"; runId: string; queue: unknown }
  | { type: "subagent_started"; runId: string; subtaskId: string }
  | {
      type: "subagent_completed";
      runId: string;
      subtaskId: string;
      summary: string | null;
    }
  | {
      type: "subagent_failed";
      runId: string;
      subtaskId: string;
      error: string;
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
      type: "approval_resolved";
      runId: string;
      toolCallId: string;
      approved: boolean;
    }
  | { type: "tool_running"; runId: string; toolCallId: string }
  | {
      type: "tool_completed";
      runId: string;
      toolCallId: string;
      result: unknown;
    }
  | { type: "tool_failed"; runId: string; toolCallId: string; error: string }
  | { type: "run_completed"; runId: string }
  | { type: "run_failed"; runId: string; error: string }
  | { type: "run_interrupted"; runId: string };

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

// ---------------------------------------------------------------------------
// Sidecar
// ---------------------------------------------------------------------------

export interface SidecarStatusDto {
  running: boolean;
}
