import { describe, expect, it } from "vitest";

import {
  applyBackendElapsedSeed,
  clearTimerSlot,
  computeTimerTransition,
  formatElapsedTime,
  isThreadTimerRunning,
  type ThreadTimerSlot,
} from "./use-thread-elapsed-timer";

describe("useThreadElapsedTimer helpers", () => {
  describe("isThreadTimerRunning", () => {
    it("returns true only for 'running'", () => {
      expect(isThreadTimerRunning("running")).toBe(true);
    });

    it.each([
      "idle",
      "waiting_approval",
      "needs_reply",
      "completed",
      "failed",
      "cancelled",
      "interrupted",
      "limit_reached",
    ] as const)("returns false for '%s'", (status) => {
      expect(isThreadTimerRunning(status)).toBe(false);
    });

    it("returns false for undefined", () => {
      expect(isThreadTimerRunning(undefined)).toBe(false);
    });
  });

  describe("computeTimerTransition", () => {
    it("freezes while waiting and resumes from the frozen elapsed value", () => {
      // 1. started
      const started = computeTimerTransition({
        isTimerRunning: true,
        previousElapsedSeconds: 0,
        previousBaseElapsedSeconds: 0,
        previousStartedAtMs: null,
        nowMs: 1_000,
      });
      expect(started).toEqual({
        elapsedSeconds: 0,
        baseElapsedSeconds: 0,
        startedAtMs: 1_000,
      });

      // 2. advanced (2.4 s elapsed → 2 full seconds)
      const advanced = computeTimerTransition({
        isTimerRunning: true,
        previousElapsedSeconds: started.elapsedSeconds,
        previousBaseElapsedSeconds: started.baseElapsedSeconds,
        previousStartedAtMs: started.startedAtMs,
        nowMs: 3_400,
      });
      expect(advanced.elapsedSeconds).toBe(2);

      // 3. frozen (user-action pause)
      const frozen = computeTimerTransition({
        isTimerRunning: false,
        previousElapsedSeconds: advanced.elapsedSeconds,
        previousBaseElapsedSeconds: advanced.baseElapsedSeconds,
        previousStartedAtMs: advanced.startedAtMs,
        nowMs: 10_000,
      });
      expect(frozen).toEqual({
        elapsedSeconds: 2,
        baseElapsedSeconds: 2,
        startedAtMs: null,
      });

      // 4. resumed
      const resumed = computeTimerTransition({
        isTimerRunning: true,
        previousElapsedSeconds: frozen.elapsedSeconds,
        previousBaseElapsedSeconds: frozen.baseElapsedSeconds,
        previousStartedAtMs: frozen.startedAtMs,
        nowMs: 10_000,
      });
      expect(resumed).toEqual({
        elapsedSeconds: 2,
        baseElapsedSeconds: 2,
        startedAtMs: 10_000,
      });

      // 5. resumed + advanced (2.1 s after resume → 2 + 2 = 4 total)
      const resumedAdvanced = computeTimerTransition({
        isTimerRunning: true,
        previousElapsedSeconds: resumed.elapsedSeconds,
        previousBaseElapsedSeconds: resumed.baseElapsedSeconds,
        previousStartedAtMs: resumed.startedAtMs,
        nowMs: 12_100,
      });
      expect(resumedAdvanced.elapsedSeconds).toBe(4);
    });

    it("continues running from a historical backend seed", () => {
      const resumed = computeTimerTransition({
        isTimerRunning: true,
        previousElapsedSeconds: 120,
        previousBaseElapsedSeconds: 120,
        previousStartedAtMs: null,
        nowMs: 5_000,
      });
      expect(resumed).toEqual({
        elapsedSeconds: 120,
        baseElapsedSeconds: 120,
        startedAtMs: 5_000,
      });

      const advanced = computeTimerTransition({
        isTimerRunning: true,
        previousElapsedSeconds: resumed.elapsedSeconds,
        previousBaseElapsedSeconds: resumed.baseElapsedSeconds,
        previousStartedAtMs: resumed.startedAtMs,
        nowMs: 8_200,
      });
      expect(advanced.elapsedSeconds).toBe(123);
    });
  });

  describe("formatElapsedTime", () => {
    it("formats seconds-only", () => {
      expect(formatElapsedTime(0)).toBe("0s");
      expect(formatElapsedTime(45)).toBe("45s");
      expect(formatElapsedTime(59)).toBe("59s");
    });

    it("formats minutes + seconds", () => {
      expect(formatElapsedTime(60)).toBe("1m 0s");
      expect(formatElapsedTime(90)).toBe("1m 30s");
      expect(formatElapsedTime(3599)).toBe("59m 59s");
    });

    it("formats hours + minutes", () => {
      expect(formatElapsedTime(3600)).toBe("1h 0m");
      expect(formatElapsedTime(3661)).toBe("1h 1m");
      expect(formatElapsedTime(7260)).toBe("2h 1m");
    });
  });

  describe("applyBackendElapsedSeed", () => {
    const makeSlot = (overrides: Partial<ThreadTimerSlot> = {}): ThreadTimerSlot => ({
      elapsed: 0,
      baseElapsed: 0,
      startedAt: null,
      seeded: false,
      ...overrides,
    });

    it("raises a frozen slot to a larger backend history seed", () => {
      const slot = makeSlot({ elapsed: 12, baseElapsed: 12, seeded: true });

      applyBackendElapsedSeed(slot, 42);

      expect(slot).toEqual({
        elapsed: 42,
        baseElapsed: 42,
        startedAt: null,
        seeded: true,
      });
    });

    it("does not lower an existing slot from an older backend snapshot", () => {
      const slot = makeSlot({ elapsed: 42, baseElapsed: 42, seeded: true });

      applyBackendElapsedSeed(slot, 12);

      expect(slot.elapsed).toBe(42);
      expect(slot.baseElapsed).toBe(42);
    });

    it("does not overwrite an actively running slot", () => {
      const slot = makeSlot({
        elapsed: 45,
        baseElapsed: 40,
        startedAt: 1_000,
        seeded: true,
      });

      applyBackendElapsedSeed(slot, 90);

      expect(slot).toEqual({
        elapsed: 45,
        baseElapsed: 40,
        startedAt: 1_000,
        seeded: true,
      });
    });
  });

  describe("clearTimerSlot", () => {
    it("is exported and callable without throwing", () => {
      expect(typeof clearTimerSlot).toBe("function");
      // Idempotent on unknown ids.
      expect(() => clearTimerSlot("never-seen-thread-id")).not.toThrow();
    });
  });
});
