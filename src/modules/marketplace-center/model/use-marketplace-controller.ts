import { useEffect, useState } from "react";
import { MARKETPLACE_CATALOG } from "@/modules/marketplace-center/model/defaults";
import { persistMarketplaceState, readStoredMarketplaceState } from "@/modules/marketplace-center/model/storage";
import type { MarketplaceStoredState } from "@/modules/marketplace-center/model/types";

export function useMarketplaceController() {
  const [itemStates, setItemStates] = useState<MarketplaceStoredState>(() => readStoredMarketplaceState());

  useEffect(() => {
    persistMarketplaceState(itemStates);
  }, [itemStates]);

  const installItem = (itemId: string) => {
    setItemStates((current) => ({
      ...current,
      [itemId]: {
        installed: true,
        enabled: true,
      },
    }));
  };

  const uninstallItem = (itemId: string) => {
    setItemStates((current) => ({
      ...current,
      [itemId]: {
        installed: false,
        enabled: false,
      },
    }));
  };

  const enableItem = (itemId: string) => {
    setItemStates((current) => {
      const state = current[itemId];

      if (!state?.installed) {
        return current;
      }

      return {
        ...current,
        [itemId]: {
          installed: true,
          enabled: true,
        },
      };
    });
  };

  const disableItem = (itemId: string) => {
    setItemStates((current) => {
      const state = current[itemId];

      if (!state?.installed) {
        return current;
      }

      return {
        ...current,
        [itemId]: {
          installed: true,
          enabled: false,
        },
      };
    });
  };

  return {
    catalog: MARKETPLACE_CATALOG,
    itemStates,
    installItem,
    uninstallItem,
    enableItem,
    disableItem,
  };
}
