import { useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Eye,
  EyeOff,
  Plus,
  Server,
} from "lucide-react";
import { useT } from "@/i18n";
import { matchProviderIcon } from "@/shared/lib/llm-brand-matcher";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { LocalLlmIcon } from "@/shared/ui/local-llm-icon";
import { Switch } from "@/shared/ui/switch";
import type {
  CustomProviderType,
  ProviderCatalogEntry,
  ProviderEntry,
} from "@/modules/settings-center/model/types";

function ProviderIcon({ name, className }: { name: string; className?: string }) {
  const slug = matchProviderIcon(name);

  if (slug) {
    return <LocalLlmIcon slug={slug} title={name} className={cn("text-app-muted", className)} />;
  }

  const initial = name.trim() ? name.trim().charAt(0).toUpperCase() : "?";

  return (
    <div
      className={cn(
        "flex items-center justify-center rounded-lg bg-app-surface-muted text-[11px] font-semibold text-app-muted",
        className,
      )}
    >
      {initial}
    </div>
  );
}

type ProviderStepProps = {
  providerCatalog: Array<ProviderCatalogEntry>;
  providers: Array<ProviderEntry>;
  selectedProviderId: string | null;
  onSelectProvider: (id: string | null) => void;
  apiKeyDrafts: Record<string, string>;
  onApiKeyDraftsChange: (drafts: Record<string, string>) => void;
  onAddProvider: (entry: Omit<ProviderEntry, "id">) => void;
  onUpdateProvider: (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => void;
  onFetchProviderModels: (id: string) => Promise<void>;
};

export function ProviderStep({
  providerCatalog,
  providers,
  selectedProviderId,
  onSelectProvider,
  apiKeyDrafts,
  onApiKeyDraftsChange,
  onAddProvider,
  onUpdateProvider,
  onFetchProviderModels,
}: ProviderStepProps) {
  const t = useT();
  const [showApiKey, setShowApiKey] = useState(false);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [modelSearch, setModelSearch] = useState("");
  const [fetchFeedback, setFetchFeedback] = useState<{
    kind: "success" | "error";
    message?: string;
  } | null>(null);
  const [showCatalogPicker, setShowCatalogPicker] = useState(false);
  const catalogPickerRef = useRef<HTMLDivElement>(null);
  const apiKeySaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const selectedProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? null;

  // Derive apiKeyDraft from lifted state; fall back to provider's stored key
  const apiKeyDraft = selectedProviderId
    ? (apiKeyDrafts[selectedProviderId] ?? selectedProvider?.apiKey ?? "")
    : "";

  const setApiKeyDraft = (value: string) => {
    if (!selectedProviderId) return;
    onApiKeyDraftsChange({ ...apiKeyDrafts, [selectedProviderId]: value });
  };

  // Refs that track latest values so the unmount flush closure can read them
  const apiKeyDraftRef = useRef(apiKeyDraft);
  apiKeyDraftRef.current = apiKeyDraft;
  const selectedProviderIdRef = useRef(selectedProviderId);
  selectedProviderIdRef.current = selectedProviderId;
  const providersRef = useRef(providers);
  providersRef.current = providers;

  const customProviderTypeOptions = useMemo(
    () =>
      providerCatalog
        .filter((entry) => entry.supportsCustom)
        .map((entry) => ({
          value: entry.providerType as CustomProviderType,
          label: entry.displayName,
          defaultBaseUrl: entry.defaultBaseUrl,
        })),
    [providerCatalog],
  );

  useEffect(() => {
    if (
      selectedProviderId &&
      !providers.some((provider) => provider.id === selectedProviderId)
    ) {
      onSelectProvider(providers[0]?.id ?? null);
    }
  }, [providers, selectedProviderId]);

  useEffect(() => {
    if (!selectedProvider) {
      return;
    }

    setFetchFeedback(null);
    setShowApiKey(false);
  }, [selectedProvider?.id]);

  useEffect(() => {
    if (!showCatalogPicker || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      if (catalogPickerRef.current?.contains(event.target as Node)) {
        return;
      }

      setShowCatalogPicker(false);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [showCatalogPicker]);

  // Flush pending API key save on unmount (don't discard it)
  useEffect(() => {
    return () => {
      if (apiKeySaveTimerRef.current) {
        clearTimeout(apiKeySaveTimerRef.current);
        apiKeySaveTimerRef.current = null;
      }

      // Flush: save the current draft immediately
      const provider = providersRef.current.find((p) => p.id === selectedProviderIdRef.current);

      if (provider) {
        const trimmed = apiKeyDraftRef.current.trim();

        if (trimmed && trimmed !== provider.apiKey) {
          onUpdateProvider(provider.id, { apiKey: trimmed, enabled: true });
        }
      }
    };
  }, []);

  // Track that we're waiting for a newly added provider to appear
  const pendingSelectRef = useRef(false);
  const prevProviderIdsRef = useRef(new Set(providers.map((p) => p.id)));

  // Auto-select a newly added provider once it appears in the providers array
  useEffect(() => {
    const currentIds = new Set(providers.map((p) => p.id));

    if (pendingSelectRef.current) {
      // Find the new id that wasn't in the previous set
      for (const id of currentIds) {
        if (!prevProviderIdsRef.current.has(id)) {
          onSelectProvider(id);
          pendingSelectRef.current = false;
          break;
        }
      }
    }

    prevProviderIdsRef.current = currentIds;
  }, [providers]);

  const handleSelectCatalogEntry = (entry: ProviderCatalogEntry) => {
    const existing = providers.find(
      (provider) => provider.providerKey === entry.providerKey,
    );

    if (existing) {
      onSelectProvider(existing.id);
      setShowCatalogPicker(false);
      return;
    }

    const newProvider: Omit<ProviderEntry, "id"> = {
      kind: entry.builtin ? "builtin" : "custom",
      providerKey: entry.providerKey,
      providerType: entry.providerType,
      displayName: entry.displayName,
      baseUrl: entry.defaultBaseUrl,
      apiKey: "",
      hasApiKey: false,
      lockedMapping: false,
      customHeaders: {},
      enabled: false,
      models: [],
    };

    pendingSelectRef.current = true;
    onAddProvider(newProvider);
    setShowCatalogPicker(false);
  };

  const handleAddCustomProvider = () => {
    const providerKey = crypto.randomUUID();
    const newProvider: Omit<ProviderEntry, "id"> = {
      kind: "custom",
      providerKey,
      providerType: customProviderTypeOptions[0]?.value ?? "openai-compatible",
      displayName: t("onboarding.provider.addCustom"),
      baseUrl:
        customProviderTypeOptions[0]?.defaultBaseUrl ??
        "https://api.example.com/v1",
      apiKey: "",
      hasApiKey: false,
      lockedMapping: false,
      customHeaders: {},
      enabled: false,
      models: [],
    };

    pendingSelectRef.current = true;
    onAddProvider(newProvider);
    setShowCatalogPicker(false);
  };

  const handleApiKeyChange = (value: string) => {
    setApiKeyDraft(value);

    if (!selectedProvider) {
      return;
    }

    // Debounced auto-save
    if (apiKeySaveTimerRef.current) {
      clearTimeout(apiKeySaveTimerRef.current);
    }

    apiKeySaveTimerRef.current = setTimeout(() => {
      const trimmed = value.trim();

      if (!trimmed) {
        return;
      }

      onUpdateProvider(selectedProvider.id, {
        apiKey: trimmed,
        enabled: true,
      });
    }, 400);
  };

  const handleFetchModels = async () => {
    if (!selectedProvider) {
      return;
    }

    // Flush any pending debounced API key save
    if (apiKeySaveTimerRef.current) {
      clearTimeout(apiKeySaveTimerRef.current);
      apiKeySaveTimerRef.current = null;
    }

    const trimmedKey = apiKeyDraft.trim();

    if (trimmedKey && trimmedKey !== selectedProvider.apiKey) {
      onUpdateProvider(selectedProvider.id, {
        apiKey: trimmedKey,
        enabled: true,
      });
    }

    setIsFetchingModels(true);
    setFetchFeedback(null);

    try {
      await onFetchProviderModels(selectedProvider.id);
      setFetchFeedback({ kind: "success" });
    } catch (error) {
      setFetchFeedback({
        kind: "error",
        message: t("onboarding.provider.fetchError", {
          message:
            error instanceof Error ? error.message : "Unknown error",
        }),
      });
    } finally {
      setIsFetchingModels(false);
    }
  };

  const handleToggleModel = (modelId: string, enabled: boolean) => {
    if (!selectedProvider) {
      return;
    }

    onUpdateProvider(selectedProvider.id, {
      models: selectedProvider.models.map((model) =>
        model.id === modelId ? { ...model, enabled } : model,
      ),
    });
  };

  const enabledModelsCount = selectedProvider?.models.filter((model) => model.enabled).length ?? 0;
  const hasModels = (selectedProvider?.models.length ?? 0) > 0;

  const filteredModels = useMemo(() => {
    if (!selectedProvider) return [];
    const query = modelSearch.trim().toLowerCase();
    if (!query) return selectedProvider.models;
    return selectedProvider.models.filter(
      (model) =>
        model.modelId.toLowerCase().includes(query) ||
        (model.displayName && model.displayName.toLowerCase().includes(query)),
    );
  }, [selectedProvider, modelSearch]);

  return (
    <div className="flex min-h-[420px] flex-1 flex-col gap-5">
      {/* Provider picker */}
      <div className="shrink-0 space-y-3">
        <div className="flex items-center gap-2 text-sm font-medium text-app-foreground">
          <Server className="size-4" />
          <span>{t("onboarding.provider.selectProvider")}</span>
        </div>

        <div ref={catalogPickerRef} className="relative">
          <button
            type="button"
            className="flex w-full items-center justify-between gap-3 rounded-xl border border-app-border bg-app-surface/60 px-4 py-3 text-left transition-colors hover:border-app-border-strong hover:bg-app-surface"
            onClick={() => setShowCatalogPicker((v) => !v)}
          >
            {selectedProvider ? (
              <div className="flex items-center gap-3">
                <ProviderIcon
                  name={selectedProvider.displayName}
                  className="size-5"
                />
                <span className="text-sm font-medium text-app-foreground">
                  {selectedProvider.displayName}
                </span>
              </div>
            ) : (
              <span className="text-sm text-app-muted">
                {t("onboarding.provider.selectProvider")}
              </span>
            )}
            <ChevronDown
              className={cn(
                "size-4 text-app-subtle transition-transform",
                showCatalogPicker && "rotate-180",
              )}
            />
          </button>

          {showCatalogPicker ? (
            <div className="absolute inset-x-0 top-[calc(100%+4px)] z-20 overflow-hidden rounded-xl border border-app-border bg-app-menu/98 p-1.5 shadow-lg backdrop-blur-xl">
              <div className="max-h-[220px] overflow-y-auto [scrollbar-width:thin]">
                <div className="space-y-0.5">
                  {providerCatalog.filter((entry) => entry.builtin).sort((a, b) => a.displayName.localeCompare(b.displayName)).map((entry) => {
                    const existing = providers.find(
                      (provider) =>
                        provider.providerKey === entry.providerKey,
                    );
                    const isSelected = existing?.id === selectedProviderId;

                    return (
                      <button
                        key={entry.providerKey}
                        type="button"
                        className={cn(
                          "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors",
                          isSelected
                            ? "bg-app-foreground/6 text-app-foreground"
                            : "text-app-muted hover:bg-app-surface-hover/70 hover:text-app-foreground",
                        )}
                        onClick={() => handleSelectCatalogEntry(entry)}
                      >
                        <ProviderIcon
                          name={entry.displayName}
                          className="size-5 shrink-0"
                        />
                        <span className="min-w-0 flex-1 truncate text-sm font-medium">
                          {entry.displayName}
                        </span>
                        {isSelected ? (
                          <Check className="size-4 shrink-0 text-app-foreground" />
                        ) : null}
                      </button>
                    );
                  })}
                  <div className="mx-2 my-1.5 h-px bg-app-border" />
                  <button
                    type="button"
                    className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-app-muted transition-colors hover:bg-app-surface-hover/70 hover:text-app-foreground"
                    onClick={handleAddCustomProvider}
                  >
                    <Plus className="size-4 shrink-0" />
                    <span className="text-sm font-medium">
                      {t("onboarding.provider.addCustom")}
                    </span>
                  </button>
                </div>
              </div>
            </div>
          ) : null}
        </div>
      </div>

      {/* Provider config form */}
      {selectedProvider ? (
        <>
          {/* Fixed config fields */}
          <div className="shrink-0 space-y-4">
            {/* Custom provider extra fields */}
            {selectedProvider.kind === "custom" ? (
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1.5">
                  <label className="text-[12px] font-medium text-app-subtle">
                    {t("onboarding.provider.displayNameLabel")}
                  </label>
                  <Input
                    value={selectedProvider.displayName}
                    onChange={(event) =>
                      onUpdateProvider(selectedProvider.id, {
                        displayName: event.target.value,
                      })
                    }
                    className="h-9 rounded-lg text-[13px]"
                  />
                </div>
                <div className="space-y-1.5">
                  <label className="text-[12px] font-medium text-app-subtle">
                    {t("onboarding.provider.providerTypeLabel")}
                  </label>
                  <div className="relative">
                    <select
                      value={selectedProvider.providerType}
                      onChange={(event) => {
                        const nextType = event.target.value as CustomProviderType;
                        const matched = customProviderTypeOptions.find((o) => o.value === nextType);
                        onUpdateProvider(selectedProvider.id, {
                          providerType: nextType,
                          ...(matched ? { baseUrl: matched.defaultBaseUrl } : {}),
                        });
                      }}
                      className="h-9 w-full appearance-none rounded-lg border border-app-border bg-app-surface-muted px-3 pr-8 text-[13px] text-app-foreground outline-none transition-colors focus-visible:border-app-border-strong"
                    >
                      {customProviderTypeOptions.map((option) => (
                        <option key={option.value} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                    <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-app-subtle" />
                  </div>
                </div>
              </div>
            ) : null}

            {/* Base URL */}
            <div className="space-y-1.5">
              <label className="text-[12px] font-medium text-app-subtle">
                {t("onboarding.provider.baseUrlLabel")}
              </label>
              <Input
                value={selectedProvider.baseUrl}
                onChange={(event) =>
                  onUpdateProvider(selectedProvider.id, {
                    baseUrl: event.target.value,
                  })
                }
                className="h-9 rounded-lg text-[13px]"
                placeholder="https://api.example.com/v1"
              />
            </div>

            {/* API Key + Load Models */}
            <div className="space-y-1.5">
              <label className="text-[12px] font-medium text-app-subtle">
                {t("onboarding.provider.apiKeyLabel")}
              </label>
              <div className="flex items-center gap-2">
                <div className="relative min-w-0 flex-1">
                  <Input
                    type={showApiKey ? "text" : "password"}
                    value={apiKeyDraft}
                    onChange={(event) => handleApiKeyChange(event.target.value)}
                    placeholder={t("onboarding.provider.apiKeyPlaceholder")}
                    className="h-9 rounded-lg pr-9 text-[13px]"
                  />
                  <button
                    type="button"
                    className="absolute right-2.5 top-1/2 -translate-y-1/2 text-app-subtle transition-colors hover:text-app-foreground"
                    onClick={() => setShowApiKey((v) => !v)}
                  >
                    {showApiKey ? (
                      <EyeOff className="size-3.5" />
                    ) : (
                      <Eye className="size-3.5" />
                    )}
                  </button>
                </div>
                <Button
                  size="sm"
                  className="relative h-9 shrink-0 rounded-lg px-4 text-[12px]"
                  onClick={() => void handleFetchModels()}
                  disabled={isFetchingModels || !apiKeyDraft.trim()}
                >
                  {/* Invisible text to hold max width */}
                  <span className="invisible contents">
                    {t("onboarding.provider.fetchModels")}
                  </span>
                  <span className="invisible contents">
                    {t("onboarding.provider.fetchingModels")}
                  </span>
                  {/* Visible text centered */}
                  <span className="absolute inset-0 flex items-center justify-center">
                    {isFetchingModels
                      ? t("onboarding.provider.fetchingModels")
                      : t("onboarding.provider.fetchModels")}
                  </span>
                </Button>
              </div>
              {fetchFeedback ? (
                <span
                  className={cn(
                    "text-[12px]",
                    fetchFeedback.kind === "success"
                      ? "text-app-success"
                      : "text-app-danger",
                  )}
                >
                  {fetchFeedback.kind === "success"
                    ? t("onboarding.provider.fetchSuccess", {
                        count: selectedProvider?.models.length ?? 0,
                      })
                    : fetchFeedback.message}
                </span>
              ) : null}
            </div>
          </div>

          {/* Model list — fills remaining space and scrolls internally */}
          {hasModels ? (
            <div className="flex min-h-0 flex-1 flex-col gap-1.5">
              <div className="flex shrink-0 items-center justify-between gap-3">
                <div className="text-[12px] font-medium text-app-subtle">
                  {t("onboarding.provider.modelsLabel")} ({enabledModelsCount}{" "}
                  / {selectedProvider.models.length})
                </div>
                <Input
                  value={modelSearch}
                  onChange={(event) => setModelSearch(event.target.value)}
                  placeholder={t("onboarding.provider.searchModels")}
                  className="h-7 w-40 rounded-lg text-[11px]"
                />
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border border-app-border bg-app-canvas/50 [scrollbar-width:thin]">
                <div className="divide-y divide-app-border/60">
                  {filteredModels.map((model) => (
                    <div
                      key={model.id}
                      className="flex items-center justify-between gap-3 px-4 py-2.5"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[13px] font-medium text-app-foreground">
                          {model.displayName || model.modelId}
                        </div>
                        {model.displayName && model.displayName !== model.modelId ? (
                          <div className="truncate text-[11px] text-app-subtle">
                            {model.modelId}
                          </div>
                        ) : null}
                      </div>
                      <Switch
                        checked={model.enabled}
                        onCheckedChange={(checked) =>
                          handleToggleModel(model.id, checked)
                        }
                      />
                    </div>
                  ))}
                </div>
              </div>
              {enabledModelsCount === 0 ? (
                <p className="shrink-0 text-[12px] text-app-warning">
                  {t("onboarding.provider.enableAtLeastOne")}
                </p>
              ) : null}
            </div>
          ) : (
            <div className="shrink-0 rounded-xl border border-dashed border-app-border bg-app-canvas/40 px-4 py-6 text-center text-[13px] text-app-muted">
              {t("onboarding.provider.noModels")}
            </div>
          )}
        </>
      ) : null}
    </div>
  );
}
