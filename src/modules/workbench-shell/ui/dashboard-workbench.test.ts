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
      // 1200 (input) + 300 (output) + 10 (cache_read) + 5 (cache_write) = 1515.
      // Mirrors `Usage::context_size()` from tiycore 0.2.10-rc.2
      // (= input + output + cache_read + cache_write). Tests can override
      // contextSize / totalTokens independently to assert the cross-protocol
      // unified semantics.
      contextSize: 1_515,
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
    // contextSize is the new "used" figure, distinct from totalTokens.
    expect(badge.contextSize).toBe(1_515);
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

  it("uses contextSize (not totalTokens) for the percentage when contextSize is larger", () => {
    // The cross-protocol unified `contextSize` is the badge's "used" figure.
    // Even when the wire-level `totalTokens` is below the context window,
    // `contextSize` above the window should mark the badge as exceeded.
    // This mirrors Anthropic: total_tokens (wire) excludes cache_read, but
    // `context_size` adds it back.
    const badge = buildThreadContextBadgeData({
      fallbackContextWindow: "1000",
      fallbackModelDisplayName: "Small Model",
      runtimeUsage: makeRuntimeUsage({
        contextSize: 1_250, // exceeds 1000
        totalTokens: 900, // under 1000 (wire-level)
      }),
    });

    expect(badge.isExceeded).toBe(true);
    expect(badge.rawUsedPercent).toBe(125);
    expect(badge.usedPercent).toBe(100);
    expect(badge.leftPercent).toBe(0);
    expect(badge.usageRatio).toBe(1);
  });

  it("uses contextSize as the percentage source when it diverges from totalTokens", () => {
    // When wire-level `totalTokens` exceeds the window but the unified
    // `contextSize` does not, the badge reflects the unified value
    // (the new "context occupancy" source of truth from
    // `Usage::context_size()`). Wire-level `totalTokens` is retained on
    // the DTO for downstream reporting; it is NOT used for the badge
    // percentage anymore.
    const badge = buildThreadContextBadgeData({
      fallbackContextWindow: "1000",
      fallbackModelDisplayName: "Small Model",
      runtimeUsage: makeRuntimeUsage({
        contextSize: 800, // under 1000
        totalTokens: 1_250, // over 1000 (wire-level)
      }),
    });

    expect(badge.isExceeded).toBe(false);
    expect(badge.rawUsedPercent).toBe(80);
    // The DTO still carries the wire-level total for consumers that
    // want it; the badge just doesn't use it for percentages.
    expect(badge.totalTokens).toBe(1_250);
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
