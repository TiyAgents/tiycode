import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  ThreadSummaryDto,
  ThreadSnapshotDto,
  MessageDto,
  AddMessageInput,
} from "@/shared/types/api";

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
): Promise<ThreadSummaryDto> {
  return invoke<ThreadSummaryDto>("thread_create", {
    workspaceId,
    title: title ?? null,
  });
}

export async function threadLoad(
  id: string,
  messageCursor?: string,
  messageLimit?: number,
): Promise<ThreadSnapshotDto> {
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
  return invoke("thread_update_title", { id, title });
}

export async function threadDelete(id: string): Promise<void> {
  return invoke("thread_delete", { id });
}

export async function threadAddMessage(
  threadId: string,
  input: AddMessageInput,
): Promise<MessageDto> {
  return invoke<MessageDto>("thread_add_message", { threadId, input });
}
