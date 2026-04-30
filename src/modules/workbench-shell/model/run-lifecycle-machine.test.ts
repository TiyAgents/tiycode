import { describe, expect, it, vi } from "vitest";
import {
  createRunLifecycleMachine,
  mapStreamEventToMachineEvent,
  type RunMachineState,
} from "./run-lifecycle-machine";
import { setThreadStatus } from "./thread-store";

// Mock the threadStore to avoid side effects
vi.mock("./thread-store", () => ({
  setThreadStatus: vi.fn(),
}));

function makeMachine(threadId = "test-thread") {
  return createRunLifecycleMachine(threadId);
}

describe("run-lifecycle-machine", () => {
  describe("initial state", () => {
    it("starts in idle state with default context", () => {
      const m = makeMachine();
      expect(m.getState()).toBe("idle");
      expect(m.getContext()).toEqual({
        runId: null,
        errorMessage: null,
        retryCount: 0,
      });
    });

    it("creates machine with correct initial state", () => {
      const m = makeMachine();
      expect(m.getState()).toBe("idle");
    });
  });

  describe("happy path: idle → running → completed", () => {
    it("transitions idle → running on RUN_STARTED", () => {
      const m = makeMachine();
      m.send("RUN_STARTED", { runId: "run-1" });
      expect(m.getState()).toBe("running");
      expect(m.getContext().runId).toBe("run-1");
      expect(m.getContext().retryCount).toBe(0);
    });

    it("transitions running → completed on RUN_COMPLETED", () => {
      const m = makeMachine();
      m.send("RUN_STARTED", { runId: "run-1" });
      m.send("RUN_COMPLETED");
      expect(m.getState()).toBe("completed");
    });
  });

  describe("approval flow", () => {
    it("transitions running → waiting_approval on APPROVAL_REQUIRED", () => {
      const m = makeMachine();
      m.send("RUN_STARTED");
      m.send("APPROVAL_REQUIRED");
      expect(m.getState()).toBe("waiting_approval");
    });

    it("transitions waiting_approval → running on APPROVAL_RESOLVED", () => {
      const m = makeMachine();
      m.send("RUN_STARTED");
      m.send("APPROVAL_REQUIRED");
      m.send("APPROVAL_RESOLVED");
      expect(m.getState()).toBe("running");
    });

    it("allows cancellation from waiting_approval", () => {
      const m = makeMachine();
      m.send("RUN_STARTED");
      m.send("APPROVAL_REQUIRED");
      m.send("RUN_CANCELLED");
      expect(m.getState()).toBe("cancelled");
    });
  });

  describe("clarify flow", () => {
    it("transitions running → needs_reply on CLARIFY_REQUIRED", () => {
      const m = makeMachine();
      m.send("RUN_STARTED");
      m.send("CLARIFY_REQUIRED");
      expect(m.getState()).toBe("needs_reply");
    });

    it("transitions needs_reply → running on CLARIFY_RESOLVED", () => {
      const m = makeMachine();
      m.send("RUN_STARTED");
      m.send("CLARIFY_REQUIRED");
      m.send("CLARIFY_RESOLVED");
      expect(m.getState()).toBe("running");
    });
  });

  describe("terminal states can restart", () => {
    const terminalStates: RunMachineState[] = [
      "completed",
      "failed",
      "cancelled",
      "interrupted",
      "limit_reached",
    ];

    for (const state of terminalStates) {
      it(`transitions ${state} → running on RUN_STARTED`, () => {
        // We need to get the machine into the target state first.
        // Use reset() to force it for testing.
        const m = makeMachine();
        m.reset(state, { runId: "old-run", errorMessage: null, retryCount: 0 });
        m.send("RUN_STARTED", { runId: "run-2" });
        expect(m.getState()).toBe("running");
        expect(m.getContext().runId).toBe("run-2");
        expect(m.getContext().retryCount).toBe(0); // resets on new run
      });
    }
  });

  describe("illegal transitions are ignored", () => {
    it("ignores idle → RUN_COMPLETED (must go through running)", () => {
      const m = makeMachine();
      m.send("RUN_COMPLETED");
      expect(m.getState()).toBe("idle");
    });

    it("ignores completed → RUN_COMPLETED", () => {
      const m = makeMachine();
      m.reset("completed");
      m.send("RUN_COMPLETED");
      expect(m.getState()).toBe("completed");
    });

    it("ignores APPROVAL_RESOLVED from idle", () => {
      const m = makeMachine();
      m.send("APPROVAL_RESOLVED");
      expect(m.getState()).toBe("idle");
    });
  });

  describe("RUN_RETRYING self-transition", () => {
    it("stays in running but updates runId and retryCount", () => {
      const m = makeMachine();
      m.send("RUN_STARTED", { runId: "run-1" });
      expect(m.getContext().retryCount).toBe(0);

      m.send("RUN_RETRYING", { newRunId: "run-2" });
      expect(m.getState()).toBe("running");
      expect(m.getContext().runId).toBe("run-2");
      expect(m.getContext().retryCount).toBe(1);
    });

    it("increments retryCount on multiple retries", () => {
      const m = makeMachine();
      m.send("RUN_STARTED", { runId: "run-1" });
      m.send("RUN_RETRYING", { newRunId: "run-2" });
      m.send("RUN_RETRYING", { newRunId: "run-3" });
      expect(m.getContext().retryCount).toBe(2);
      expect(m.getContext().runId).toBe("run-3");
    });
  });

  describe("RUN_FAILED context", () => {
    it("stores error message in context", () => {
      const m = makeMachine();
      m.send("RUN_STARTED");
      m.send("RUN_FAILED", { message: "something went wrong" });
      expect(m.getState()).toBe("failed");
      expect(m.getContext().errorMessage).toBe("something went wrong");
    });
  });

  describe("reset", () => {
    it("resets to given state and context", () => {
      const m = makeMachine();
      m.send("RUN_STARTED", { runId: "run-1" });
      m.reset("waiting_approval", {
        runId: "snapshot-run",
        errorMessage: null,
        retryCount: 0,
      });
      expect(m.getState()).toBe("waiting_approval");
      expect(m.getContext().runId).toBe("snapshot-run");
    });

    it("defaults to initial state when no args provided", () => {
      const m = makeMachine();
      m.send("RUN_STARTED", { runId: "run-1" });
      m.reset();
      expect(m.getState()).toBe("idle");
    });
  });

  describe("subscribe / threadStore sync", () => {
    it("notifies subscriber on state change", () => {
      const m = makeMachine("sync-thread");
      const listener = vi.fn();
      m.subscribe(listener);

      // Initial subscribe already called (machine creation)
      vi.clearAllMocks();

      m.send("RUN_STARTED", { runId: "r1" });
      expect(listener).toHaveBeenCalled();
      expect(setThreadStatus).toHaveBeenCalledWith(
        "sync-thread",
        "running",
        expect.objectContaining({ runId: "r1", source: "stream" }),
      );
    });

    it("notifies on context-only change (RUN_RETRYING)", () => {
      const m = makeMachine("retry-thread");
      m.send("RUN_STARTED", { runId: "r1" });
      vi.clearAllMocks();

      m.send("RUN_RETRYING", { newRunId: "r2" });
      // Machine sends to threadStore even though state is still "running"
      // because context changed.
      expect(setThreadStatus).toHaveBeenCalledWith(
        "retry-thread",
        "running",
        expect.objectContaining({ runId: "r2", source: "stream" }),
      );
    });
  });
});

