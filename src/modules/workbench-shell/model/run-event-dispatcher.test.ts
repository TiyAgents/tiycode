import type { Machine } from "@/shared/lib/create-machine";
import { describe, expect, it, beforeEach, vi } from "vitest";
import type { RunMachineContext, RunMachineEvent, RunMachineState } from "./run-lifecycle-machine";
import {
  dispatchGlobalEvent,
  dispatchRunFinishedEvent,
  dispatchRunStatusChangedEvent,
  registerRunMachine,
  unregisterRunMachine,
} from "./run-event-dispatcher";
import { setThreadStatus } from "./thread-store";

vi.mock("./thread-store", () => ({
  setThreadStatus: vi.fn(),
}));

function makeMockMachine(): Machine<RunMachineState, RunMachineEvent, RunMachineContext> {
  return {
    getState: vi.fn().mockReturnValue("idle"),
    getContext: vi.fn().mockReturnValue({ runId: null, errorMessage: null, retryCount: 0 }),
    send: vi.fn(),
    subscribe: vi.fn().mockReturnValue(() => {}),
    reset: vi.fn(),
    destroy: vi.fn(),
  };
}

describe("run-event-dispatcher", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Clean up module-level registry between tests.
    unregisterRunMachine("thread-1");
    unregisterRunMachine("thread-unknown");
    unregisterRunMachine("a");
    unregisterRunMachine("b");
  });

  describe("dispatchGlobalEvent", () => {
    it("routes event to registered machine", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);

      dispatchGlobalEvent("thread-1", "RUN_STARTED", { runId: "r1" });
      expect(machine.send).toHaveBeenCalledWith("RUN_STARTED", { runId: "r1" });
    });

    it("falls back to threadStore for unregistered threads", () => {
      dispatchGlobalEvent("thread-unknown", "RUN_STARTED", { runId: "r2" });
      expect(setThreadStatus).toHaveBeenCalledWith(
        "thread-unknown",
        "running", // RUN_STARTED maps to "running"
        expect.objectContaining({ runId: "r2", source: "tauri_event" }),
      );
    });

    it("falls back to threadStore after unregister", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);
      unregisterRunMachine("thread-1");

      dispatchGlobalEvent("thread-1", "RUN_COMPLETED", { runId: "r1" });
      expect(machine.send).not.toHaveBeenCalled();
      expect(setThreadStatus).toHaveBeenCalledWith(
        "thread-1",
        "completed",
        expect.objectContaining({ runId: "r1", source: "tauri_event" }),
      );
    });
  });

  describe("dispatchRunFinishedEvent", () => {
    it("routes finished event to registered machine", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);

      dispatchRunFinishedEvent("thread-1", "r1", "failed");
      expect(machine.send).toHaveBeenCalledWith("RUN_FAILED", {
        runId: "r1",
        message: undefined,
      });
    });

    it("maps completed status to RUN_COMPLETED", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);

      dispatchRunFinishedEvent("thread-1", "r1", "completed");
      expect(machine.send).toHaveBeenCalledWith("RUN_COMPLETED", {
        runId: "r1",
        message: undefined,
      });
    });

    it("falls back to threadStore for unregistered threads", () => {
      dispatchRunFinishedEvent("thread-unknown", "r1", "failed");
      expect(setThreadStatus).toHaveBeenCalledWith(
        "thread-unknown",
        "failed",
        expect.objectContaining({ runId: "r1", source: "tauri_event" }),
      );
    });
  });

  describe("registerUnregister", () => {
    it("does not leak machines between tests via module-level map", () => {
      const m1 = makeMockMachine();
      const m2 = makeMockMachine();

      registerRunMachine("a", m1);
      registerRunMachine("b", m2);

      dispatchGlobalEvent("a", "RUN_STARTED");
      dispatchGlobalEvent("b", "RUN_CANCELLED");

      expect(m1.send).toHaveBeenCalledWith("RUN_STARTED", undefined);
      expect(m2.send).toHaveBeenCalledWith("RUN_CANCELLED", undefined);

      unregisterRunMachine("a");
      unregisterRunMachine("b");
    });
  });

  describe("dispatchRunStatusChangedEvent", () => {
    it("routes to machine with mapped event for registered thread", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);

      dispatchRunStatusChangedEvent("thread-1", "r1", "waiting_approval");
      expect(machine.send).toHaveBeenCalledWith("APPROVAL_REQUIRED", {
        runId: "r1",
        message: undefined,
      });
    });

    it("maps needs_reply to CLARIFY_REQUIRED for registered machine", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);

      dispatchRunStatusChangedEvent("thread-1", "r1", "needs_reply");
      expect(machine.send).toHaveBeenCalledWith("CLARIFY_REQUIRED", {
        runId: "r1",
        message: undefined,
      });
    });

    it("maps running to RUN_STARTED for registered machine", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);

      dispatchRunStatusChangedEvent("thread-1", "r1", "running");
      expect(machine.send).toHaveBeenCalledWith("RUN_STARTED", {
        runId: "r1",
        message: undefined,
      });
    });

    it("falls back to setThreadStatus for unregistered thread with waiting_approval", () => {
      dispatchRunStatusChangedEvent("thread-unknown", "r2", "waiting_approval");
      expect(setThreadStatus).toHaveBeenCalledWith(
        "thread-unknown",
        "waiting_approval",
        expect.objectContaining({ runId: "r2", source: "tauri_event" }),
      );
    });

    it("falls back to setThreadStatus for unregistered thread with needs_reply", () => {
      dispatchRunStatusChangedEvent("thread-unknown", "r2", "needs_reply");
      expect(setThreadStatus).toHaveBeenCalledWith(
        "thread-unknown",
        "needs_reply",
        expect.objectContaining({ runId: "r2", source: "tauri_event" }),
      );
    });

    it("falls back to setThreadStatus for unregistered thread with running", () => {
      dispatchRunStatusChangedEvent("thread-unknown", "r3", "running");
      expect(setThreadStatus).toHaveBeenCalledWith(
        "thread-unknown",
        "running",
        expect.objectContaining({ runId: "r3", source: "tauri_event" }),
      );
    });

    it("maps terminal statuses correctly for unregistered thread", () => {
      dispatchRunStatusChangedEvent("thread-unknown", "r4", "limit_reached");
      expect(setThreadStatus).toHaveBeenCalledWith(
        "thread-unknown",
        "limit_reached",
        expect.objectContaining({ runId: "r4", source: "tauri_event" }),
      );
    });

    it("skips unknown status without crashing for registered machine", () => {
      const machine = makeMockMachine();
      registerRunMachine("thread-1", machine);

      dispatchRunStatusChangedEvent("thread-1", "r1", "some_unknown_status");
      expect(machine.send).not.toHaveBeenCalled();
    });
  });
});
