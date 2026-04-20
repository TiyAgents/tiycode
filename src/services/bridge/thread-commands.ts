import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  ThreadSummaryDto,
  ThreadSnapshotDto,
  MessageDto,
  AddMessageInput,
  RunModelPlanDto,
} from "@/shared/types/api";

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

export async function threadList(
  workspaceId: string,
  limit?: number,
  offset?: number,
): Promise<ThreadSummaryDto[]> {
  if (!isTauri()) return [];
  return invoke<ThreadSummaryDto[]>("thread_list", {
    workspaceId,
    limit: limit ?? null,
    offset: offset ?? null,
  });
}

export async function threadCreate(
  workspaceId: string,
  title?: string,
  profileId?: string | null,
): Promise<ThreadSummaryDto> {
  requireTauri("thread_create");
  return invoke<ThreadSummaryDto>("thread_create", {
    workspaceId,
    title: title ?? null,
    profileId: profileId ?? null,
  });
}

export async function threadLoad(
  id: string,
  messageCursor?: string,
  messageLimit?: number,
): Promise<ThreadSnapshotDto> {
  requireTauri("thread_load");
  return invoke<ThreadSnapshotDto>("thread_load", {
    id,
    messageCursor: messageCursor ?? null,
    messageLimit: messageLimit ?? null,
  });
}

export async function threadUpdateTitle(
  id: string,
  title: string,
): Promise<void> {
  requireTauri("thread_update_title");
  return invoke("thread_update_title", { id, title });
}

export async function threadUpdateProfile(
  id: string,
  profileId: string | null,
): Promise<void> {
  requireTauri("thread_update_profile");
  return invoke("thread_update_profile", { id, profileId });
}

export async function threadDelete(id: string): Promise<void> {
  requireTauri("thread_delete");
  return invoke("thread_delete", { id });
}

export async function threadAddMessage(
  threadId: string,
  input: AddMessageInput,
): Promise<MessageDto> {
  requireTauri("thread_add_message");
  return invoke<MessageDto>("thread_add_message", { threadId, input });
}

export async function threadRegenerateTitle(
  threadId: string,
  modelPlan: RunModelPlanDto,
): Promise<string> {
  requireTauri("thread_regenerate_title");
  return invoke<string>("thread_regenerate_title", { threadId, modelPlan });
}
