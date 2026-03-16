import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  ThreadSummaryDto,
  ThreadSnapshotDto,
  MessageDto,
  AddMessageInput,
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
): Promise<ThreadSummaryDto> {
  requireTauri("thread_create");
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
