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
  includeRawContent: false,
};

export type PersistedWebSearchSettings = {
  enabled?: unknown;
  engine?: unknown;
  apiKey?: unknown;
  baseUrl?: unknown;
  maxResults?: unknown;
  includeRawContent?: unknown;
};

export type WebSearchSettingsPatch = Partial<
  Omit<WebSearchSettings, "hasApiKey">
> & {
  apiKey?: string;
  clearApiKey?: boolean;
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

export function mapPersistedWebSearchSettings(
  value: unknown,
): WebSearchSettings {
  if (!value || typeof value !== "object") {
    return DEFAULT_WEB_SEARCH_SETTINGS;
  }

  const raw = value as PersistedWebSearchSettings;
  return {
    enabled: typeof raw.enabled === "boolean" ? raw.enabled : DEFAULT_WEB_SEARCH_SETTINGS.enabled,
    engine: isWebSearchEngine(raw.engine) ? raw.engine : DEFAULT_WEB_SEARCH_SETTINGS.engine,
    hasApiKey: typeof raw.apiKey === "string" && raw.apiKey.trim().length > 0,
    baseUrl: typeof raw.baseUrl === "string" && raw.baseUrl.trim().length > 0
      ? raw.baseUrl.trim()
      : undefined,
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
  const nextBaseUrl = patch.baseUrl ?? current.baseUrl;
  const nextApiKey = patch.clearApiKey
    ? undefined
    : typeof patch.apiKey === "string" && patch.apiKey.trim().length > 0
      ? patch.apiKey.trim()
      : typeof existingRecord.apiKey === "string"
        ? existingRecord.apiKey
        : undefined;

  return {
    enabled: patch.enabled ?? current.enabled,
    engine: nextEngine,
    ...(nextApiKey ? { apiKey: nextApiKey } : {}),
    ...(nextBaseUrl && nextBaseUrl.trim().length > 0 ? { baseUrl: nextBaseUrl.trim() } : {}),
    maxResults: normalizeMaxResults(patch.maxResults ?? current.maxResults),
    includeRawContent: patch.includeRawContent ?? current.includeRawContent,
  };
}
