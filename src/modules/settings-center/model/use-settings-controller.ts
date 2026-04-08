import { isTauri } from "@tauri-apps/api/core";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import {
  DEFAULT_AGENT_PROFILES,
  DEFAULT_GENERAL_PREFERENCES,
  GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY,
  GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY,
  DEFAULT_POLICY_SETTINGS,
  GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
  TERMINAL_SHELL_PATH_SETTING_KEY,
  TERMINAL_SHELL_ARGS_SETTING_KEY,
  TERMINAL_FONT_FAMILY_SETTING_KEY,
  TERMINAL_FONT_SIZE_SETTING_KEY,
  TERMINAL_LINE_HEIGHT_SETTING_KEY,
  TERMINAL_CURSOR_STYLE_SETTING_KEY,
  TERMINAL_CURSOR_BLINK_SETTING_KEY,
  TERMINAL_SCROLLBACK_SETTING_KEY,
  TERMINAL_COPY_ON_SELECT_SETTING_KEY,
  TERMINAL_TERM_ENV_SETTING_KEY,
} from "@/modules/settings-center/model/defaults";
import {
  persistLocalUiSettings,
  persistSettings,
  readStoredSettings,
} from "@/modules/settings-center/model/settings-storage";
import type {
  AgentProfile,
  CommandSettings,
  CommandEntry,
  GeneralPreferences,
  PatternEntry,
  PolicySettings,
  ProviderCatalogEntry,
  ProviderEntry,
  SettingsState,
  TerminalSettings,
  WorkspaceEntry,
  WritableRootEntry,
} from "@/modules/settings-center/model/types";
import type {
  WorkspaceDto,
  ProviderModelConnectionTestResultDto,
  ProviderSettingsDto,
} from "@/shared/types/api";
import {
  policyGetAll,
  policySet,
  profileCreate,
  profileDelete,
  profileList,
  profileUpdate,
  providerCatalogList,
  providerModelTestConnection,
  providerSettingsCreateCustom,
  providerSettingsDeleteCustom,
  providerSettingsFetchModels,
  providerSettingsGetAll,
  providerSettingsUpdateCustom,
  providerSettingsUpsertBuiltin,
  settingsGet,
  settingsGetAll,
  settingsSet,
  workspaceAdd,
  workspaceList,
  workspaceRemove,
  workspaceSetDefault,
} from "@/services/bridge";

export * from "@/modules/settings-center/model/types";

const ACTIVE_AGENT_PROFILE_SETTING_KEY = "active_profile_id";
const DB_BACKED_SETTINGS_MIGRATION_KEY = "settings.db_backed_sources_v1";

function mapBooleanSettingValue(value: unknown, fallback: boolean) {
  return typeof value === "boolean" ? value : fallback;
}

function mapGeneralPreferencesFromSettings(
  settings: ReadonlyArray<import("@/shared/types/api").SettingDto>,
  fallback: GeneralPreferences,
): GeneralPreferences {
  const values = new Map(settings.map((entry) => [entry.key, entry.value]));

  return {
    launchAtLogin: mapBooleanSettingValue(
      values.get(GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY),
      fallback.launchAtLogin,
    ),
    preventSleepWhileRunning: mapBooleanSettingValue(
      values.get(GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY),
      fallback.preventSleepWhileRunning,
    ),
    minimizeToTray: mapBooleanSettingValue(
      values.get(GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY),
      fallback.minimizeToTray,
    ),
  };
}

const TERMINAL_SETTING_KEY_MAP: Record<keyof TerminalSettings, string> = {
  shellPath: TERMINAL_SHELL_PATH_SETTING_KEY,
  shellArgs: TERMINAL_SHELL_ARGS_SETTING_KEY,
  fontFamily: TERMINAL_FONT_FAMILY_SETTING_KEY,
  fontSize: TERMINAL_FONT_SIZE_SETTING_KEY,
  lineHeight: TERMINAL_LINE_HEIGHT_SETTING_KEY,
  cursorStyle: TERMINAL_CURSOR_STYLE_SETTING_KEY,
  cursorBlink: TERMINAL_CURSOR_BLINK_SETTING_KEY,
  scrollback: TERMINAL_SCROLLBACK_SETTING_KEY,
  copyOnSelect: TERMINAL_COPY_ON_SELECT_SETTING_KEY,
  termEnv: TERMINAL_TERM_ENV_SETTING_KEY,
};

