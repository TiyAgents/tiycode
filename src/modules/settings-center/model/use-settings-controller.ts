import { useEffect, useState } from "react";
import { persistSettings, readStoredSettings } from "@/modules/settings-center/model/settings-storage";
import type {
  AgentProfile,
  CommandSettings,
  CommandEntry,
  GeneralPreferences,
  PatternEntry,
  PolicySettings,
  ProviderEntry,
  SettingsState,
  WorkspaceEntry,
  WritableRootEntry,
} from "@/modules/settings-center/model/types";

export * from "@/modules/settings-center/model/types";

export function useSettingsController() {
  const [settings, setSettings] = useState<SettingsState>(() => readStoredSettings());

  useEffect(() => {
    persistSettings(settings);
  }, [settings]);

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
    setSettings((current) => ({
      ...current,
      providers: [...current.providers, { ...entry, id: crypto.randomUUID() }],
    }));
  };

  const removeProvider = (id: string) => {
    setSettings((current) => ({
      ...current,
      providers: current.providers.filter((provider) => provider.id !== id),
    }));
  };

  const updateProvider = (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => {
    setSettings((current) => ({
      ...current,
      providers: current.providers.map((provider) =>
        provider.id === id ? { ...provider, ...patch } : provider,
      ),
    }));
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
