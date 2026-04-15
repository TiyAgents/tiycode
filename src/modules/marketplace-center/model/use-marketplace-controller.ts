import { MARKETPLACE_CATALOG } from "@/modules/marketplace-center/model/defaults";

export function useMarketplaceController() {
  return {
    catalog: MARKETPLACE_CATALOG,
  };
}
