export type MarketplaceTab = "skills" | "mcps" | "plugins" | "automations";

export type MarketplaceItemIcon =
  | "bot"
  | "books"
  | "box"
  | "bug"
  | "chrome"
  | "database"
  | "folder"
  | "git-branch"
  | "globe"
  | "layout"
  | "plug"
  | "search"
  | "server"
  | "sparkles"
  | "terminal"
  | "workflow";

export type MarketplaceCatalogItem = {
  id: string;
  tab: MarketplaceTab;
  name: string;
  summary: string;
  description: string;
  publisher: string;
  version: string;
  tags: Array<string>;
  recommended: boolean;
  icon: MarketplaceItemIcon;
  sourceLabel: string;
};

export type MarketplaceItemState = {
  installed: boolean;
  enabled: boolean;
};

export type MarketplaceStoredState = Record<string, MarketplaceItemState>;

export type MarketplaceDrawerTarget =
  | {
      mode: "item";
      itemId: string;
    }
  | {
      mode: "source-intake";
    };
