import { type KeyboardEvent, type MouseEvent, type RefObject, useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  BookOpenText,
  Bot,
  Box,
  Boxes,
  Bug,
  Chrome,
  CirclePlus,
  ExternalLink,
  FolderCog,
  GitBranch,
  Globe,
  LayoutTemplate,
  Plug,
  Search,
  Server,
  Sparkles,
  Workflow,
  Wrench,
  X,
} from "lucide-react";
import { MARKETPLACE_CATALOG } from "@/modules/marketplace-center/model/defaults";
import {
  persistMarketplaceActiveTab,
  readStoredMarketplaceActiveTab,
} from "@/modules/marketplace-center/model/storage";
import type {
  MarketplaceCatalogItem,
  MarketplaceDrawerTarget,
  MarketplaceItemIcon,
  MarketplaceStoredState,
  MarketplaceTab,
} from "@/modules/marketplace-center/model/types";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";

type MarketplaceOverlayProps = {
  contentRef: RefObject<HTMLDivElement | null>;
  itemStates: MarketplaceStoredState;
  onClose: () => void;
  onDisableItem: (itemId: string) => void;
  onEnableItem: (itemId: string) => void;
  onInstallItem: (itemId: string) => void;
  onUninstallItem: (itemId: string) => void;
};

type MarketplaceItemState = {
  enabled: boolean;
  installed: boolean;
};

const TAB_META: ReadonlyArray<{
  description: string;
  label: string;
  value: MarketplaceTab;
}> = [
  {
    value: "skills",
    label: "Skills",
    description: "Focused instructions and reusable workflows for everyday coding tasks.",
  },
  {
    value: "mcps",
    label: "MCP",
    description: "Live servers that connect the assistant to tools, APIs, and current data.",
  },
  {
    value: "plugins",
    label: "Plugins",
    description: "Workbench add-ons that extend desktop surfaces and task-oriented utilities.",
  },
  {
    value: "automations",
    label: "Automations",
    description: "Scheduled tasks and recurring workflows that run with minimal manual work.",
  },
];

const EMPTY_STATE_COPY: Record<"installed" | "recommended", Record<MarketplaceTab, string>> = {
  installed: {
    skills: "No installed skills match this search yet.",
    mcps: "No installed MCP match this search yet.",
    plugins: "No installed plugins match this search yet.",
    automations: "No installed automations match this search yet.",
  },
  recommended: {
    skills: "No recommended skills match the current search.",
    mcps: "No recommended MCP match the current search.",
    plugins: "No recommended plugins match the current search.",
    automations: "No recommended automations match the current search.",
  },
};

const SEARCH_PLACEHOLDER: Record<MarketplaceTab, string> = {
  skills: "Search skills, workflows, and prompt packs",
  mcps: "Search MCP, integrations, and tools",
  plugins: "Search plugins, add-ons, and desktop extensions",
  automations: "Search automations, schedules, and recurring tasks",
};

const TOOLBAR_TAB_ORDER: ReadonlyArray<MarketplaceTab> = ["plugins", "skills", "mcps", "automations"];

function getItemState(itemStates: MarketplaceStoredState, itemId: string): MarketplaceItemState {
  return itemStates[itemId] ?? { installed: false, enabled: false };
}

function getMarketplaceIcon(icon: MarketplaceItemIcon) {
  switch (icon) {
    case "books":
      return BookOpenText;
    case "bot":
      return Bot;
    case "box":
      return Box;
    case "bug":
      return Bug;
    case "chrome":
      return Chrome;
    case "database":
      return Server;
    case "folder":
      return FolderCog;
    case "git-branch":
      return GitBranch;
    case "globe":
      return Globe;
    case "layout":
      return LayoutTemplate;
    case "plug":
      return Plug;
    case "search":
      return Search;
    case "server":
      return Server;
    case "terminal":
      return Wrench;
    case "workflow":
      return Workflow;
    case "sparkles":
    default:
      return Sparkles;
  }
}

function getDrawerStatusMeta(state: MarketplaceItemState) {
  if (!state.installed) {
    return {
      label: "Available",
      className: "bg-app-warning/10 text-app-warning",
    };
  }

  if (state.enabled) {
    return {
      label: "Installed + Enabled",
      className: "bg-app-success/12 text-app-success",
    };
  }

  return {
    label: "Installed + Disabled",
    className: "bg-app-surface-muted/90 text-app-subtle",
  };
}

