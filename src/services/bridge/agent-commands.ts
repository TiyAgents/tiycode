import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type { ThreadStreamEvent, SidecarStatusDto } from "@/shared/types/api";

/**
 * Start a new agent run for a thread.
 *
 * Returns the run ID. Events are delivered via the `onEvent` callback,
 * which is backed by a Tauri Channel for real-time streaming.
 */
export async function threadStartRun(
  threadId: string,
  prompt: string,
  onEvent: (event: ThreadStreamEvent) => void,
  runMode?: string,
): Promise<string> {
  const channel = new Channel<ThreadStreamEvent>();
  channel.onmessage = onEvent;

  return invoke<string>("thread_start_run", {
    threadId,
    prompt,
    runMode: runMode ?? null,
    onEvent: channel,
  });
}

export async function threadCancelRun(threadId: string): Promise<void> {
  return invoke("thread_cancel_run", { threadId });
}

export async function toolApprovalRespond(
  toolCallId: string,
  runId: string,
  approved: boolean,
): Promise<void> {
  return invoke("tool_approval_respond", { toolCallId, runId, approved });
}

export async function sidecarStatus(): Promise<SidecarStatusDto> {
  if (!isTauri()) return { running: false };
  return invoke<SidecarStatusDto>("sidecar_status");
}
