import type {
  TerminalAttachDto,
  TerminalSessionDto,
  TerminalStreamEvent,
} from "@/shared/types/api";
import {
  terminalClose,
  terminalCreateOrAttach,
  terminalList,
  terminalResize,
  terminalRestart,
  terminalWriteInput,
  type TerminalShellConfig,
} from "@/services/bridge";

export const terminalClient = {
  createOrAttach(
    threadId: string,
    onEvent: (event: TerminalStreamEvent) => void,
    cols?: number,
    rows?: number,
    shellConfig?: TerminalShellConfig,
  ): Promise<TerminalAttachDto> {
    return terminalCreateOrAttach(threadId, onEvent, cols, rows, shellConfig);
  },
  writeInput(threadId: string, data: string): Promise<void> {
    return terminalWriteInput(threadId, data);
  },
  resize(threadId: string, cols: number, rows: number): Promise<void> {
    return terminalResize(threadId, cols, rows);
  },
  restart(
    threadId: string,
    onEvent: (event: TerminalStreamEvent) => void,
    cols?: number,
    rows?: number,
    shellConfig?: TerminalShellConfig,
  ): Promise<TerminalAttachDto> {
    return terminalRestart(threadId, onEvent, cols, rows, shellConfig);
  },
  close(threadId: string): Promise<void> {
    return terminalClose(threadId);
  },
  list(): Promise<TerminalSessionDto[]> {
    return terminalList();
  },
};

