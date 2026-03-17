import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type {
  TerminalAttachDto,
  TerminalSessionDto,
  TerminalStreamEvent,
} from "@/shared/types/api";

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

export async function terminalCreateOrAttach(
  threadId: string,
  onEvent: (event: TerminalStreamEvent) => void,
  cols?: number,
  rows?: number,
): Promise<TerminalAttachDto> {
  requireTauri("terminal_create_or_attach");

  const channel = new Channel<TerminalStreamEvent>();
  channel.onmessage = onEvent;

  return invoke<TerminalAttachDto>("terminal_create_or_attach", {
    threadId,
    cols: cols ?? null,
    rows: rows ?? null,
    onEvent: channel,
  });
}

export async function terminalWriteInput(
  threadId: string,
  data: string,
): Promise<void> {
  requireTauri("terminal_write_input");
  return invoke("terminal_write_input", { threadId, data });
}

export async function terminalResize(
  threadId: string,
  cols: number,
  rows: number,
): Promise<void> {
  requireTauri("terminal_resize");
  return invoke("terminal_resize", { threadId, cols, rows });
}

export async function terminalRestart(
  threadId: string,
  onEvent: (event: TerminalStreamEvent) => void,
  cols?: number,
  rows?: number,
): Promise<TerminalAttachDto> {
  requireTauri("terminal_restart");

  const channel = new Channel<TerminalStreamEvent>();
  channel.onmessage = onEvent;

  return invoke<TerminalAttachDto>("terminal_restart", {
    threadId,
    cols: cols ?? null,
    rows: rows ?? null,
    onEvent: channel,
  });
}

export async function terminalClose(threadId: string): Promise<void> {
  requireTauri("terminal_close");
  return invoke("terminal_close", { threadId });
}

export async function terminalList(): Promise<TerminalSessionDto[]> {
  if (!isTauri()) return [];
  return invoke<TerminalSessionDto[]>("terminal_list");
}

