import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import {
  GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
} from "@/modules/settings-center/model/defaults";
import {
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
  WorkspaceEntry,
  WritableRootEntry,
} from "@/modules/settings-center/model/types";
import type { ProviderSettingsDto } from "@/shared/types/api";
import {
  providerCatalogList,
  providerSettingsCreateCustom,
  providerSettingsDeleteCustom,
  providerSettingsFetchModels,
  providerSettingsGetAll,
  providerSettingsUpdateCustom,
  providerSettingsUpsertBuiltin,
  settingsSet,
} from "@/services/bridge";

export * from "@/modules/settings-center/model/types";

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

export function useSettingsController() {
  const [settings, setSettings] = useState<SettingsState>(() => readStoredSettings());
  const [providerCatalog, setProviderCatalog] = useState<Array<ProviderCatalogEntry>>([]);

  useEffect(() => {
    persistSettings(settings);
  }, [settings]);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    void providerSettingsGetAll()
      .then((providers) => {
        setSettings((current) => ({
          ...current,
          providers: providers.map(mapProviderDto),
        }));
      })
      .catch((error) => {
        console.warn("Failed to load provider settings", error);
      });

    void providerCatalogList()
      .then((catalog) => {
        setProviderCatalog(catalog.map((entry) => ({
          providerKey: entry.providerKey as ProviderCatalogEntry["providerKey"],
          providerType: entry.providerType as ProviderCatalogEntry["providerType"],
          displayName: entry.displayName,
          builtin: entry.builtin,
          supportsCustom: entry.supportsCustom,
          defaultBaseUrl: entry.defaultBaseUrl,
        })));
      })
      .catch((error) => {
        console.warn("Failed to load provider catalog", error);
      });
  }, []);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    void settingsSet(
      GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
      JSON.stringify(settings.general.preventSleepWhileRunning),
    ).catch((error) => {
      console.warn("Failed to sync preventSleepWhileRunning setting", error);
    });
  }, [settings.general.preventSleepWhileRunning]);

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

  const addAgentProfile = (entry: Omit<AgentProfile, "id">) => {
    const id = crypto.randomUUID();
    setSettings((current) => ({
      ...current,
      agentProfiles: [...current.agentProfiles, { ...entry, id }],
      activeAgentProfileId: id,
    }));
  };

  const removeAgentProfile = (id: string) => {
    setSettings((current) => {
      const remaining = current.agentProfiles.filter((p) => p.id !== id);
      if (remaining.length === 0) return current;
      const activeId = current.activeAgentProfileId === id ? remaining[0].id : current.activeAgentProfileId;
      return { ...current, agentProfiles: remaining, activeAgentProfileId: activeId };
    });
  };

  const updateAgentProfile = (id: string, patch: Partial<Omit<AgentProfile, "id">>) => {
    setSettings((current) => ({
      ...current,
      agentProfiles: current.agentProfiles.map((p) =>
        p.id === id ? { ...p, ...patch } : p,
      ),
    }));
  };

  const setActiveAgentProfile = (id: string) => {
    setSettings((current) => ({ ...current, activeAgentProfileId: id }));
  };

  const duplicateAgentProfile = (id: string) => {
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
  };

  const updatePolicySetting = <Key extends keyof PolicySettings>(key: Key, value: PolicySettings[Key]) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        [key]: value,
      },
    }));
  };

  const addAllowEntry = (entry: Omit<PatternEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: [...current.policy.allowList, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeAllowEntry = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        allowList: current.policy.allowList.filter((entry) => entry.id !== id),
      },
    }));
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
  };

  const addDenyEntry = (entry: Omit<PatternEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: [...current.policy.denyList, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeDenyEntry = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        denyList: current.policy.denyList.filter((entry) => entry.id !== id),
      },
    }));
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
  };

  const addWritableRoot = (entry: Omit<WritableRootEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: [...current.policy.writableRoots, { ...entry, id: crypto.randomUUID() }],
      },
    }));
  };

  const removeWritableRoot = (id: string) => {
    setSettings((current) => ({
      ...current,
      policy: {
        ...current.policy,
        writableRoots: current.policy.writableRoots.filter((entry) => entry.id !== id),
      },
    }));
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
  };

  const addWorkspace = (entry: Omit<WorkspaceEntry, "id">) => {
    setSettings((current) => ({
      ...current,
      workspaces: [
        ...current.workspaces,
        { ...entry, id: crypto.randomUUID() },
      ],
    }));
  };

  const removeWorkspace = (id: string) => {
    setSettings((current) => ({
      ...current,
      workspaces: current.workspaces.filter((workspace) => workspace.id !== id),
    }));
  };

  const updateWorkspace = (id: string, patch: Partial<Omit<WorkspaceEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      workspaces: current.workspaces.map((workspace) =>
        workspace.id === id ? { ...workspace, ...patch } : workspace,
      ),
    }));
  };

  const setDefaultWorkspace = (id: string) => {
    setSettings((current) => ({
      ...current,
      workspaces: current.workspaces.map((workspace) => ({
        ...workspace,
        isDefault: workspace.id === id,
      })),
    }));
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
    policy: settings.policy,
    updateGeneralPreference,
    addWorkspace,
    removeWorkspace,
    updateWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    fetchProviderModels,
    updateCommandSetting,
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
