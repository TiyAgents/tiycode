import {
  MARKETPLACE_ACTIVE_TAB_STORAGE_KEY,
} from "@/modules/marketplace-center/model/defaults";
import type {
  MarketplaceTab,
} from "@/modules/marketplace-center/model/types";

function isMarketplaceTab(value: unknown): value is MarketplaceTab {
  return value === "skills" || value === "mcps" || value === "plugins" || value === "automations";
}

export function readStoredMarketplaceActiveTab(): MarketplaceTab {
  if (typeof window === "undefined") {
    return "plugins";
  }

  const rawValue = window.localStorage.getItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY);

  return isMarketplaceTab(rawValue) ? rawValue : "plugins";
}

export function persistMarketplaceActiveTab(tab: MarketplaceTab) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(MARKETPLACE_ACTIVE_TAB_STORAGE_KEY, tab);
}
