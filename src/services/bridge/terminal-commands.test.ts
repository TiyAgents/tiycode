import { beforeEach, describe, expect, it, vi } from "vitest";

const { channels, invokeMock, isTauriMock } = vi.hoisted(() => ({
  channels: [] as Array<{ id: number; onmessage: ((event: unknown) => void) | null }>,
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
  isTauri: isTauriMock,
  Channel: class MockChannel<T> {
    id = channels.length + 1;
    onmessage: ((event: T) => void) | null = null;
    constructor() {
      channels.push(this as unknown as { id: number; onmessage: ((event: unknown) => void) | null });
    }
  },
}));

import {
  terminalClose,
  terminalCreateOrAttach,
  terminalList,
  terminalResize,
  terminalRestart,
  terminalWriteInput,
} from "./terminal-commands";

describe("terminal bridge commands", () => {
  beforeEach(() => {
    channels.length = 0;
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("returns an empty list outside Tauri and throws for runtime-only commands", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(terminalList()).resolves.toEqual([]);
    await expect(terminalWriteInput("thread-1", "x")).rejects.toThrow("terminal_write_input requires Tauri runtime");
    await expect(terminalCreateOrAttach("thread-1", vi.fn())).rejects.toThrow("terminal_create_or_attach requires Tauri runtime");
  });

  it("creates channels for attach and restart and forwards optional shell config", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ session: { id: "session-1" } });
    const onEvent = vi.fn();

    await terminalCreateOrAttach("thread-1", onEvent, 120, 40, {
      shellPath: "/bin/zsh",
      shellArgs: "-l",
      termEnv: "A=1",
    });
    channels[0].onmessage?.({ type: "output", data: "hello" });

    await terminalRestart("thread-1", onEvent);

    expect(onEvent).toHaveBeenCalledWith({ type: "output", data: "hello" });
    expect(invokeMock).toHaveBeenCalledWith("terminal_create_or_attach", expect.objectContaining({
      threadId: "thread-1",
      cols: 120,
      rows: 40,
      shellPath: "/bin/zsh",
      shellArgs: "-l",
      termEnv: "A=1",
      onEvent: channels[0],
    }));
    expect(invokeMock).toHaveBeenCalledWith("terminal_restart", expect.objectContaining({
      threadId: "thread-1",
      cols: null,
      rows: null,
      shellPath: null,
      shellArgs: null,
      termEnv: null,
      onEvent: channels[1],
    }));
  });

  it("invokes simple terminal commands", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    await terminalWriteInput("thread-1", "ls\n");
    await terminalResize("thread-1", 80, 24);
    await terminalClose("thread-1");
    await terminalList();

    expect(invokeMock).toHaveBeenCalledWith("terminal_write_input", { threadId: "thread-1", data: "ls\n" });
    expect(invokeMock).toHaveBeenCalledWith("terminal_resize", { threadId: "thread-1", cols: 80, rows: 24 });
    expect(invokeMock).toHaveBeenCalledWith("terminal_close", { threadId: "thread-1" });
    expect(invokeMock).toHaveBeenCalledWith("terminal_list");
  });
});
