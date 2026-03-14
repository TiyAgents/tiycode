import {
  MARKETPLACE_ACTIVE_TAB_STORAGE_KEY,
  DEFAULT_MARKETPLACE_STATE,
  MARKETPLACE_CATALOG,
  MARKETPLACE_STORAGE_KEY,
} from "@/modules/marketplace-center/model/defaults";
import type {
  MarketplaceItemState,
  MarketplaceStoredState,
  MarketplaceTab,
} from "@/modules/marketplace-center/model/types";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isMarketplaceItemState(value: unknown): value is MarketplaceItemState {
  return isRecord(value) && typeof value.installed === "boolean" && typeof value.enabled === "boolean";
}

function isMarketplaceTab(value: unknown): value is MarketplaceTab {
  return value === "skills" || value === "mcps" || value === "plugins" || value === "automations";
}

export function readStoredMarketplaceState(): MarketplaceStoredState {
  if (typeof window === "undefined") {
    return DEFAULT_MARKETPLACE_STATE;
  }

  const rawValue = window.localStorage.getItem(MARKETPLACE_STORAGE_KEY);

  if (!rawValue) {
    return DEFAULT_MARKETPLACE_STATE;
  }

  try {
    const parsed = JSON.parse(rawValue) as unknown;

    if (!isRecord(parsed)) {
      return DEFAULT_MARKETPLACE_STATE;
    }

    const knownIds = new Set(MARKETPLACE_CATALOG.map((item) => item.id));
    const parsedEntries = Object.entries(parsed).filter(
      (entry): entry is [string, MarketplaceItemState] => knownIds.has(entry[0]) && isMarketplaceItemState(entry[1]),
    );

    return {
      ...DEFAULT_MARKETPLACE_STATE,
      ...Object.fromEntries(parsedEntries),
    };
  } catch {
    return DEFAULT_MARKETPLACE_STATE;
  }
}

export function persistMarketplaceState(state: MarketplaceStoredState) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(MARKETPLACE_STORAGE_KEY, JSON.stringify(state));
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