function handleCardKeyDown(event: KeyboardEvent<HTMLElement>, onOpen: () => void) {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    onOpen();
  }
}

function MarketplacePrimaryActionButton({
  itemId,
  state,
  onDisableItem,
  onEnableItem,
  onInstallItem,
}: {
  itemId: string;
  state: MarketplaceItemState;
  onDisableItem: (itemId: string) => void;
  onEnableItem: (itemId: string) => void;
  onInstallItem: (itemId: string) => void;
}) {
  const label = state.installed ? (state.enabled ? "Disable" : "Enable") : "Install";

  const handleClick = (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();

    if (!state.installed) {
      onInstallItem(itemId);
      return;
    }

    if (state.enabled) {
      onDisableItem(itemId);
      return;
    }

    onEnableItem(itemId);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    event.stopPropagation();
  };

  return (
    <Button
      size="sm"
      variant={state.installed ? "secondary" : "default"}
      className="h-7 rounded-lg px-2.5 text-[12px]"
      onClick={handleClick}
      onKeyDown={handleKeyDown}
    >
      {label}
    </Button>
  );
}

function MarketplaceEnabledSwitch({
  itemId,
  state,
  onDisableItem,
  onEnableItem,
}: {
  itemId: string;
  state: MarketplaceItemState;
  onDisableItem: (itemId: string) => void;
  onEnableItem: (itemId: string) => void;
}) {
  const handleCheckedChange = (checked: boolean) => {
    if (checked) {
      onEnableItem(itemId);
      return;
    }

    onDisableItem(itemId);
  };

  const stopPropagation = (event: MouseEvent<HTMLButtonElement> | KeyboardEvent<HTMLButtonElement>) => {
    event.stopPropagation();
  };

  return (
    <Switch
      size="sm"
      checked={state.enabled}
      aria-label={`${itemId} enabled state`}
      onCheckedChange={handleCheckedChange}
      onClick={stopPropagation}
      onKeyDown={stopPropagation}
    />
  );
}

function MarketplaceDrawerActionRow({
  itemId,
  state,
  onDisableItem,
  onEnableItem,
  onInstallItem,
  onUninstallItem,
}: {
  itemId: string;
  state: MarketplaceItemState;
  onDisableItem: (itemId: string) => void;
  onEnableItem: (itemId: string) => void;
  onInstallItem: (itemId: string) => void;
  onUninstallItem: (itemId: string) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <MarketplacePrimaryActionButton
        itemId={itemId}
        state={state}
        onDisableItem={onDisableItem}
        onEnableItem={onEnableItem}
        onInstallItem={onInstallItem}
      />
      {state.installed ? (
        <Button
          size="sm"
          variant="ghost"
          className="h-8 rounded-lg px-3 text-[12px] text-app-danger hover:bg-app-danger/10 hover:text-app-danger"
          onClick={() => onUninstallItem(itemId)}
        >
          Uninstall
        </Button>
      ) : null}
    </div>
  );
}

