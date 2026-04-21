import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type {
  GitBranchDto,
  GitCommitSummaryDto,
  GitDiffDto,
  GitFileStatusDto,
  GitMutationResponseDto,
  GitSnapshotDto,
  GitStreamEvent,
  RunModelPlanDto,
} from "@/shared/types/api";

export type GitUnsubscribe = () => Promise<void>;

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

export async function gitGetSnapshot(
  workspaceId: string,
): Promise<GitSnapshotDto | null> {
  if (!isTauri()) return null;
  return invoke<GitSnapshotDto>("git_get_snapshot", { workspaceId });
}

export async function gitGetHistory(
  workspaceId: string,
  limit?: number,
): Promise<GitCommitSummaryDto[]> {
  if (!isTauri()) return [];
  return invoke<GitCommitSummaryDto[]>("git_get_history", {
    workspaceId,
    limit: limit ?? null,
  });
}

export async function gitGetDiff(
  workspaceId: string,
  path: string,
  staged?: boolean,
): Promise<GitDiffDto> {
  requireTauri("git_get_diff");
  return invoke<GitDiffDto>("git_get_diff", {
    workspaceId,
    path,
    staged: staged ?? null,
  });
}

export async function gitGetFileStatus(
  workspaceId: string,
  path: string,
): Promise<GitFileStatusDto> {
  requireTauri("git_get_file_status");
  return invoke<GitFileStatusDto>("git_get_file_status", {
    workspaceId,
    path,
  });
}

export async function gitGetConflictDiff(
  workspaceId: string,
  path: string,
): Promise<GitDiffDto> {
  requireTauri("git_get_conflict_diff");
  return invoke<GitDiffDto>("git_get_conflict_diff", {
    workspaceId,
    path,
  });
}

export async function gitSubscribe(
  workspaceId: string,
  onEvent: (event: GitStreamEvent) => void,
): Promise<GitUnsubscribe> {
  requireTauri("git_subscribe");

  const channel = new Channel<GitStreamEvent>();
  channel.onmessage = onEvent;

  await invoke("git_subscribe", {
    workspaceId,
    onEvent: channel,
  });

  let unsubscribed = false;
  return async () => {
    if (unsubscribed) {
      return;
    }
    unsubscribed = true;
    await invoke("git_unsubscribe", {
      workspaceId,
      subscriptionId: channel.id,
    });
  };
}

export async function gitRefresh(workspaceId: string): Promise<GitSnapshotDto> {
  requireTauri("git_refresh");
  return invoke<GitSnapshotDto>("git_refresh", { workspaceId });
}

export async function gitStage(
  workspaceId: string,
  paths: string[],
): Promise<GitSnapshotDto> {
  requireTauri("git_stage");
  return invoke<GitSnapshotDto>("git_stage", { workspaceId, paths });
}

export async function gitUnstage(
  workspaceId: string,
  paths: string[],
): Promise<GitSnapshotDto> {
  requireTauri("git_unstage");
  return invoke<GitSnapshotDto>("git_unstage", { workspaceId, paths });
}

export async function gitCommit(
  workspaceId: string,
  message: string,
  approved?: boolean,
): Promise<GitMutationResponseDto> {
  requireTauri("git_commit");
  return invoke<GitMutationResponseDto>("git_commit", {
    workspaceId,
    message,
    approved: approved ?? null,
  });
}

export async function gitGenerateCommitMessage(
  workspaceId: string,
  modelPlan: RunModelPlanDto,
  language: string,
  prompt: string,
): Promise<string> {
  requireTauri("git_generate_commit_message");
  return invoke<string>("git_generate_commit_message", {
    workspaceId,
    modelPlan,
    language,
    prompt,
  });
}

export async function gitFetch(
  workspaceId: string,
  approved?: boolean,
): Promise<GitMutationResponseDto> {
  requireTauri("git_fetch");
  return invoke<GitMutationResponseDto>("git_fetch", {
    workspaceId,
    approved: approved ?? null,
  });
}

export async function gitPull(
  workspaceId: string,
  approved?: boolean,
): Promise<GitMutationResponseDto> {
  requireTauri("git_pull");
  return invoke<GitMutationResponseDto>("git_pull", {
    workspaceId,
    approved: approved ?? null,
  });
}

export async function gitPush(
  workspaceId: string,
  approved?: boolean,
): Promise<GitMutationResponseDto> {
  requireTauri("git_push");
  return invoke<GitMutationResponseDto>("git_push", {
    workspaceId,
    approved: approved ?? null,
  });
}

export async function gitListBranches(
  workspaceId: string,
): Promise<GitBranchDto[]> {
  if (!isTauri()) return [];
  return invoke<GitBranchDto[]>("git_list_branches", { workspaceId });
}

export async function gitCheckoutBranch(
  workspaceId: string,
  branch: string,
  approved?: boolean,
): Promise<GitMutationResponseDto> {
  requireTauri("git_checkout_branch");
  return invoke<GitMutationResponseDto>("git_checkout_branch", {
    workspaceId,
    branch,
    approved: approved ?? null,
  });
}

export async function gitCreateBranch(
  workspaceId: string,
  branch: string,
  approved?: boolean,
): Promise<GitMutationResponseDto> {
  requireTauri("git_create_branch");
  return invoke<GitMutationResponseDto>("git_create_branch", {
    workspaceId,
    branch,
    approved: approved ?? null,
  });
}

export async function gitGenerateBranchName(
  workspaceId: string,
  modelPlan: RunModelPlanDto,
): Promise<string> {
  requireTauri("git_generate_branch_name");
  return invoke<string>("git_generate_branch_name", {
    workspaceId,
    modelPlan,
  });
}