function mapTerminalSettingsFromDb(
  settings: ReadonlyArray<import("@/shared/types/api").SettingDto>,
  fallback: TerminalSettings,
): TerminalSettings {
  const values = new Map(settings.map((entry) => [entry.key, entry.value]));

  const str = (key: string, fb: string) => {
    const v = values.get(key);
    return typeof v === "string" ? v : fb;
  };
  const num = (key: string, fb: number) => {
    const v = values.get(key);
    return typeof v === "number" ? v : fb;
  };
  const bool = (key: string, fb: boolean) => {
    const v = values.get(key);
    return typeof v === "boolean" ? v : fb;
  };

  return {
    shellPath: str(TERMINAL_SHELL_PATH_SETTING_KEY, fallback.shellPath),
    shellArgs: str(TERMINAL_SHELL_ARGS_SETTING_KEY, fallback.shellArgs),
    fontFamily: str(TERMINAL_FONT_FAMILY_SETTING_KEY, fallback.fontFamily),
    fontSize: num(TERMINAL_FONT_SIZE_SETTING_KEY, fallback.fontSize),
    lineHeight: num(TERMINAL_LINE_HEIGHT_SETTING_KEY, fallback.lineHeight),
    cursorStyle: str(TERMINAL_CURSOR_STYLE_SETTING_KEY, fallback.cursorStyle) as TerminalSettings["cursorStyle"],
    cursorBlink: bool(TERMINAL_CURSOR_BLINK_SETTING_KEY, fallback.cursorBlink),
    scrollback: num(TERMINAL_SCROLLBACK_SETTING_KEY, fallback.scrollback),
    copyOnSelect: bool(TERMINAL_COPY_ON_SELECT_SETTING_KEY, fallback.copyOnSelect),
    termEnv: str(TERMINAL_TERM_ENV_SETTING_KEY, fallback.termEnv),
  };
}

function mapProfileDto(profile: import("@/shared/types/api").AgentProfileDto): AgentProfile {
  const defaultProfile = DEFAULT_AGENT_PROFILES[0];

  return {
    id: profile.id,
    name: profile.name,
    customInstructions: profile.customInstructions ?? defaultProfile.customInstructions,
    commitMessagePrompt: profile.commitMessagePrompt ?? defaultProfile.commitMessagePrompt,
    responseStyle: (profile.responseStyle as AgentProfile["responseStyle"] | null) ?? defaultProfile.responseStyle,
    thinkingLevel: (profile.thinkingLevel as AgentProfile["thinkingLevel"] | null) ?? defaultProfile.thinkingLevel,
    responseLanguage: profile.responseLanguage ?? defaultProfile.responseLanguage,
    commitMessageLanguage: profile.commitMessageLanguage ?? defaultProfile.commitMessageLanguage,
    primaryProviderId: profile.primaryProviderId ?? "",
    primaryModelId: profile.primaryModelId ?? "",
    assistantProviderId: profile.auxiliaryProviderId ?? "",
    assistantModelId: profile.auxiliaryModelId ?? "",
    liteProviderId: profile.lightweightProviderId ?? "",
    liteModelId: profile.lightweightModelId ?? "",
  };
}

function toProfileInput(profile: Omit<AgentProfile, "id">, isDefault?: boolean) {
  return {
    name: profile.name,
    customInstructions: profile.customInstructions,
    commitMessagePrompt: profile.commitMessagePrompt,
    responseStyle: profile.responseStyle,
    thinkingLevel: profile.thinkingLevel,
    responseLanguage: profile.responseLanguage,
    commitMessageLanguage: profile.commitMessageLanguage,
    primaryProviderId: profile.primaryProviderId || undefined,
    primaryModelId: profile.primaryModelId || undefined,
    auxiliaryProviderId: profile.assistantProviderId || undefined,
    auxiliaryModelId: profile.assistantModelId || undefined,
    lightweightProviderId: profile.liteProviderId || undefined,
    lightweightModelId: profile.liteModelId || undefined,
    ...(typeof isDefault === "boolean" ? { isDefault } : {}),
  };
}

function mapApprovalPolicyFromDb(value: unknown): PolicySettings["approvalPolicy"] {
  if (typeof value === "string") {
    if (value === "require_all") return "untrusted";
    if (value === "auto") return "never";
    return "on-request";
  }

  if (value && typeof value === "object" && "mode" in value) {
    return mapApprovalPolicyFromDb((value as { mode?: unknown }).mode);
  }

  return DEFAULT_POLICY_SETTINGS.approvalPolicy;
}

function mapApprovalPolicyToDb(value: PolicySettings["approvalPolicy"]) {
  const mode = value === "untrusted"
    ? "require_all"
    : value === "never"
      ? "auto"
      : "require_for_mutations";

  return { mode };
}

function parsePrefixedPolicyPattern(raw: string): { tool: string; pattern: string } | null {
  const trimmed = raw.trim();
  if (!trimmed) {
    return null;
  }

  const colonIndex = trimmed.indexOf(":");
  if (colonIndex < 0) {
    return null;
  }

  const prefix = trimmed.slice(0, colonIndex).trim().toLowerCase();
  const remainder = trimmed.slice(colonIndex + 1).trimStart();
  if (!remainder) {
    return null;
  }

  if (prefix === "shell") {
    return { tool: "shell", pattern: remainder };
  }

  if (prefix === "any") {
    return { tool: "*", pattern: remainder };
  }

  if (prefix === "tool") {
    const separatorIndex = remainder.search(/\s/);
    if (separatorIndex < 0) {
      return null;
    }

    const tool = remainder.slice(0, separatorIndex).trim().toLowerCase();
    const pattern = remainder.slice(separatorIndex).trim();
    if (!tool || !pattern) {
      return null;
    }

    return { tool, pattern };
  }

  return null;
}

function formatPolicyPatternForUi(tool: string, pattern: string) {
  const normalizedTool = tool.trim().toLowerCase();
  if (!normalizedTool) {
    return pattern;
  }
  if (normalizedTool === "*") {
    return `any:${pattern}`;
  }
  if (normalizedTool === "shell") {
    return `shell:${pattern}`;
  }
  return `tool:${normalizedTool} ${pattern}`;
}