function MarketplaceItemCard({
  item,
  selected,
  state,
  onDisableItem,
  onEnableItem,
  onInstallItem,
  onOpenDetails,
}: {
  item: MarketplaceCatalogItem;
  selected: boolean;
  state: MarketplaceItemState;
  onDisableItem: (itemId: string) => void;
  onEnableItem: (itemId: string) => void;
  onInstallItem: (itemId: string) => void;
  onOpenDetails: (itemId: string) => void;
}) {
  const Icon = getMarketplaceIcon(item.icon);

  return (
    <article
      role="button"
      tabIndex={0}
      title="Open details"
      className={cn(
        "flex h-auto cursor-pointer flex-col self-start rounded-[18px] border px-3.5 py-3 text-left transition-[border-color,background-color,box-shadow] duration-200 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-app-border-strong",
        selected
          ? "border-app-border-strong bg-app-surface-active/82 shadow-[0_18px_40px_rgba(15,23,42,0.08)] dark:shadow-[0_18px_40px_rgba(0,0,0,0.18)]"
          : "border-app-border bg-app-surface/82 shadow-[0_14px_36px_rgba(15,23,42,0.06)] hover:border-app-border-strong hover:bg-app-surface/92 dark:shadow-[0_14px_36px_rgba(0,0,0,0.14)]",
      )}
      onClick={() => onOpenDetails(item.id)}
      onKeyDown={(event) => handleCardKeyDown(event, () => onOpenDetails(item.id))}
    >
      <div className="space-y-2.5">
        <div className="flex items-start justify-between gap-2.5">
          <div className="flex min-w-0 items-start gap-2.5">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-border bg-app-surface-muted/90 text-app-foreground">
              <Icon className="size-4" />
            </div>
            <div className="min-w-0">
              <h3 className="truncate text-sm font-semibold tracking-[-0.02em] text-app-foreground">{item.name}</h3>
              <p className="mt-1 line-clamp-2 text-[12px] leading-[1.45] text-app-muted">{item.summary}</p>
            </div>
          </div>

          <div className="flex shrink-0 items-start">
            {state.installed ? (
              <MarketplaceEnabledSwitch
                itemId={item.id}
                state={state}
                onDisableItem={onDisableItem}
                onEnableItem={onEnableItem}
              />
            ) : (
              <MarketplacePrimaryActionButton
                itemId={item.id}
                state={state}
                onDisableItem={onDisableItem}
                onEnableItem={onEnableItem}
                onInstallItem={onInstallItem}
              />
            )}
          </div>
        </div>
      </div>
    </article>
  );
}

function MarketplaceSection({
  emptyCopy,
  items,
  selectedItemId,
  title,
  itemStates,
  onDisableItem,
  onEnableItem,
  onInstallItem,
  onOpenDetails,
}: {
  emptyCopy: string;
  items: ReadonlyArray<MarketplaceCatalogItem>;
  selectedItemId: string | null;
  title: string;
  itemStates: MarketplaceStoredState;
  onDisableItem: (itemId: string) => void;
  onEnableItem: (itemId: string) => void;
  onInstallItem: (itemId: string) => void;
  onOpenDetails: (itemId: string) => void;
}) {
  return (
    <section className="space-y-3.5">
      <div className="flex items-center justify-between gap-3 border-b border-app-border/70 pb-2.5">
        <h2 className="text-[11px] font-semibold uppercase tracking-[0.18em] text-app-subtle">{title}</h2>
        <span className="rounded-full border border-app-border bg-app-surface-muted/70 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-app-subtle">
          {items.length}
        </span>
      </div>

      {items.length > 0 ? (
        <div className="grid items-start gap-3 md:grid-cols-3">
          {items.map((item) => (
            <MarketplaceItemCard
              key={item.id}
              item={item}
              selected={selectedItemId === item.id}
              state={getItemState(itemStates, item.id)}
              onDisableItem={onDisableItem}
              onEnableItem={onEnableItem}
              onInstallItem={onInstallItem}
              onOpenDetails={onOpenDetails}
            />
          ))}
        </div>
      ) : (
        <div className="rounded-[20px] border border-dashed border-app-border bg-app-surface-muted/45 px-4 py-5 text-[13px] text-app-muted">
          {emptyCopy}
        </div>
      )}
    </section>
  );
}

function MarketplaceSourceOption({
  description,
  icon: Icon,
  title,
}: {
  description: string;
  icon: typeof CirclePlus;
  title: string;
}) {
  return (
    <div className="flex items-start gap-3 rounded-2xl border border-app-border/70 bg-app-surface-muted/40 px-3.5 py-3.5">
      <div className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-border bg-app-surface text-app-foreground">
        <Icon className="size-4" />
      </div>
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <p className="text-sm font-semibold text-app-foreground">{title}</p>
          <span className="rounded-full bg-app-surface px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-app-subtle">
            Coming soon
          </span>
        </div>
        <p className="mt-1 text-[13px] leading-5 text-app-muted">{description}</p>
      </div>
    </div>
  );
}

