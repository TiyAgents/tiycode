import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type {
  GitCommitSummaryDto,
  GitDiffDto,
  GitFileStatusDto,
  GitSnapshotDto,
  GitStreamEvent,
} from "@/shared/types/api";

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

export async function gitSubscribe(
  workspaceId: string,
  onEvent: (event: GitStreamEvent) => void,
): Promise<void> {
  requireTauri("git_subscribe");

  const channel = new Channel<GitStreamEvent>();
  channel.onmessage = onEvent;

  await invoke("git_subscribe", {
    workspaceId,
    onEvent: channel,
  });
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