describe("mapStreamEventToMachineEvent", () => {
  it("maps known stream events to machine events", () => {
    expect(mapStreamEventToMachineEvent("run_started")).toBe("RUN_STARTED");
    expect(mapStreamEventToMachineEvent("approval_required")).toBe(
      "APPROVAL_REQUIRED",
    );
    expect(mapStreamEventToMachineEvent("clarify_required")).toBe(
      "CLARIFY_REQUIRED",
    );
    expect(mapStreamEventToMachineEvent("approval_resolved")).toBe(
      "APPROVAL_RESOLVED",
    );
    expect(mapStreamEventToMachineEvent("clarify_resolved")).toBe(
      "CLARIFY_RESOLVED",
    );
    expect(mapStreamEventToMachineEvent("run_checkpointed")).toBe(
      "APPROVAL_REQUIRED",
    );
    expect(mapStreamEventToMachineEvent("run_retrying")).toBe("RUN_RETRYING");
    expect(mapStreamEventToMachineEvent("run_completed")).toBe("RUN_COMPLETED");
    expect(mapStreamEventToMachineEvent("run_failed")).toBe("RUN_FAILED");
    expect(mapStreamEventToMachineEvent("run_cancelled")).toBe("RUN_CANCELLED");
    expect(mapStreamEventToMachineEvent("run_interrupted")).toBe(
      "RUN_INTERRUPTED",
    );
    expect(mapStreamEventToMachineEvent("run_limit_reached")).toBe(
      "LIMIT_REACHED",
    );
  });

  it("returns null for non-lifecycle events", () => {
    expect(mapStreamEventToMachineEvent("message_delta")).toBeNull();
    expect(mapStreamEventToMachineEvent("tool_requested")).toBeNull();
    expect(mapStreamEventToMachineEvent("reasoning_updated")).toBeNull();
    expect(mapStreamEventToMachineEvent("unknown_event")).toBeNull();
  });
});