function mapPatternEntriesFromDb(value: unknown): Array<PatternEntry> {
  if (!Array.isArray(value)) {
    return DEFAULT_POLICY_SETTINGS.allowList;
  }

  return value.flatMap((entry) => {
    if (!entry || typeof entry !== "object") {
      return [];
    }

    const record = entry as { id?: unknown; pattern?: unknown; tool?: unknown };
    if (typeof record.pattern !== "string") {
      return [];
    }

    return [{
      id: typeof record.id === "string" ? record.id : crypto.randomUUID(),
      pattern: formatPolicyPatternForUi(
        typeof record.tool === "string" ? record.tool : "*",
        record.pattern,
      ),
    }];
  });
}

function mapWritableRootsFromDb(value: unknown): Array<WritableRootEntry> {
  if (!Array.isArray(value)) {
    return DEFAULT_POLICY_SETTINGS.writableRoots;
  }

  return value.flatMap((entry) => {
    if (!entry || typeof entry !== "object") {
      return [];
    }

    const record = entry as { id?: unknown; path?: unknown };
    if (typeof record.path !== "string") {
      return [];
    }

    return [{
      id: typeof record.id === "string" ? record.id : crypto.randomUUID(),
      path: record.path,
    }];
  });
}

function mapPoliciesFromDtos(policyDtos: Array<import("@/shared/types/api").SettingDto>): PolicySettings {
  const policyByKey = new Map(policyDtos.map((entry) => [entry.key, entry.value]));

  return {
    approvalPolicy: mapApprovalPolicyFromDb(policyByKey.get("approval_policy")),
    allowList: mapPatternEntriesFromDb(policyByKey.get("allow_list")),
    denyList: mapPatternEntriesFromDb(policyByKey.get("deny_list")),
    writableRoots: mapWritableRootsFromDb(policyByKey.get("writable_roots")),
  };
}

async function persistPolicyState(policy: PolicySettings) {
  await Promise.all([
    policySet("approval_policy", JSON.stringify(mapApprovalPolicyToDb(policy.approvalPolicy))),
    policySet("allow_list", JSON.stringify(policy.allowList.map((entry) => {
      const parsed = parsePrefixedPolicyPattern(entry.pattern);
      return {
        id: entry.id,
        tool: parsed?.tool ?? "*",
        pattern: parsed?.pattern ?? entry.pattern,
      };
    }))),
    policySet("deny_list", JSON.stringify(policy.denyList.map((entry) => {
      const parsed = parsePrefixedPolicyPattern(entry.pattern);
      return {
        id: entry.id,
        tool: parsed?.tool ?? "*",
        pattern: parsed?.pattern ?? entry.pattern,
      };
    }))),
    policySet("writable_roots", JSON.stringify(policy.writableRoots)),
  ]);
}

function isDefaultPolicyState(policy: PolicySettings) {
  return (
    policy.approvalPolicy === DEFAULT_POLICY_SETTINGS.approvalPolicy
    && policy.allowList.length === 0
    && policy.denyList.length === 0
    && policy.writableRoots.length === 0
  );
}

function resolveActiveProfileId(
  profiles: ReadonlyArray<AgentProfile>,
  activeProfileId: unknown,
) {
  if (typeof activeProfileId === "string" && profiles.some((profile) => profile.id === activeProfileId)) {
    return activeProfileId;
  }

  return profiles[0]?.id ?? DEFAULT_AGENT_PROFILES[0]?.id ?? "default-profile";
}

function mapProviderDto(provider: ProviderSettingsDto): ProviderEntry {
  return {
    id: provider.id,
    kind: provider.kind,
    providerKey: provider.providerKey,
    providerType: provider.providerType as ProviderEntry["providerType"],
    displayName: provider.displayName,
    baseUrl: provider.baseUrl,
    apiKey: "",
    hasApiKey: provider.hasApiKey,
    lockedMapping: provider.lockedMapping,
    customHeaders: provider.customHeaders ?? {},
    enabled: provider.enabled,
    models: provider.models.map((model) => ({
      id: model.id,
      modelId: model.modelId,
      sortIndex: model.sortIndex,
      displayName: model.displayName ?? model.modelId,
      enabled: model.enabled,
      contextWindow: model.contextWindow ?? undefined,
      maxOutputTokens: model.maxOutputTokens ?? undefined,
      capabilityOverrides: model.capabilityOverrides ?? {},
      providerOptions: model.providerOptions ?? {},
      isManual: model.isManual,
    })),
  };
}

function mapWorkspaceDto(workspace: WorkspaceDto): WorkspaceEntry {
  return {
    id: workspace.id,
    name: workspace.name,
    path: workspace.path,
    isDefault: workspace.isDefault,
    isGit: workspace.isGit,
    autoWorkTree: workspace.autoWorkTree,
  };
}

