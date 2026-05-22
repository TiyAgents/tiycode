import type { WebSearchEngine, WebSearchSettings } from "@/modules/settings-center/model/types";

export const WEB_SEARCH_SETTINGS_KEY = "web_search.settings";

export const WEB_SEARCH_ENGINES: WebSearchEngine[] = [
  "tavily",
  "brave",
  "exa",
  "firecrawl",
];

export const DEFAULT_WEB_SEARCH_SETTINGS: WebSearchSettings = {
  enabled: false,
  engine: "tavily",
  hasApiKey: false,
  maxResults: 5,
  includeRawContent: true,
};

export type PersistedWebSearchSettings = {
  enabled?: unknown;
  engine?: unknown;
  apiKeys?: unknown;
  apiKey?: unknown;
  baseUrls?: unknown;
  baseUrl?: unknown;
  maxResults?: unknown;
  includeRawContent?: unknown;
};

export type WebSearchSettingsPatch = Partial<
  Omit<WebSearchSettings, "hasApiKey">
> & {
  apiKey?: string;
};

export function isWebSearchEngine(value: unknown): value is WebSearchEngine {
  return typeof value === "string" && WEB_SEARCH_ENGINES.includes(value as WebSearchEngine);
}

function normalizeMaxResults(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return DEFAULT_WEB_SEARCH_SETTINGS.maxResults;
  }
  return Math.min(20, Math.max(1, Math.round(value)));
}

function normalizeStringMap(value: unknown): Partial<Record<WebSearchEngine, string>> {
  if (!value || typeof value !== "object") {
    return {};
  }
  const record = value as Record<string, unknown>;
  return WEB_SEARCH_ENGINES.reduce<Partial<Record<WebSearchEngine, string>>>((acc, engine) => {
    const entry = record[engine];
    if (typeof entry === "string" && entry.trim().length > 0) {
      acc[engine] = entry.trim();
    }
    return acc;
  }, {});
}

function normalizeApiKeys(value: unknown): Partial<Record<WebSearchEngine, string>> {
  return normalizeStringMap(value);
}

function normalizeBaseUrls(value: unknown): Partial<Record<WebSearchEngine, string>> {
  return normalizeStringMap(value);
}

function hasApiKeyForEngine(raw: PersistedWebSearchSettings, engine: WebSearchEngine): boolean {
  const apiKeys = normalizeApiKeys(raw.apiKeys);
  if (apiKeys[engine]) {
    return true;
  }
  return (
    (isWebSearchEngine(raw.engine) ? raw.engine : DEFAULT_WEB_SEARCH_SETTINGS.engine) === engine
    && typeof raw.apiKey === "string"
    && raw.apiKey.trim().length > 0
  );
}

function baseUrlForEngine(raw: PersistedWebSearchSettings, engine: WebSearchEngine): string | undefined {
  const baseUrls = normalizeBaseUrls(raw.baseUrls);
  if (baseUrls[engine]) {
    return baseUrls[engine];
  }
  return (isWebSearchEngine(raw.engine) ? raw.engine : DEFAULT_WEB_SEARCH_SETTINGS.engine) === engine
    && typeof raw.baseUrl === "string"
    && raw.baseUrl.trim().length > 0
    ? raw.baseUrl.trim()
    : undefined;
}

export function mapPersistedWebSearchSettings(
  value: unknown,
): WebSearchSettings {
  if (!value || typeof value !== "object") {
    return DEFAULT_WEB_SEARCH_SETTINGS;
  }

  const raw = value as PersistedWebSearchSettings;
  const engine = isWebSearchEngine(raw.engine) ? raw.engine : DEFAULT_WEB_SEARCH_SETTINGS.engine;
  return {
    enabled: typeof raw.enabled === "boolean" ? raw.enabled : DEFAULT_WEB_SEARCH_SETTINGS.enabled,
    engine,
    hasApiKey: hasApiKeyForEngine(raw, engine),
    baseUrl: baseUrlForEngine(raw, engine),
    maxResults: normalizeMaxResults(raw.maxResults),
    includeRawContent:
      typeof raw.includeRawContent === "boolean"
        ? raw.includeRawContent
        : DEFAULT_WEB_SEARCH_SETTINGS.includeRawContent,
  };
}

export function buildPersistedWebSearchSettings(
  current: WebSearchSettings,
  existing: unknown,
  patch: WebSearchSettingsPatch,
): PersistedWebSearchSettings {
  const existingRecord = existing && typeof existing === "object"
    ? (existing as PersistedWebSearchSettings)
    : {};

  const nextEngine = patch.engine ?? current.engine;
  const nextBaseUrls = normalizeBaseUrls(existingRecord.baseUrls);
  if (typeof existingRecord.baseUrl === "string" && existingRecord.baseUrl.trim().length > 0) {
    const legacyBaseUrlEngine = isWebSearchEngine(existingRecord.engine)
      ? existingRecord.engine
      : current.engine;
    nextBaseUrls[legacyBaseUrlEngine] ??= existingRecord.baseUrl.trim();
  }
  if (Object.prototype.hasOwnProperty.call(patch, "baseUrl")) {
    const nextBaseUrl = patch.baseUrl?.trim() ?? "";
    if (nextBaseUrl) {
      nextBaseUrls[nextEngine] = nextBaseUrl;
    } else {
      delete nextBaseUrls[nextEngine];
    }
  }
  const nextApiKeys = normalizeApiKeys(existingRecord.apiKeys);
  if (typeof existingRecord.apiKey === "string" && existingRecord.apiKey.trim().length > 0) {
    const legacyApiKeyEngine = isWebSearchEngine(existingRecord.engine)
      ? existingRecord.engine
      : current.engine;
    nextApiKeys[legacyApiKeyEngine] ??= existingRecord.apiKey.trim();
  }
  if (Object.prototype.hasOwnProperty.call(patch, "apiKey")) {
    const nextApiKey = patch.apiKey?.trim() ?? "";
    if (nextApiKey) {
      nextApiKeys[nextEngine] = nextApiKey;
    } else {
      delete nextApiKeys[nextEngine];
    }
  }

  return {
    enabled: patch.enabled ?? current.enabled,
    engine: nextEngine,
    ...(Object.keys(nextApiKeys).length > 0 ? { apiKeys: nextApiKeys } : {}),
    ...(Object.keys(nextBaseUrls).length > 0 ? { baseUrls: nextBaseUrls } : {}),
    maxResults: normalizeMaxResults(patch.maxResults ?? current.maxResults),
    includeRawContent: patch.includeRawContent ?? current.includeRawContent,
  };
}
