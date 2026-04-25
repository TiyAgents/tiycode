import { describe, expect, it } from "vitest";
import { resolveThreadProfileId, resolveActiveThreadWorkbenchProfileId } from "./dashboard-workbench-logic";

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
