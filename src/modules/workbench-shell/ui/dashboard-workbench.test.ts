import { describe, expect, it } from "vitest";
import {
  buildThreadContextBadgeData,
  resolveThreadProfileId,
  resolveActiveThreadWorkbenchProfileId,
} from "./dashboard-workbench-logic";
import type { ThreadContextUsage } from "@/modules/workbench-shell/model/thread-store";

describe("resolveThreadProfileId", () => {
  const globalActive = "p-global";

  it("returns the global active profile when the thread has no persisted profile", () => {
    expect(resolveThreadProfileId(null, globalActive)).toBe(globalActive);
  });

  it("returns the persisted thread profile when present", () => {
    expect(resolveThreadProfileId("p-thread", globalActive)).toBe("p-thread");
  });

  it("preserves deleted profile ids instead of silently falling back", () => {
    expect(resolveThreadProfileId("p-deleted", globalActive)).toBe("p-deleted");
  });

  it("falls back to global active profile when thread profile is an empty string", () => {
    expect(resolveThreadProfileId("", globalActive)).toBe(globalActive);
  });
});

describe("resolveActiveThreadWorkbenchProfileId", () => {
  const globalActive = "p-global";

  it("uses the global active profile in new thread mode", () => {
    expect(resolveActiveThreadWorkbenchProfileId(null, globalActive)).toBe(globalActive);
  });

  it("uses the thread persisted profile for existing threads", () => {
    expect(resolveActiveThreadWorkbenchProfileId("p-thread", globalActive)).toBe("p-thread");
  });

  it("keeps deleted profile ids for existing threads so the UI can show missing state", () => {
    expect(resolveActiveThreadWorkbenchProfileId("p-deleted", globalActive)).toBe("p-deleted");
  });

  it("falls back to global active profile when thread profile is an empty string", () => {
    expect(resolveActiveThreadWorkbenchProfileId("", globalActive)).toBe(globalActive);
  });
});

describe("buildThreadContextBadgeData", () => {
  function makeRuntimeUsage(
    overrides: Partial<ThreadContextUsage> = {},
  ): ThreadContextUsage {
    return {
      cacheReadTokens: 10,
      cacheWriteTokens: 5,
      contextWindow: "8k",
      inputTokens: 1_200,
      modelDisplayName: "Old Runtime Model",
      outputTokens: 300,
      runId: "run-1",
      totalTokens: 1_500,
      ...overrides,
    };
  }

  it("uses the selected model context window before stale runtime usage", () => {
    const badge = buildThreadContextBadgeData({
      fallbackContextWindow: "16000",
      fallbackModelDisplayName: "Selected Model",
      runtimeUsage: makeRuntimeUsage({
        contextWindow: "4000",
        modelDisplayName: "Old Runtime Model",
      }),
    });

    expect(badge.contextWindow).toBe(16_000);
    expect(badge.modelDisplayName).toBe("Selected Model");
    expect(badge.totalTokens).toBe(1_500);
    expect(badge.isExceeded).toBe(false);
  });

  it("falls back to runtime context window when the selected model has none", () => {
    const badge = buildThreadContextBadgeData({
      fallbackContextWindow: null,
      fallbackModelDisplayName: null,
      runtimeUsage: makeRuntimeUsage({
        contextWindow: "32000",
        modelDisplayName: "Runtime Model",
      }),
    });

    expect(badge.contextWindow).toBe(32_000);
    expect(badge.modelDisplayName).toBe("Runtime Model");
    expect(badge.totalLabel).toBe("32K");
  });

  it("marks usage as exceeded when used tokens are over the current context window", () => {
    const badge = buildThreadContextBadgeData({
      fallbackContextWindow: "1000",
      fallbackModelDisplayName: "Small Model",
      runtimeUsage: makeRuntimeUsage({ totalTokens: 1_250 }),
    });

    expect(badge.isExceeded).toBe(true);
    expect(badge.rawUsedPercent).toBe(125);
    expect(badge.usedPercent).toBe(100);
    expect(badge.leftPercent).toBe(0);
    expect(badge.usageRatio).toBe(1);
  });

  it("does not exceed when no valid context window is available", () => {
    const badge = buildThreadContextBadgeData({
      fallbackContextWindow: null,
      fallbackModelDisplayName: "Selected Model",
      runtimeUsage: makeRuntimeUsage({ contextWindow: null }),
    });

    expect(badge.contextWindow).toBeNull();
    expect(badge.isExceeded).toBe(false);
    expect(badge.rawUsedPercent).toBe(0);
    expect(badge.totalLabel).toBe("N/A");
  });
});