function MarketplaceDrawer({
  drawerTarget,
  itemStates,
  onClose,
  onDisableItem,
  onEnableItem,
  onInstallItem,
  onUninstallItem,
}: {
  drawerTarget: MarketplaceDrawerTarget | null;
  itemStates: MarketplaceStoredState;
  onClose: () => void;
  onDisableItem: (itemId: string) => void;
  onEnableItem: (itemId: string) => void;
  onInstallItem: (itemId: string) => void;
  onUninstallItem: (itemId: string) => void;
}) {
  const isOpen = drawerTarget !== null;
  const selectedItem =
    drawerTarget?.mode === "item"
      ? MARKETPLACE_CATALOG.find((item) => item.id === drawerTarget.itemId) ?? null
      : null;
  const itemState = selectedItem ? getItemState(itemStates, selectedItem.id) : null;
  const DrawerIcon = selectedItem ? getMarketplaceIcon(selectedItem.icon) : CirclePlus;
  const drawerStatusMeta = itemState ? getDrawerStatusMeta(itemState) : null;

  return (
    <>
      <div
        className={cn(
          "absolute inset-0 z-10 bg-app-chrome/24 backdrop-blur-[1px] transition-opacity duration-200",
          isOpen ? "opacity-100" : "pointer-events-none opacity-0",
        )}
        onClick={onClose}
      />
      <aside
        className={cn(
          "absolute inset-y-0 right-0 z-20 flex w-full max-w-[400px] flex-col border-l border-app-border bg-app-menu/94 shadow-[-20px_0_48px_rgba(15,23,42,0.14)] backdrop-blur-xl transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] dark:shadow-[-20px_0_48px_rgba(0,0,0,0.3)]",
          isOpen ? "translate-x-0" : "translate-x-full",
        )}
      >
        <div className="flex items-start justify-between gap-4 border-b border-app-border/80 px-4 py-4">
          <div className="min-w-0">
            <div className="flex items-center gap-3">
              <div className="flex size-10 items-center justify-center rounded-2xl border border-app-border bg-app-surface-muted/80 text-app-foreground">
                <DrawerIcon className="size-[18px]" />
              </div>
              <div className="min-w-0">
                <p className="truncate text-[15px] font-semibold tracking-[-0.02em] text-app-foreground">
                  {drawerTarget?.mode === "item" ? selectedItem?.name : "Add source"}
                </p>
                <p className="mt-0.5 truncate text-[12px] text-app-subtle">
                  {drawerTarget?.mode === "item"
                    ? selectedItem?.sourceLabel
                    : "Prepare custom source and URL-based install flows"}
                </p>
              </div>
            </div>
          </div>
          <Button size="icon" variant="ghost" className="size-8 rounded-lg" onClick={onClose}>
            <X className="size-4" />
          </Button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-4 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {selectedItem && itemState && drawerStatusMeta ? (
            <div className="space-y-5">
              <section className="border-b border-app-border/70 pb-5">
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="rounded-full bg-app-surface-muted/75 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-app-subtle">
                    {selectedItem.version}
                  </span>
                  <span
                    className={cn(
                      "rounded-full px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.12em]",
                      drawerStatusMeta.className,
                    )}
                  >
                    {drawerStatusMeta.label}
                  </span>
                  {selectedItem.recommended ? (
                    <span className="rounded-full bg-app-surface-muted/75 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-app-subtle">
                      Recommended
                    </span>
                  ) : null}
                </div>
                <p className="mt-3 text-[13px] leading-6 text-app-muted">{selectedItem.description}</p>
              </section>

              <section className="border-b border-app-border/70 pb-5">
                <p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-app-subtle">Actions</p>
                <div className="mt-3">
                  <MarketplaceDrawerActionRow
                    itemId={selectedItem.id}
                    state={itemState}
                    onDisableItem={onDisableItem}
                    onEnableItem={onEnableItem}
                    onInstallItem={onInstallItem}
                    onUninstallItem={onUninstallItem}
                  />
                </div>
              </section>

              <section className="border-b border-app-border/70 pb-5">
                <p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-app-subtle">Details</p>
                <dl className="mt-3 space-y-3 text-[13px]">
                  <div className="flex items-start justify-between gap-4">
                    <dt className="text-app-subtle">Publisher</dt>
                    <dd className="text-right text-app-foreground">{selectedItem.publisher}</dd>
                  </div>
                  <div className="flex items-start justify-between gap-4">
                    <dt className="text-app-subtle">Source</dt>
                    <dd className="text-right text-app-foreground">{selectedItem.sourceLabel}</dd>
                  </div>
                  <div className="flex items-start justify-between gap-4">
                    <dt className="text-app-subtle">Category</dt>
                    <dd className="text-right text-app-foreground">
                      {TAB_META.find((tab) => tab.value === selectedItem.tab)?.label ?? selectedItem.tab}
                    </dd>
                  </div>
                </dl>
              </section>

              <section>
                <p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-app-subtle">Capabilities</p>
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {selectedItem.tags.map((tag) => (
                    <span
                      key={tag}
                      className="rounded-full border border-app-border bg-app-surface-muted/60 px-2 py-0.5 text-[10px] text-app-muted"
                    >
                      {tag}
                    </span>
                  ))}
                </div>
              </section>
            </div>
          ) : (
            <div className="space-y-3">
              <p className="text-[13px] leading-6 text-app-muted">
                Prepare curated team catalogs or direct package installs without leaving the workbench.
              </p>
              <MarketplaceSourceOption
                icon={CirclePlus}
                title="Custom source"
                description="Register organization catalogs, internal package feeds, or curated team bundles."
              />
              <MarketplaceSourceOption
                icon={ExternalLink}
                title="Install from URL"
                description="Paste a package URL, git repository, or managed source endpoint to install directly."
              />
            </div>
          )}
        </div>
      </aside>
    </>
  );
}

