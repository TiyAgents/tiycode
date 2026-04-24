import { beforeEach, describe, expect, it } from "vitest";
import {
  readStoredMarketplaceActiveTab,
  persistMarketplaceActiveTab,
} from "@/modules/marketplace-center/model/storage";
import {
  MARKETPLACE_ACTIVE_TAB_STORAGE_KEY,
  MARKETPLACE_CATALOG,
} from "@/modules/marketplace-center/model/defaults";

beforeEach(() => {
  window.localStorage.clear();
});

describe("readStoredMarketplaceActiveTab", () => {
  it("returns 'plugins' as default when nothing is stored", () => {
    expect(readStoredMarketplaceActiveTab()).toBe("plugins");
  });

  it("returns stored value when it is a valid tab", () => {
    window.localStorage.setItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY, "skills");
    expect(readStoredMarketplaceActiveTab()).toBe("skills");
  });

  it("returns stored value for 'mcps'", () => {
    window.localStorage.setItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY, "mcps");
    expect(readStoredMarketplaceActiveTab()).toBe("mcps");
  });

  it("returns stored value for 'automations'", () => {
    window.localStorage.setItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY, "automations");
    expect(readStoredMarketplaceActiveTab()).toBe("automations");
  });

  it("returns 'plugins' when stored value is invalid", () => {
    window.localStorage.setItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY, "invalid-tab");
    expect(readStoredMarketplaceActiveTab()).toBe("plugins");
  });

  it("returns 'plugins' when stored value is empty string", () => {
    window.localStorage.setItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY, "");
    expect(readStoredMarketplaceActiveTab()).toBe("plugins");
  });
});

describe("persistMarketplaceActiveTab", () => {
  it("writes the tab to localStorage", () => {
    persistMarketplaceActiveTab("skills");
    expect(window.localStorage.getItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY)).toBe("skills");
  });

  it("overwrites previously stored tab", () => {
    persistMarketplaceActiveTab("skills");
    persistMarketplaceActiveTab("mcps");
    expect(window.localStorage.getItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY)).toBe("mcps");
  });
});

describe("MARKETPLACE_CATALOG data integrity", () => {
  it("has entries", () => {
    expect(MARKETPLACE_CATALOG.length).toBeGreaterThan(0);
  });

  it("every item has a valid tab", () => {
    const validTabs = new Set(["skills", "mcps", "plugins", "automations"]);
    for (const item of MARKETPLACE_CATALOG) {
      expect(validTabs.has(item.tab)).toBe(true);
    }
  });

  it("every item has a unique id", () => {
    const ids = MARKETPLACE_CATALOG.map((item) => item.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("every item has required string fields", () => {
    for (const item of MARKETPLACE_CATALOG) {
      expect(item.id).toBeTruthy();
      expect(item.name).toBeTruthy();
      expect(item.summary).toBeTruthy();
      expect(item.description).toBeTruthy();
      expect(item.publisher).toBeTruthy();
      expect(item.version).toBeTruthy();
      expect(item.icon).toBeTruthy();
      expect(item.sourceLabel).toBeTruthy();
    }
  });

  it("every item has a non-empty tags array", () => {
    for (const item of MARKETPLACE_CATALOG) {
      expect(Array.isArray(item.tags)).toBe(true);
      expect(item.tags.length).toBeGreaterThan(0);
    }
  });

  it("has items for each tab category", () => {
    const tabCounts = new Map<string, number>();
    for (const item of MARKETPLACE_CATALOG) {
      tabCounts.set(item.tab, (tabCounts.get(item.tab) ?? 0) + 1);
    }
    expect(tabCounts.get("skills")).toBeGreaterThan(0);
    expect(tabCounts.get("mcps")).toBeGreaterThan(0);
    expect(tabCounts.get("plugins")).toBeGreaterThan(0);
    expect(tabCounts.get("automations")).toBeGreaterThan(0);
  });
});
