import { describe, expect, it } from "vitest";
import { resolveThreadProfileId, resolveActiveThreadWorkbenchProfileId } from "./dashboard-workbench";

describe("resolveThreadProfileId", () => {
  const profileIds = new Set(["p-1", "p-2", "p-3", "p-global"]);
  const globalActive = "p-global";
  const firstProfile = "p-1";

  it("returns global active when threadId is null", () => {
    expect(
      resolveThreadProfileId(null, { "t-1": "p-2" }, profileIds, globalActive, firstProfile),
    ).toBe(globalActive);
  });

  it("returns global active when thread has no binding", () => {
    expect(
      resolveThreadProfileId("t-99", {}, profileIds, globalActive, firstProfile),
    ).toBe(globalActive);
  });

  it("returns the bound profile when it exists in profileIds", () => {
    const bindings = { "t-1": "p-2", "t-2": "p-3" };
    expect(
      resolveThreadProfileId("t-1", bindings, profileIds, globalActive, firstProfile),
    ).toBe("p-2");
    expect(
      resolveThreadProfileId("t-2", bindings, profileIds, globalActive, firstProfile),
    ).toBe("p-3");
  });

  it("prefers the thread binding over the global active profile", () => {
    expect(
      resolveThreadProfileId("t-1", { "t-1": "p-2" }, profileIds, "p-3", firstProfile),
    ).toBe("p-2");
  });

  it("falls back to firstProfileId when bound profile was deleted", () => {
    const bindings = { "t-1": "p-deleted" };
    expect(
      resolveThreadProfileId("t-1", bindings, profileIds, globalActive, firstProfile),
    ).toBe(firstProfile);
  });

  it("falls back to globalActive when bound profile was deleted and firstProfileId is null", () => {
    const bindings = { "t-1": "p-deleted" };
    expect(
      resolveThreadProfileId("t-1", bindings, profileIds, globalActive, null),
    ).toBe(globalActive);
  });

  it("returns global active when profileIds is empty and no binding", () => {
    expect(
      resolveThreadProfileId("t-1", {}, new Set(), globalActive, null),
    ).toBe(globalActive);
  });

  it("falls back correctly when profileIds is empty but binding exists", () => {
    const bindings = { "t-1": "p-1" };
    expect(
      resolveThreadProfileId("t-1", bindings, new Set(), globalActive, null),
    ).toBe(globalActive);
  });
});

describe("resolveActiveThreadWorkbenchProfileId", () => {
  const profileIds = new Set(["p-1", "p-2", "p-3", "p-global"]);
  const globalActive = "p-global";
  const firstProfile = "p-1";

  it("uses the global active profile in new thread mode", () => {
    expect(
      resolveActiveThreadWorkbenchProfileId(null, {}, null, profileIds, globalActive, firstProfile),
    ).toBe(globalActive);
  });

  it("prefers the persisted binding over the temporary active thread override", () => {
    expect(
      resolveActiveThreadWorkbenchProfileId(
        "t-1",
        { "t-1": "p-2" },
        "p-3",
        profileIds,
        globalActive,
        firstProfile,
      ),
    ).toBe("p-2");
  });

  it("uses the persisted binding for an existing thread when no override is set", () => {
    expect(
      resolveActiveThreadWorkbenchProfileId(
        "t-1",
        { "t-1": "p-2" },
        null,
        profileIds,
        globalActive,
        firstProfile,
      ),
    ).toBe("p-2");
  });

  it("keeps the active thread override when the global active profile changes", () => {
    expect(
      resolveActiveThreadWorkbenchProfileId(
        "t-1",
        {},
        "p-2",
        profileIds,
        "p-3",
        firstProfile,
      ),
    ).toBe("p-2");
  });

  it("falls back to the first profile when the remembered binding was deleted", () => {
    expect(
      resolveActiveThreadWorkbenchProfileId(
        "t-1",
        {},
        "p-deleted",
        profileIds,
        globalActive,
        firstProfile,
      ),
    ).toBe(firstProfile);
  });
});
