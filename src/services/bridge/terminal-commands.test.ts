import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri, Channel } from "@tauri-apps/api/core";

import * as terminalCommands from "./terminal-commands";

describe("terminal-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // terminalCreateOrAttach
  // ---------------------------------------------------------------------------
  describe("terminalCreateOrAttach", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        terminalCommands.terminalCreateOrAttach("t1", vi.fn()),
      ).rejects.toThrow("terminal_create_or_attach requires Tauri runtime");
    });

    it("calls terminal_create_or_attach with threadId and channel, defaults null for optional params", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const attachDto = { sessionId: "s1", ptyPid: 123 } as any;
      vi.mocked(invoke).mockResolvedValue(attachDto);

      const onEvent = vi.fn();
      const result = await terminalCommands.terminalCreateOrAttach("t1", onEvent);

      expect(result).toEqual(attachDto);
      expect(invoke).toHaveBeenCalledWith("terminal_create_or_attach", {
        threadId: "t1",
        cols: null,
        rows: null,
        shellPath: null,
        shellArgs: null,
        termEnv: null,
        onEvent: expect.any(Channel),
      });
    });

    it("passes cols and rows when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      const onEvent = vi.fn();
      await terminalCommands.terminalCreateOrAttach("t1", onEvent, 80, 24);

      expect(invoke).toHaveBeenCalledWith("terminal_create_or_attach", expect.objectContaining({
        cols: 80,
        rows: 24,
      }));
    });

    it("passes shellConfig when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      const shellConfig = { shellPath: "/bin/zsh", shellArgs: "-l", termEnv: "xterm-256color" };
      await terminalCommands.terminalCreateOrAttach("t1", vi.fn(), undefined, undefined, shellConfig);

      expect(invoke).toHaveBeenCalledWith("terminal_create_or_attach", expect.objectContaining({
        shellPath: "/bin/zsh",
        shellArgs: "-l",
        termEnv: "xterm-256color",
      }));
    });
  });

  // ---------------------------------------------------------------------------
  // terminalWriteInput
  // ---------------------------------------------------------------------------
  describe("terminalWriteInput", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        terminalCommands.terminalWriteInput("t1", "ls\n"),
      ).rejects.toThrow("terminal_write_input requires Tauri runtime");
    });

    it("calls terminal_write_input with threadId and data", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await terminalCommands.terminalWriteInput("t1", "echo hello");
      expect(invoke).toHaveBeenCalledWith("terminal_write_input", {
        threadId: "t1",
        data: "echo hello",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // terminalResize
  // ---------------------------------------------------------------------------
  describe("terminalResize", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(terminalCommands.terminalResize("t1", 100, 30)).rejects.toThrow(
        "terminal_resize requires Tauri runtime",
      );
    });

    it("calls terminal_resize with cols and rows", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await terminalCommands.terminalResize("t1", 120, 40);
      expect(invoke).toHaveBeenCalledWith("terminal_resize", {
        threadId: "t1",
        cols: 120,
        rows: 40,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // terminalRestart
  // ---------------------------------------------------------------------------
  describe("terminalRestart", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(terminalCommands.terminalRestart("t1", vi.fn())).rejects.toThrow(
        "terminal_restart requires Tauri runtime",
      );
    });

    it("calls terminal_restart with default null values", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const attachDto = { sessionId: "s2" } as any;
      vi.mocked(invoke).mockResolvedValue(attachDto);

      const result = await terminalCommands.terminalRestart("t1", vi.fn());
      expect(result).toEqual(attachDto);
      expect(invoke).toHaveBeenCalledWith("terminal_restart", expect.objectContaining({
        threadId: "t1",
        cols: null,
        rows: null,
        shellPath: null,
        shellArgs: null,
        termEnv: null,
        onEvent: expect.any(Channel),
      }));
    });

    it("passes all options through", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue({} as any);

      const cfg = { shellPath: "/bin/bash" };
      await terminalCommands.terminalRestart("t1", vi.fn(), 160, 48, cfg);

      expect(invoke).toHaveBeenCalledWith("terminal_restart", expect.objectContaining({
        cols: 160,
        rows: 48,
        shellPath: "/bin/bash",
      }));
    });
  });

  // ---------------------------------------------------------------------------
  // terminalClose
  // ---------------------------------------------------------------------------
  describe("terminalClose", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(terminalCommands.terminalClose("t1")).rejects.toThrow(
        "terminal_close requires Tauri runtime",
      );
    });

    it("calls terminal_close with threadId", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await terminalCommands.terminalClose("t1");
      expect(invoke).toHaveBeenCalledWith("terminal_close", { threadId: "t1" });
    });
  });

  // ---------------------------------------------------------------------------
  // terminalList
  // ---------------------------------------------------------------------------
  describe("terminalList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await terminalCommands.terminalList();
      expect(result).toEqual([]);
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls terminal_list when in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const sessions = [{ threadId: "t1" }, { threadId: "t2" }] as any;
      vi.mocked(invoke).mockResolvedValue(sessions);

      const result = await terminalCommands.terminalList();
      expect(result).toEqual(sessions);
      expect(invoke).toHaveBeenCalledWith("terminal_list");
    });
  });
});
