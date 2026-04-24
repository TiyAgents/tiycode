import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri } from "@tauri-apps/api/core";

import * as agentCommands from "./agent-commands";

describe("agent-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // requireTauri guard — all streaming functions throw when not in Tauri
  // ---------------------------------------------------------------------------
  describe("requireTauri guard", () => {
    it("throws for threadStartRun when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.threadStartRun("t1", { prompt: "hi" }, vi.fn()),
      ).rejects.toThrow("thread_start_run requires Tauri runtime");
    });

    it("throws for threadSubscribeRun when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.threadSubscribeRun("t1", vi.fn()),
      ).rejects.toThrow("thread_subscribe_run requires Tauri runtime");
    });

    it("throws for threadExecuteApprovedPlan when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.threadExecuteApprovedPlan("t1", "m1", "apply_plan" as const, vi.fn()),
      ).rejects.toThrow("thread_execute_approved_plan requires Tauri runtime");
    });

    it("throws for threadClearContext when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.threadClearContext("t1"),
      ).rejects.toThrow("thread_clear_context requires Tauri runtime");
    });

    it("throws for threadCompactContext when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.threadCompactContext("t1", undefined, undefined, vi.fn()),
      ).rejects.toThrow("thread_compact_context requires Tauri runtime");
    });

    it("throws for threadCancelRun when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.threadCancelRun("t1"),
      ).rejects.toThrow("thread_cancel_run requires Tauri runtime");
    });

    it("throws for toolApprovalRespond when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.toolApprovalRespond("tc1", "r1", true),
      ).rejects.toThrow("tool_approval_respond requires Tauri runtime");
    });

    it("throws for toolClarifyRespond when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        agentCommands.toolClarifyRespond("tc1", {}),
      ).rejects.toThrow("tool_clarify_respond requires Tauri runtime");
    });
  });

  // ---------------------------------------------------------------------------
  // threadStartRun
  // ---------------------------------------------------------------------------
  describe("threadStartRun", () => {
    it("calls thread_start_run with correct defaults", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue("run-123");

      const events: Array<unknown> = [];
      const result = await agentCommands.threadStartRun(
        "t1", { prompt: "hello" }, (e) => events.push(e),
      );

      expect(result).toBe("run-123");
      expect(invoke).toHaveBeenCalledWith("thread_start_run", {
        threadId: "t1",
        prompt: "hello",
        displayPrompt: null,
        promptMetadata: null,
        attachments: [],
        runMode: null,
        modelPlan: null,
        onEvent: expect.any(Object),
      });
    });

    it("passes optional parameters when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue("run-456");

      const input = {
        prompt: "test",
        displayPrompt: "Test",
        promptMetadata: { source: "user" },
        attachments: [{ filename: "f.ts" }] as any,
      };
      const modelPlan = { modelKey: "gpt-4" } as any;

      await agentCommands.threadStartRun(
        "t2", input, vi.fn(), "plan", modelPlan,
      );

      expect(invoke).toHaveBeenCalledWith("thread_start_run", expect.objectContaining({
        displayPrompt: "Test",
        promptMetadata: { source: "user" },
        attachments: [{ filename: "f.ts" }],
        runMode: "plan",
        modelPlan,
      }));
    });
  });

  // ---------------------------------------------------------------------------
  // threadSubscribeRun
  // ---------------------------------------------------------------------------
  describe("threadSubscribeRun", () => {
    it("calls thread_subscribe_run and returns runId", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue("run-existing");

      const result = await agentCommands.threadSubscribeRun("t1", vi.fn());
      expect(result).toBe("run-existing");
      expect(invoke).toHaveBeenCalledWith("thread_subscribe_run", {
        threadId: "t1",
        onEvent: expect.any(Object),
      });
    });

    it("handles null return (no active run)", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(null);

      const result = await agentCommands.threadSubscribeRun("t1", vi.fn());
      expect(result).toBeNull();
    });
  });

  // ---------------------------------------------------------------------------
  // threadClearContext
  // ---------------------------------------------------------------------------
  describe("threadClearContext", () => {
    it("calls thread_clear_context with threadId", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await agentCommands.threadClearContext("t1");
      expect(invoke).toHaveBeenCalledWith("thread_clear_context", { threadId: "t1" });
    });
  });

  // ---------------------------------------------------------------------------
  // threadCompactContext
  // ---------------------------------------------------------------------------
  describe("threadCompactContext", () => {
    it("calls thread_compact_context with null defaults", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue("run-cmp");

      const result = await agentCommands.threadCompactContext(
        "t1", undefined, undefined, vi.fn(),
      );
      expect(result).toBe("run-cmp");
      expect(invoke).toHaveBeenCalledWith("thread_compact_context", {
        threadId: "t1",
        instructions: null,
        modelPlan: null,
        onEvent: expect.any(Object),
      });
    });

    it("passes instructions and modelPlan when provided", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue("run-cmp2");

      const modelPlan = { modelKey: "gpt-4" } as any;
      await agentCommands.threadCompactContext(
        "t1", "Summarize context", modelPlan, vi.fn(),
      );

      expect(invoke).toHaveBeenCalledWith("thread_compact_context", expect.objectContaining({
        instructions: "Summarize context",
        modelPlan,
      }));
    });
  });

  // ---------------------------------------------------------------------------
  // threadExecuteApprovedPlan
  // ---------------------------------------------------------------------------
  describe("threadExecuteApprovedPlan", () => {
    it("calls thread_execute_approved_plan with action", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue("run-plan");

      const result = await agentCommands.threadExecuteApprovedPlan(
        "t1", "msg-1", "apply_plan", vi.fn(),
      );
      expect(result).toBe("run-plan");
      expect(invoke).toHaveBeenCalledWith("thread_execute_approved_plan", {
        threadId: "t1",
        approvalMessageId: "msg-1",
        action: "apply_plan",
        onEvent: expect.any(Object),
      });
    });

    it("supports apply_plan_with_context_reset action", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue("run-plan2");

      const result = await agentCommands.threadExecuteApprovedPlan(
        "t1", "msg-2", "apply_plan_with_context_reset", vi.fn(),
      );
      expect(result).toBe("run-plan2");
      expect(invoke).toHaveBeenCalledWith("thread_execute_approved_plan", expect.objectContaining({
        action: "apply_plan_with_context_reset",
      }));
    });
  });

  // ---------------------------------------------------------------------------
  // threadCancelRun
  // ---------------------------------------------------------------------------
  describe("threadCancelRun", () => {
    it("returns true on successful cancellation", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(true);

      const result = await agentCommands.threadCancelRun("t1");
      expect(result).toBe(true);
      expect(invoke).toHaveBeenCalledWith("thread_cancel_run", { threadId: "t1" });
    });

    it("returns false when cancellation fails", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(false);

      const result = await agentCommands.threadCancelRun("t1");
      expect(result).toBe(false);
    });
  });

  // ---------------------------------------------------------------------------
  // toolApprovalRespond / toolClarifyRespond
  // ---------------------------------------------------------------------------
  describe("toolApprovalRespond", () => {
    it("calls tool_approval_respond with all params", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await agentCommands.toolApprovalRespond("tc-1", "run-1", true);
      expect(invoke).toHaveBeenCalledWith("tool_approval_respond", {
        toolCallId: "tc-1",
        runId: "run-1",
        approved: true,
      });
    });

    it("passes approved=false for rejection", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await agentCommands.toolApprovalRespond("tc-2", "run-2", false);
      expect(invoke).toHaveBeenCalledWith("tool_approval_respond", expect.objectContaining({
        approved: false,
      }));
    });
  });

  describe("toolClarifyRespond", () => {
    it("calls tool_clarify_respond with response data", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const response = { answer: "Yes, proceed" };
      await agentCommands.toolClarifyRespond("tc-1", response);
      expect(invoke).toHaveBeenCalledWith("tool_clarify_respond", {
        toolCallId: "tc-1",
        response,
      });
    });
  });
});