export function MarketplaceOverlay({
  contentRef,
  itemStates,
  onClose,
  onDisableItem,
  onEnableItem,
  onInstallItem,
  onUninstallItem,
}: MarketplaceOverlayProps) {
  const [activeTab, setActiveTab] = useState<MarketplaceTab>(() => readStoredMarketplaceActiveTab());
  const [searchByTab, setSearchByTab] = useState<Record<MarketplaceTab, string>>({
    skills: "",
    mcps: "",
    plugins: "",
    automations: "",
  });
  const [drawerTarget, setDrawerTarget] = useState<MarketplaceDrawerTarget | null>(null);

  const searchValue = searchByTab[activeTab];
  const normalizedQuery = searchValue.trim().toLowerCase();
  const activeTabMeta = TAB_META.find((tab) => tab.value === activeTab) ?? TAB_META[0];
  const selectedItemId = drawerTarget?.mode === "item" ? drawerTarget.itemId : null;
  const tabItems = useMemo(
    () => MARKETPLACE_CATALOG.filter((item) => item.tab === activeTab),
    [activeTab],
  );
  const filteredItems = useMemo(() => {
    if (!normalizedQuery) {
      return tabItems;
    }

    return tabItems.filter((item) => {
      const haystack = [
        item.name,
        item.summary,
        item.publisher,
        item.description,
        item.sourceLabel,
        ...item.tags,
      ]
        .join(" ")
        .toLowerCase();

      return haystack.includes(normalizedQuery);
    });
  }, [normalizedQuery, tabItems]);
  const installedItems = useMemo(
    () => filteredItems.filter((item) => getItemState(itemStates, item.id).installed),
    [filteredItems, itemStates],
  );
  const recommendedItems = useMemo(
    () => filteredItems.filter((item) => item.recommended && !getItemState(itemStates, item.id).installed),
    [filteredItems, itemStates],
  );
  useEffect(() => {
    persistMarketplaceActiveTab(activeTab);
  }, [activeTab]);

  useEffect(() => {
    if (!drawerTarget || drawerTarget.mode !== "item") {
      return;
    }

    const targetItem = MARKETPLACE_CATALOG.find((item) => item.id === drawerTarget.itemId);

    if (!targetItem || targetItem.tab !== activeTab) {
      setDrawerTarget(null);
    }
  }, [activeTab, drawerTarget]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || !drawerTarget) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      setDrawerTarget(null);
    };

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [drawerTarget]);

  return (
    <div className="fixed inset-x-0 bottom-0 top-9 z-[60] overflow-hidden bg-app-canvas text-app-foreground">
      <div className="flex h-full min-h-0 flex-col">
        <header className="shrink-0 border-b border-app-border bg-app-canvas/92 backdrop-blur-xl">
          <div className="mx-auto w-full max-w-6xl px-5 py-4">
            <div className="min-w-0">
              <button
                type="button"
                className="inline-flex items-center gap-2 text-[12px] text-app-muted transition-colors hover:text-app-foreground"
                onClick={onClose}
              >
                <ArrowLeft className="size-3.5" />
                <span>Back to app</span>
              </button>

              <div className="mt-3 flex items-start gap-3">
                <div className="flex size-10 items-center justify-center rounded-2xl border border-app-border bg-app-surface/80 text-app-foreground">
                  <Boxes className="size-4" />
                </div>
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <h1 className="text-[19px] font-semibold tracking-[-0.03em] text-app-foreground">Marketplace</h1>
                    <span className="rounded-full border border-app-border bg-app-surface-muted/70 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-app-subtle">
                      {activeTabMeta.label}
                    </span>
                  </div>
                  <p className="mt-1 text-[13px] leading-5 text-app-muted">
                    Browse skills, MCP servers, plugins, and scheduled automations in one catalog view.
                  </p>
                </div>
              </div>

              <div className="mt-4 flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                <div className="flex flex-wrap items-center gap-2">
                  {TOOLBAR_TAB_ORDER.map((tabValue) => {
                    const tab = TAB_META.find((item) => item.value === tabValue);

                    if (!tab) {
                      return null;
                    }

                    const isActive = tab.value === activeTab;

                    return (
                      <button
                        key={tab.value}
                        type="button"
                        className={cn(
                          "inline-flex h-8 items-center gap-2 rounded-lg border px-3 text-[12px] font-medium transition-colors",
                          isActive
                            ? "border-app-border-strong bg-app-surface text-app-foreground"
                            : "border-app-border bg-transparent text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                        )}
                        onClick={() => setActiveTab(tab.value)}
                      >
                        <span>{tab.label}</span>
                        <span className="rounded-full bg-app-surface-muted px-1.5 py-0.5 text-[10px] text-app-subtle">
                          {MARKETPLACE_CATALOG.filter((item) => item.tab === tab.value).length}
                        </span>
                      </button>
                    );
                  })}
                </div>

                <div className="flex flex-col gap-3 sm:flex-row sm:items-center lg:justify-end">
                  <div className="relative min-w-0 flex-1 sm:w-[320px] lg:flex-none">
                    <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-app-subtle" />
                    <Input
                      value={searchValue}
                      onChange={(event) =>
                        setSearchByTab((current) => ({
                          ...current,
                          [activeTab]: event.target.value,
                        }))
                      }
                      placeholder={SEARCH_PLACEHOLDER[activeTab]}
                      className="h-9 rounded-xl border-app-border bg-app-surface-muted/80 pl-10 text-[13px]"
                    />
                  </div>

                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 shrink-0 self-start rounded-lg bg-app-surface/70 px-3 text-[12px] sm:self-auto"
                    onClick={() => setDrawerTarget({ mode: "source-intake" })}
                  >
                    <CirclePlus className="size-4" />
                    Add source
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </header>

        <div ref={contentRef} className="relative min-h-0 flex-1">
          <div className="h-full overflow-y-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <div className="mx-auto flex w-full max-w-6xl flex-col gap-5 px-5 pb-6 pt-5">
              <MarketplaceSection
                title="Installed"
                items={installedItems}
                selectedItemId={selectedItemId}
                emptyCopy={EMPTY_STATE_COPY.installed[activeTab]}
                itemStates={itemStates}
                onDisableItem={onDisableItem}
                onEnableItem={onEnableItem}
                onInstallItem={onInstallItem}
                onOpenDetails={(itemId) => setDrawerTarget({ mode: "item", itemId })}
              />

              <MarketplaceSection
                title="Recommended"
                items={recommendedItems}
                selectedItemId={selectedItemId}
                emptyCopy={EMPTY_STATE_COPY.recommended[activeTab]}
                itemStates={itemStates}
                onDisableItem={onDisableItem}
                onEnableItem={onEnableItem}
                onInstallItem={onInstallItem}
                onOpenDetails={(itemId) => setDrawerTarget({ mode: "item", itemId })}
              />
            </div>
          </div>

          <MarketplaceDrawer
            drawerTarget={drawerTarget}
            itemStates={itemStates}
            onClose={() => setDrawerTarget(null)}
            onDisableItem={onDisableItem}
            onEnableItem={onEnableItem}
            onInstallItem={onInstallItem}
            onUninstallItem={onUninstallItem}
          />
        </div>
      </div>
    </div>
  );
}