export function useSettingsController() {
  const storedSettingsRef = useRef<SettingsState>(readStoredSettings());
  const [settings, setSettings] = useState<SettingsState>(() => storedSettingsRef.current);
  const [providerCatalog, setProviderCatalog] = useState<Array<ProviderCatalogEntry>>([]);
  const [availableShells, setAvailableShells] = useState<Array<{ path: string; name: string }>>([]);
  const [backendHydrated, setBackendHydrated] = useState(!isTauri());
  const [generalSettingsHydrated, setGeneralSettingsHydrated] = useState(!isTauri());
  const settingsRef = useRef(settings);

  settingsRef.current = settings;

  useEffect(() => {
    if (!backendHydrated) {
      return;
    }

    if (isTauri()) {
      persistLocalUiSettings(settings);
      return;
    }

    persistSettings(settings);
  }, [backendHydrated, settings]);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    let cancelled = false;

    async function hydrateDbBackedSettings() {
      try {
        const [providers, catalog, policies, profiles, workspaceEntries, storedSettings, activeProfileSetting, migrationSetting] =
          await Promise.all([
            providerSettingsGetAll(),
            providerCatalogList(),
            policyGetAll(),
            profileList(),
            workspaceList(),
            settingsGetAll(),
            settingsGet(ACTIVE_AGENT_PROFILE_SETTING_KEY),
            settingsGet(DB_BACKED_SETTINGS_MIGRATION_KEY),
          ]);

        const mappedProviders = providers.map(mapProviderDto);

        // Deduplicate providers by providerKey, keeping first occurrence
        const seenProviderKeys = new Set<string>();
        const dedupedProviders = mappedProviders.filter((provider) => {
          if (seenProviderKeys.has(provider.providerKey)) {
            return false;
          }
          seenProviderKeys.add(provider.providerKey);
          return true;
        });

        const mappedCatalog = catalog.map((entry) => ({
          providerKey: entry.providerKey as ProviderCatalogEntry["providerKey"],
          providerType: entry.providerType as ProviderCatalogEntry["providerType"],
          displayName: entry.displayName,
          builtin: entry.builtin,
          supportsCustom: entry.supportsCustom,
          defaultBaseUrl: entry.defaultBaseUrl,
        }));

        let mappedProfiles = profiles.map(mapProfileDto);
        const localProfiles = storedSettingsRef.current.agentProfiles.length > 0
          ? storedSettingsRef.current.agentProfiles
          : DEFAULT_AGENT_PROFILES;
        const localGeneral = storedSettingsRef.current.general;
        const migrated = migrationSetting?.value === true;

        if (!migrated && mappedProfiles.length === 0) {
          const activeLocalProfileId = storedSettingsRef.current.activeAgentProfileId;
          const profileIdMap = new Map<string, string>();
          const createdProfiles = [];

          for (const profile of localProfiles) {
            const created = await profileCreate(
              toProfileInput(
                {
                  name: profile.name,
                  customInstructions: profile.customInstructions,
                  commitMessagePrompt: profile.commitMessagePrompt,
                  responseStyle: profile.responseStyle,
                  thinkingLevel: profile.thinkingLevel,
                  responseLanguage: profile.responseLanguage,
                  commitMessageLanguage: profile.commitMessageLanguage,
                  primaryProviderId: profile.primaryProviderId,
                  primaryModelId: profile.primaryModelId,
                  assistantProviderId: profile.assistantProviderId,
                  assistantModelId: profile.assistantModelId,
                  liteProviderId: profile.liteProviderId,
                  liteModelId: profile.liteModelId,
                },
                profile.id === activeLocalProfileId,
              ),
            );

            profileIdMap.set(profile.id, created.id);
            createdProfiles.push(mapProfileDto(created));
          }

          mappedProfiles = createdProfiles;
          const migratedActiveProfileId = resolveActiveProfileId(
            createdProfiles,
            profileIdMap.get(activeLocalProfileId),
          );
          await settingsSet(
            ACTIVE_AGENT_PROFILE_SETTING_KEY,
            JSON.stringify(migratedActiveProfileId),
          );
        }

        const mappedPolicy = mapPoliciesFromDtos(policies);
        const shouldMigrateLocalPolicy =
          !migrated
          && isDefaultPolicyState(mappedPolicy)
          && !isDefaultPolicyState(storedSettingsRef.current.policy);

        if (shouldMigrateLocalPolicy) {
          await persistPolicyState(storedSettingsRef.current.policy);
        }

        const resolvedPolicy = shouldMigrateLocalPolicy
          ? storedSettingsRef.current.policy
          : mappedPolicy;

        const generalSettingKeys = new Set(storedSettings.map((entry) => entry.key));
        const generalMigrationWrites = !migrated
          ? [
              !generalSettingKeys.has(GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY)
              && localGeneral.launchAtLogin !== DEFAULT_GENERAL_PREFERENCES.launchAtLogin
                ? settingsSet(
                    GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY,
                    JSON.stringify(localGeneral.launchAtLogin),
                  )
                : null,
              !generalSettingKeys.has(GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY)
              && localGeneral.preventSleepWhileRunning !== DEFAULT_GENERAL_PREFERENCES.preventSleepWhileRunning
                ? settingsSet(
                    GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
                    JSON.stringify(localGeneral.preventSleepWhileRunning),
                  )
                : null,
              !generalSettingKeys.has(GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY)
              && localGeneral.minimizeToTray !== DEFAULT_GENERAL_PREFERENCES.minimizeToTray
                ? settingsSet(
                    GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY,
                    JSON.stringify(localGeneral.minimizeToTray),
                  )
                : null,
            ].filter((operation): operation is Promise<void> => operation !== null)
          : [];

        if (generalMigrationWrites.length > 0) {
          await Promise.all(generalMigrationWrites);
        }

        const resolvedGeneral = mapGeneralPreferencesFromSettings(storedSettings, localGeneral);
        const resolvedTerminal = mapTerminalSettingsFromDb(storedSettings, storedSettingsRef.current.terminal);

        // Fetch available shells
        let shells: Array<{ path: string; name: string }> = [];
        try {
          shells = await invoke<Array<{ path: string; name: string }>>("terminal_list_available_shells");
        } catch (shellError) {
          console.warn("Failed to list available shells", shellError);
        }

        const resolvedActiveProfileId = resolveActiveProfileId(
          mappedProfiles,
          activeProfileSetting?.value,
        );

        if (mappedProfiles.length > 0 && activeProfileSetting?.value !== resolvedActiveProfileId) {
          await settingsSet(
            ACTIVE_AGENT_PROFILE_SETTING_KEY,
            JSON.stringify(resolvedActiveProfileId),
          );
        }

        if (!migrated) {
          await settingsSet(DB_BACKED_SETTINGS_MIGRATION_KEY, "true");
        }

        if (cancelled) {
          return;
        }

        const nextGeneral = resolvedGeneral;

        setProviderCatalog(mappedCatalog);
        setAvailableShells(shells);
        setSettings((current) => ({
          ...current,
          general: nextGeneral,
          terminal: resolvedTerminal,
          workspaces: workspaceEntries.map(mapWorkspaceDto),
          providers: dedupedProviders,
          policy: resolvedPolicy,
          agentProfiles: mappedProfiles.length > 0 ? mappedProfiles : DEFAULT_AGENT_PROFILES,
          activeAgentProfileId: mappedProfiles.length > 0
            ? resolvedActiveProfileId
            : DEFAULT_AGENT_PROFILES[0]?.id ?? "default-profile",
        }));
        storedSettingsRef.current = {
          ...storedSettingsRef.current,
          general: nextGeneral,
        };
        setGeneralSettingsHydrated(true);
      } catch (error) {
        console.warn("Failed to hydrate DB-backed settings", error);
      } finally {
        if (!cancelled) {
          if (!generalSettingsHydrated) {
            setGeneralSettingsHydrated(true);
          }
          setBackendHydrated(true);
        }
      }
    }

    void hydrateDbBackedSettings();

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    const generalSettings = [
      {
        key: GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY,
        value: settings.general.launchAtLogin,
      },
      {
        key: GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
        value: settings.general.preventSleepWhileRunning,
      },
      {
        key: GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY,
        value: settings.general.minimizeToTray,
      },
    ];

    void Promise.all(
      generalSettings.map(({ key, value }) => settingsSet(key, JSON.stringify(value))),
    ).catch((error) => {
      console.warn("Failed to sync general settings", error);
    });
  }, [
    generalSettingsHydrated,
    settings.general.launchAtLogin,
    settings.general.preventSleepWhileRunning,
    settings.general.minimizeToTray,
  ]);

  const updateGeneralPreference = <Key extends keyof GeneralPreferences>(key: Key, value: GeneralPreferences[Key]) => {
    setSettings((current) => ({
      ...current,
      general: {
        ...current.general,
        [key]: value,
      },
    }));
  };

  const updateCommandSetting = <Key extends keyof CommandSettings>(key: Key, value: CommandSettings[Key]) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        [key]: value,
      },
    }));
  };

  const updateTerminalSetting = <Key extends keyof TerminalSettings>(key: Key, value: TerminalSettings[Key]) => {
    setSettings((current) => ({
      ...current,
      terminal: {
        ...current.terminal,
        [key]: value,
      },
    }));

    if (isTauri()) {
      const dbKey = TERMINAL_SETTING_KEY_MAP[key];
      void settingsSet(dbKey, JSON.stringify(value)).catch((error) => {
        console.warn(`Failed to persist terminal setting ${key}`, error);
      });
    }
  };

  const addAgentProfile = (entry: Omit<AgentProfile, "id">) => {
    if (!isTauri()) {
      const id = crypto.randomUUID();
      setSettings((current) => ({
        ...current,
        agentProfiles: [...current.agentProfiles, { ...entry, id }],
        activeAgentProfileId: id,
      }));
      return;
    }

    void profileCreate(toProfileInput(entry, false))
      .then(async (profile) => {
        const mapped = mapProfileDto(profile);
        await settingsSet(ACTIVE_AGENT_PROFILE_SETTING_KEY, JSON.stringify(mapped.id));
        setSettings((current) => ({
          ...current,
          agentProfiles: [...current.agentProfiles, mapped],
          activeAgentProfileId: mapped.id,
        }));
      })
      .catch((error) => {
        console.warn("Failed to create profile", error);
      });
  };

  const removeAgentProfile = (id: string) => {
    if (!isTauri()) {
      setSettings((current) => {
        const remaining = current.agentProfiles.filter((p) => p.id !== id);
        if (remaining.length === 0) return current;
        const activeId = current.activeAgentProfileId === id ? remaining[0].id : current.activeAgentProfileId;
        return { ...current, agentProfiles: remaining, activeAgentProfileId: activeId };
      });
      return;
    }

    const current = settingsRef.current;
    const remaining = current.agentProfiles.filter((profile) => profile.id !== id);
    if (remaining.length === 0) {
      return;
    }

    void profileDelete(id)
      .then(async () => {
        const nextActiveId = current.activeAgentProfileId === id
          ? remaining[0].id
          : current.activeAgentProfileId;
        await settingsSet(ACTIVE_AGENT_PROFILE_SETTING_KEY, JSON.stringify(nextActiveId));
        setSettings((latest) => ({
          ...latest,
          agentProfiles: latest.agentProfiles.filter((profile) => profile.id !== id),
          activeAgentProfileId: nextActiveId,
        }));
      })
      .catch((error) => {
        console.warn("Failed to delete profile", error);
      });
  };

  const updateAgentProfile = (id: string, patch: Partial<Omit<AgentProfile, "id">>) => {
    // Calculate the next settings state
    const currentSettings = settingsRef.current;
    const nextSettings: SettingsState = {
      ...currentSettings,
      agentProfiles: currentSettings.agentProfiles.map((p) =>
        p.id === id ? { ...p, ...patch } : p,
      ),
    };

    // Update React state immediately for UI responsiveness
    setSettings(nextSettings);

    if (!isTauri()) {
      // In non-Tauri environments, persist to localStorage immediately
      // This ensures changes survive component unmounting and page reloads
      persistSettings(nextSettings);
      return;
    }

    // In Tauri environments, sync to backend database
    const currentProfile = currentSettings.agentProfiles.find((profile) => profile.id === id);
    if (!currentProfile) {
      return;
    }

    const nextProfile = { ...currentProfile, ...patch };

    void profileUpdate(id, toProfileInput({
      name: nextProfile.name,
      customInstructions: nextProfile.customInstructions,
      commitMessagePrompt: nextProfile.commitMessagePrompt,
      responseStyle: nextProfile.responseStyle,
      thinkingLevel: nextProfile.thinkingLevel,
      responseLanguage: nextProfile.responseLanguage,
      commitMessageLanguage: nextProfile.commitMessageLanguage,
      primaryProviderId: nextProfile.primaryProviderId,
      primaryModelId: nextProfile.primaryModelId,
      assistantProviderId: nextProfile.assistantProviderId,
      assistantModelId: nextProfile.assistantModelId,
      liteProviderId: nextProfile.liteProviderId,
      liteModelId: nextProfile.liteModelId,
    }))
      .then((profile) => {
        const mapped = mapProfileDto(profile);
        setSettings((current) => ({
          ...current,
          agentProfiles: current.agentProfiles.map((entry) =>
            entry.id === id ? mapped : entry,
          ),
        }));
      })
      .catch((error) => {
        console.warn("Failed to update profile", error);
      });
  };

  const setActiveAgentProfile = (id: string) => {
    setSettings((current) => ({ ...current, activeAgentProfileId: id }));

    if (!isTauri()) {
      return;
    }

    void settingsSet(ACTIVE_AGENT_PROFILE_SETTING_KEY, JSON.stringify(id)).catch((error) => {
      console.warn("Failed to persist active profile", error);
    });
  };

  const duplicateAgentProfile = (id: string) => {
    if (!isTauri()) {
      setSettings((current) => {
        const source = current.agentProfiles.find((p) => p.id === id);
        if (!source) return current;
        const newId = crypto.randomUUID();
        const copy: AgentProfile = { ...source, id: newId, name: `${source.name} Copy` };
        return {
          ...current,
          agentProfiles: [...current.agentProfiles, copy],
          activeAgentProfileId: newId,
        };
      });
      return;
    }

    const source = settingsRef.current.agentProfiles.find((profile) => profile.id === id);
    if (!source) {
      return;
    }

    void profileCreate(toProfileInput({
      name: `${source.name} Copy`,
      customInstructions: source.customInstructions,
      commitMessagePrompt: source.commitMessagePrompt,
      responseStyle: source.responseStyle,
      thinkingLevel: source.thinkingLevel,
      responseLanguage: source.responseLanguage,
      commitMessageLanguage: source.commitMessageLanguage,
      primaryProviderId: source.primaryProviderId,
      primaryModelId: source.primaryModelId,
      assistantProviderId: source.assistantProviderId,
      assistantModelId: source.assistantModelId,
      liteProviderId: source.liteProviderId,
      liteModelId: source.liteModelId,
    }))
      .then(async (profile) => {
        const mapped = mapProfileDto(profile);
        await settingsSet(ACTIVE_AGENT_PROFILE_SETTING_KEY, JSON.stringify(mapped.id));
        setSettings((current) => ({
          ...current,
          agentProfiles: [...current.agentProfiles, mapped],
          activeAgentProfileId: mapped.id,
        }));
      })
      .catch((error) => {
        console.warn("Failed to duplicate profile", error);
      });
  };

  const updatePolicySetting = <Key extends keyof PolicySettings>(key: Key, value: PolicySettings[Key]) => {
    setSettings((current) => {
      const nextPolicy = {
        ...current.policy,
        [key]: value,
      };

      if (isTauri()) {
        void persistPolicyState(nextPolicy).catch((error) => {
          console.warn("Failed to update policy setting", error);
        });
      }

      return {
        ...current,
        policy: nextPolicy,
      };
    });
  };

  const addAllowEntry = (entry: Omit<PatternEntry, "id">) => {
    const nextEntry = { ...entry, id: crypto.randomUUID() };

    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: [...current.policy.allowList, nextEntry],
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        allowList: [...settingsRef.current.policy.allowList, nextEntry],
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to add allow list entry", error);
      });
    }
  };

  const removeAllowEntry = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: current.policy.allowList.filter((entry) => entry.id !== id),
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        allowList: settingsRef.current.policy.allowList.filter((entry) => entry.id !== id),
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to remove allow list entry", error);
      });
    }
  };

  const updateAllowEntry = (id: string, patch: Partial<Omit<PatternEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: current.policy.allowList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        allowList: settingsRef.current.policy.allowList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to update allow list entry", error);
      });
    }
  };

  const addDenyEntry = (entry: Omit<PatternEntry, "id">) => {
    const nextEntry = { ...entry, id: crypto.randomUUID() };

    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: [...current.policy.denyList, nextEntry],
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        denyList: [...settingsRef.current.policy.denyList, nextEntry],
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to add deny list entry", error);
      });
    }
  };

  const removeDenyEntry = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: current.policy.denyList.filter((entry) => entry.id !== id),
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        denyList: settingsRef.current.policy.denyList.filter((entry) => entry.id !== id),
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to remove deny list entry", error);
      });
    }
  };

  const updateDenyEntry = (id: string, patch: Partial<Omit<PatternEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: current.policy.denyList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        denyList: settingsRef.current.policy.denyList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to update deny list entry", error);
      });
    }
  };

  const addWritableRoot = (entry: Omit<WritableRootEntry, "id">) => {
    const nextEntry = { ...entry, id: crypto.randomUUID() };

    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: [...current.policy.writableRoots, nextEntry],
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        writableRoots: [...settingsRef.current.policy.writableRoots, nextEntry],
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to add writable root", error);
      });
    }
  };

  const removeWritableRoot = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: current.policy.writableRoots.filter((entry) => entry.id !== id),
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        writableRoots: settingsRef.current.policy.writableRoots.filter((entry) => entry.id !== id),
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to remove writable root", error);
      });
    }
  };

  const updateWritableRoot = (id: string, patch: Partial<Omit<WritableRootEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: current.policy.writableRoots.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      },
    }));

    if (isTauri()) {
      const nextPolicy = {
        ...settingsRef.current.policy,
        writableRoots: settingsRef.current.policy.writableRoots.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
      };
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to update writable root", error);
      });
    }
  };

  const addWorkspace = (entry: Omit<WorkspaceEntry, "id">) => {
    if (!isTauri()) {
      setSettings((current) => ({
        ...current,
        workspaces: [
          ...current.workspaces,
          { ...entry, id: crypto.randomUUID() },
        ],
      }));
      return;
    }

    void workspaceAdd(entry.path, entry.name)
      .then(async (workspace) => {
        if (entry.isDefault) {
          await workspaceSetDefault(workspace.id);
        }

        setSettings((current) => ({
          ...current,
          workspaces: current.workspaces
            .map((currentWorkspace) => ({
              ...currentWorkspace,
              isDefault: entry.isDefault ? false : currentWorkspace.isDefault,
            }))
            .concat({
              ...mapWorkspaceDto(workspace),
              isDefault: entry.isDefault,
            }),
        }));
      })
      .catch((error) => {
        console.warn("Failed to add workspace", error);
      });
  };

  const removeWorkspace = (id: string) => {
    if (!isTauri()) {
      setSettings((current) => ({
        ...current,
        workspaces: current.workspaces.filter((workspace) => workspace.id !== id),
      }));
      return;
    }

    void workspaceRemove(id)
      .then(() => {
        setSettings((current) => ({
          ...current,
          workspaces: current.workspaces.filter((workspace) => workspace.id !== id),
        }));
      })
      .catch((error) => {
        console.warn("Failed to remove workspace", error);
      });
  };

  const setDefaultWorkspace = (id: string) => {
    if (!isTauri()) {
      setSettings((current) => ({
        ...current,
        workspaces: current.workspaces.map((workspace) => ({
          ...workspace,
          isDefault: workspace.id === id,
        })),
      }));
      return;
    }

    void workspaceSetDefault(id)
      .then(() => {
        setSettings((current) => ({
          ...current,
          workspaces: current.workspaces.map((workspace) => ({
            ...workspace,
            isDefault: workspace.id === id,
          })),
        }));
      })
      .catch((error) => {
        console.warn("Failed to set default workspace", error);
      });
  };

  const addProvider = (entry: Omit<ProviderEntry, "id">) => {
    if (!isTauri()) {
      setSettings((current) => ({
        ...current,
        providers: [...current.providers, { ...entry, id: crypto.randomUUID() }],
      }));
      return;
    }

    void providerSettingsCreateCustom({
      displayName: entry.displayName,
      providerType: entry.providerType,
      baseUrl: entry.baseUrl,
      apiKey: entry.apiKey || undefined,
      enabled: entry.enabled,
      customHeaders: entry.customHeaders,
      models: entry.models.map((model) => ({
        id: model.id,
        modelId: model.modelId,
        displayName: model.displayName,
        enabled: model.enabled,
        contextWindow: model.contextWindow,
        maxOutputTokens: model.maxOutputTokens,
        capabilityOverrides: model.capabilityOverrides,
        providerOptions: model.providerOptions,
        isManual: model.isManual,
      })),
    })
      .then((provider) => {
        setSettings((current) => ({
          ...current,
          providers: [...current.providers, mapProviderDto(provider)],
        }));
      })
      .catch((error) => {
        console.warn("Failed to create provider", error);
      });
  };

  const removeProvider = (id: string) => {
    const target = settings.providers.find((provider) => provider.id === id);
    if (!target) {
      return;
    }

    if (!isTauri()) {
      setSettings((current) => ({
        ...current,
        providers: current.providers.filter((provider) => provider.id !== id),
      }));
      return;
    }

    if (target.kind !== "custom") {
      return;
    }

    void providerSettingsDeleteCustom(id)
      .then(() => {
        setSettings((current) => ({
          ...current,
          providers: current.providers.filter((provider) => provider.id !== id),
        }));
      })
      .catch((error) => {
        console.warn("Failed to delete provider", error);
      });
  };

  const updateProvider = (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => {
    const currentProvider = settings.providers.find((provider) => provider.id === id);
    if (!currentProvider) {
      return;
    }

    const nextProvider = { ...currentProvider, ...patch };

    setSettings((current) => ({
      ...current,
      providers: current.providers.map((provider) =>
        provider.id === id ? nextProvider : provider,
      ),
    }));

    if (!isTauri()) {
      return;
    }

    const input = {
      ...(Object.prototype.hasOwnProperty.call(patch, "displayName") ? { displayName: nextProvider.displayName } : {}),
      ...(Object.prototype.hasOwnProperty.call(patch, "providerType") ? { providerType: nextProvider.providerType } : {}),
      ...(Object.prototype.hasOwnProperty.call(patch, "baseUrl") ? { baseUrl: nextProvider.baseUrl } : {}),
      ...(Object.prototype.hasOwnProperty.call(patch, "apiKey") ? { apiKey: nextProvider.apiKey } : {}),
      ...(Object.prototype.hasOwnProperty.call(patch, "enabled") ? { enabled: nextProvider.enabled } : {}),
      ...(Object.prototype.hasOwnProperty.call(patch, "customHeaders") ? { customHeaders: nextProvider.customHeaders } : {}),
      ...(Object.prototype.hasOwnProperty.call(patch, "models")
        ? {
            models: nextProvider.models.map((model) => ({
              id: model.id,
              modelId: model.modelId,
              displayName: model.displayName,
              enabled: model.enabled,
              contextWindow: model.contextWindow,
              maxOutputTokens: model.maxOutputTokens,
              capabilityOverrides: model.capabilityOverrides,
              providerOptions: model.providerOptions,
              isManual: model.isManual,
            })),
          }
        : {}),
    };

    const request = currentProvider.kind === "builtin"
      ? providerSettingsUpsertBuiltin(currentProvider.providerKey, input)
      : providerSettingsUpdateCustom(id, input);

    void request
      .then((provider) => {
        setSettings((current) => ({
          ...current,
          providers: current.providers.map((entry) =>
            entry.id === id ? mapProviderDto(provider) : entry,
          ),
        }));
      })
      .catch((error) => {
        console.warn("Failed to update provider", error);
      });
  };

  const fetchProviderModels = async (id: string) => {
    if (!isTauri()) {
      return;
    }

    try {
      const provider = await providerSettingsFetchModels(id);
      setSettings((current) => ({
        ...current,
        providers: current.providers.map((entry) =>
          entry.id === id ? mapProviderDto(provider) : entry,
        ),
      }));
    } catch (error) {
      console.warn("Failed to fetch provider models", error);
      throw error;
    }
  };

  const testProviderModelConnection = async (
    providerId: string,
    modelId: string,
  ): Promise<ProviderModelConnectionTestResultDto> => {
    if (!isTauri()) {
      return {
        success: false,
        unsupported: false,
        message: "Test Connection requires Tauri runtime.",
        detail: null,
      };
    }

    return providerModelTestConnection(providerId, modelId);
  };

  const addCommand = (entry: Omit<CommandEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        commands: [...current.commands.commands, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeCommand = (id: string) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        commands: current.commands.commands.filter((cmd) => cmd.id !== id),
      },
    }));
  };

  const updateCommand = (id: string, patch: Partial<Omit<CommandEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      commands: {
        ...current.commands,
        commands: current.commands.commands.map((cmd) =>
          cmd.id === id ? { ...cmd, ...patch } : cmd,
        ),
      },
    }));
  };

  return {
    general: settings.general,
    workspaces: settings.workspaces,
    providerCatalog,
    providers: settings.providers,
    commands: settings.commands,
    terminal: settings.terminal,
    availableShells,
    policy: settings.policy,
    updateGeneralPreference,
    addWorkspace,
    removeWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    fetchProviderModels,
    testProviderModelConnection,
    updateCommandSetting,
    updateTerminalSetting,
    agentProfiles: settings.agentProfiles,
    activeAgentProfileId: settings.activeAgentProfileId,
    addAgentProfile,
    removeAgentProfile,
    updateAgentProfile,
    setActiveAgentProfile,
    duplicateAgentProfile,
    updatePolicySetting,
    addAllowEntry,
    removeAllowEntry,
    updateAllowEntry,
    addDenyEntry,
    removeDenyEntry,
    updateDenyEntry,
    addWritableRoot,
    removeWritableRoot,
    updateWritableRoot,
    addCommand,
    removeCommand,
    updateCommand,
  };
}
