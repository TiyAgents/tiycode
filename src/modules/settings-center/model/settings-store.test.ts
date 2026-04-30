import { describe, expect, it, beforeEach } from "vitest";
import { settingsStore } from "./settings-store";
import type { HydrationPhase } from "./settings-store";

beforeEach(() => {
  settingsStore.reset();
});

describe("settingsStore", () => {
  describe("initial state", () => {
    it("starts with hydrationPhase uninitialized", () => {
      expect(settingsStore.getState().hydrationPhase).toBe("uninitialized");
    });

    it("has empty providers array", () => {
      expect(settingsStore.getState().providers).toEqual([]);
    });

    it("has default agent profiles", () => {
      expect(settingsStore.getState().agentProfiles.length).toBeGreaterThan(0);
    });

    it("has default active agent profile", () => {
      expect(settingsStore.getState().activeAgentProfileId).toBeTruthy();
      expect(typeof settingsStore.getState().activeAgentProfileId).toBe("string");
    });

    it("has empty workspaces", () => {
      expect(settingsStore.getState().workspaces).toEqual([]);
    });

    it("has default general preferences", () => {
      const general = settingsStore.getState().general;
      expect(typeof general.launchAtLogin).toBe("boolean");
      expect(typeof general.preventSleepWhileRunning).toBe("boolean");
      expect(typeof general.minimizeToTray).toBe("boolean");
    });

    it("has default terminal settings", () => {
      const terminal = settingsStore.getState().terminal;
      expect(typeof terminal.shellPath).toBe("string");
      expect(typeof terminal.fontSize).toBe("number");
    });

    it("has default built-in commands", () => {
      const commands = settingsStore.getState().commands;
      expect(commands.length).toBeGreaterThan(0);
      expect(commands.some((c) => c.id === "cmd-commit")).toBe(true);
      expect(commands.some((c) => c.id === "cmd-create-pr")).toBe(true);
    });

    it("has empty available shells", () => {
      expect(settingsStore.getState().availableShells).toEqual([]);
    });
  });

  describe("setState", () => {
    it("partially merges state", () => {
      settingsStore.setState({ hydrationPhase: "loading_phase1" });
      expect(settingsStore.getState().hydrationPhase).toBe("loading_phase1");
      // Other fields should be unchanged
      expect(settingsStore.getState().providers).toEqual([]);
    });

    it("updates via function form", () => {
      settingsStore.setState({ hydrationPhase: "loading_phase1" });
      settingsStore.setState((prev) => ({
        hydrationPhase: "phase1_ready",
        providers: [...prev.providers, { id: "p1", kind: "builtin" } as never],
      }));
      expect(settingsStore.getState().hydrationPhase).toBe("phase1_ready");
      expect(settingsStore.getState().providers).toHaveLength(1);
    });
  });

  describe("hydrationPhase lifecycle", () => {
    const phases: HydrationPhase[] = [
      "uninitialized",
      "loading_phase1",
      "phase1_ready",
      "loading_phase2",
      "hydrated",
      "error",
    ];

    it.each(phases)("sets hydrationPhase to %s", (phase) => {
      settingsStore.setState({ hydrationPhase: phase });
      expect(settingsStore.getState().hydrationPhase).toBe(phase);
    });
  });

  describe("reset", () => {
    it("returns to initial state after modifications", () => {
      settingsStore.setState({
        hydrationPhase: "hydrated",
        providers: [{ id: "p1" } as never],
      });

      settingsStore.reset();

      expect(settingsStore.getState().hydrationPhase).toBe("uninitialized");
      expect(settingsStore.getState().providers).toEqual([]);
      expect(settingsStore.getState().activeAgentProfileId).toBeTruthy();
    });
  });

  describe("subscribe", () => {
    it("calls listener on state change", () => {
      let called = false;
      const unsub = settingsStore.subscribe(() => {
        called = true;
      });

      settingsStore.setState({ hydrationPhase: "loading_phase1" });
      expect(called).toBe(true);

      unsub();
    });

    it("returns unsubscribe function that stops notifications", () => {
      let callCount = 0;
      const unsub = settingsStore.subscribe(() => {
        callCount++;
      });

      settingsStore.setState({ hydrationPhase: "loading_phase1" });
      expect(callCount).toBe(1);

      unsub();
      settingsStore.setState({ hydrationPhase: "phase1_ready" });
      expect(callCount).toBe(1);
    });
  });
});
