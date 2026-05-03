import { isTauri } from "@tauri-apps/api/core";
import {
  DEFAULT_AGENT_PROFILES,
} from "@/modules/settings-center/model/defaults";
import type {
  AgentProfile,
  CommandEntry,
  GeneralPreferences,
  PatternEntry,
  PolicySettings,
  ProviderEntry,
  TerminalSettings,
  WorkspaceEntry,
  WritableRootEntry,
} from "@/modules/settings-center/model/types";
import type {
  ProviderModelConnectionTestResultDto,
  ProviderSettingsDto,
  PromptCommandDto,
} from "@/shared/types/api";
import {
  policySet,
  profileCreate,
  profileDelete,
  profileUpdate,
  promptCommandCreate,
  promptCommandDelete,
  promptCommandUpdate,
  providerModelTestConnection,
  providerSettingsCreateCustom,
  providerSettingsDeleteCustom,
  providerSettingsFetchModels,
  providerSettingsUpdateCustom,
  providerSettingsUpsertBuiltin,
  settingsSet,
  workspaceAdd,
  workspaceRemove,
  workspaceSetDefault,
} from "@/services/bridge";
import { settingsStore } from "./settings-store";
import { syncToBackend, SyncError } from "@/shared/lib/ipc-sync";

const ACTIVE_AGENT_PROFILE_SETTING_KEY = "active_profile_id";

// ---------------------------------------------------------------------------
// DTO helpers (same as in settings-hydration.ts)
// ---------------------------------------------------------------------------

function mapProfileDto(
  profile: import("@/shared/types/api").AgentProfileDto,
): AgentProfile {
  const defaultProfile = DEFAULT_AGENT_PROFILES[0];
  return {
    id: profile.id,
    name: profile.name,
    customInstructions:
      profile.customInstructions ?? defaultProfile.customInstructions,
    commitMessagePrompt:
      profile.commitMessagePrompt ?? defaultProfile.commitMessagePrompt,
    responseStyle:
      (profile.responseStyle as AgentProfile["responseStyle"] | null) ??
      defaultProfile.responseStyle,
    thinkingLevel:
      (profile.thinkingLevel as AgentProfile["thinkingLevel"] | null) ??
      defaultProfile.thinkingLevel,
    responseLanguage:
      profile.responseLanguage ?? defaultProfile.responseLanguage,
    commitMessageLanguage:
      profile.commitMessageLanguage ?? defaultProfile.commitMessageLanguage,
    primaryProviderId: profile.primaryProviderId ?? "",
    primaryModelId: profile.primaryModelId ?? "",
    assistantProviderId: profile.auxiliaryProviderId ?? "",
    assistantModelId: profile.auxiliaryModelId ?? "",
    liteProviderId: profile.lightweightProviderId ?? "",
    liteModelId: profile.lightweightModelId ?? "",
  };
}

function toProfileInput(
  profile: Omit<AgentProfile, "id">,
  isDefault?: boolean,
) {
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

function mapWorkspaceDto(
  workspace: import("@/shared/types/api").WorkspaceDto,
): WorkspaceEntry {
  return {
    id: workspace.id,
    name: workspace.name,
    path: workspace.canonicalPath || workspace.path,
    isDefault: workspace.isDefault,
    isGit: workspace.isGit,
    autoWorkTree: workspace.autoWorkTree,
    kind: workspace.kind,
    parentWorkspaceId: workspace.parentWorkspaceId,
    worktreeHash: workspace.worktreeName
      ? workspace.worktreeName.slice(0, 6)
      : null,
  };
}

function mapPromptCommandDto(command: PromptCommandDto): CommandEntry {
  return {
    id: command.id,
    name: command.name,
    path: command.path,
    argumentHint: command.argumentHint,
    description: command.description,
    prompt: command.prompt,
    source: command.source,
    enabled: command.enabled,
    version: command.version,
    fileName: command.fileName,
  };
}

function mapApprovalPolicyToDb(value: PolicySettings["approvalPolicy"]) {
  const mode =
    value === "untrusted"
      ? "require_all"
      : value === "never"
        ? "auto"
        : "require_for_mutations";
  return { mode };
}

function parsePrefixedPolicyPattern(
  raw: string,
): { tool: string; pattern: string } | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;

  const colonIndex = trimmed.indexOf(":");
  if (colonIndex < 0) return null;

  const prefix = trimmed.slice(0, colonIndex).trim().toLowerCase();
  const remainder = trimmed.slice(colonIndex + 1).trimStart();
  if (!remainder) return null;

  if (prefix === "shell") return { tool: "shell", pattern: remainder };
  if (prefix === "any") return { tool: "*", pattern: remainder };
  if (prefix === "tool") {
    const separatorIndex = remainder.search(/\s/);
    if (separatorIndex < 0) return null;
    const tool = remainder.slice(0, separatorIndex).trim().toLowerCase();
    const pattern = remainder.slice(separatorIndex).trim();
    if (!tool || !pattern) return null;
    return { tool, pattern };
  }
  return null;
}

async function persistPolicyState(policy: PolicySettings) {
  await Promise.all([
    policySet(
      "approval_policy",
      JSON.stringify(mapApprovalPolicyToDb(policy.approvalPolicy)),
    ),
    policySet(
      "allow_list",
      JSON.stringify(
        policy.allowList.map((entry) => {
          const parsed = parsePrefixedPolicyPattern(entry.pattern);
          return {
            id: entry.id,
            tool: parsed?.tool ?? "*",
            pattern: parsed?.pattern ?? entry.pattern,
          };
        }),
      ),
    ),
    policySet(
      "deny_list",
      JSON.stringify(
        policy.denyList.map((entry) => {
          const parsed = parsePrefixedPolicyPattern(entry.pattern);
          return {
            id: entry.id,
            tool: parsed?.tool ?? "*",
            pattern: parsed?.pattern ?? entry.pattern,
          };
        }),
      ),
    ),
    policySet("writable_roots", JSON.stringify(policy.writableRoots)),
  ]);
}

/** Check whether a Tauri invoke error carries a `.not_found` error code. */
function isTauriNotFoundError(error: unknown): boolean {
  return (
    error !== null &&
    typeof error === "object" &&
    "errorCode" in error &&
    typeof (error as Record<string, unknown>).errorCode === "string" &&
    ((error as Record<string, unknown>).errorCode as string).endsWith(
      ".not_found",
    )
  );
}

/** Track IDs of pending entries whose backend create call is in-flight. */
const inflightCreateIds = new Set<string>();

// ---------------------------------------------------------------------------
// General / Terminal / Commands — purely local (no backend sync needed)
// ---------------------------------------------------------------------------

export function updateGeneralPreference<Key extends keyof GeneralPreferences>(
  key: Key,
  value: GeneralPreferences[Key],
) {
  settingsStore.setState((prev) => ({
    general: { ...prev.general, [key]: value },
  }));
}

export function updateTerminalSetting<Key extends keyof TerminalSettings>(
  key: Key,
  value: TerminalSettings[Key],
) {
  settingsStore.setState((prev) => ({
    terminal: { ...prev.terminal, [key]: value },
  }));
}

// ---------------------------------------------------------------------------
// Agent Profiles
// ---------------------------------------------------------------------------

export function addAgentProfile(entry: Omit<AgentProfile, "id">) {
  if (!isTauri()) {
    const id = crypto.randomUUID();
    settingsStore.setState((prev) => ({
      agentProfiles: [...prev.agentProfiles, { ...entry, id }],
      activeAgentProfileId: id,
    }));
    return;
  }

  void profileCreate(toProfileInput(entry, false))
    .then(async (profile) => {
      const mapped = mapProfileDto(profile);
      await settingsSet(
        ACTIVE_AGENT_PROFILE_SETTING_KEY,
        JSON.stringify(mapped.id),
      );
      settingsStore.setState((prev) => ({
        agentProfiles: [...prev.agentProfiles, mapped],
        activeAgentProfileId: mapped.id,
      }));
    })
    .catch((error) => {
      console.warn("Failed to create profile", error);
    });
}

export function removeAgentProfile(id: string) {
  if (!isTauri()) {
    settingsStore.setState((prev) => {
      const remaining = prev.agentProfiles.filter((p) => p.id !== id);
      if (remaining.length === 0) return {};
      const activeId =
        prev.activeAgentProfileId === id
          ? remaining[0].id
          : prev.activeAgentProfileId;
      return { agentProfiles: remaining, activeAgentProfileId: activeId };
    });
    return;
  }

  const current = settingsStore.getState();
  const remaining = current.agentProfiles.filter((profile) => profile.id !== id);
  if (remaining.length === 0) return;

  void profileDelete(id)
    .then(async () => {
      const nextActiveId =
        current.activeAgentProfileId === id
          ? remaining[0].id
          : current.activeAgentProfileId;
      await settingsSet(
        ACTIVE_AGENT_PROFILE_SETTING_KEY,
        JSON.stringify(nextActiveId),
      );
      settingsStore.setState((prev) => ({
        agentProfiles: prev.agentProfiles.filter(
          (profile) => profile.id !== id,
        ),
        activeAgentProfileId: nextActiveId,
      }));
    })
    .catch((error) => {
      console.warn("Failed to delete profile", error);
    });
}

export async function updateAgentProfile(
  id: string,
  patch: Partial<Omit<AgentProfile, "id">>,
) {
  const currentSettings = settingsStore.getState();
  const currentProfile = currentSettings.agentProfiles.find(
    (profile) => profile.id === id,
  );
  if (!currentProfile) return;

  if (!isTauri()) {
    settingsStore.setState((prev) => ({
      agentProfiles: prev.agentProfiles.map((p) =>
        p.id === id ? { ...p, ...patch } : p,
      ),
    }));
    return;
  }

  const nextProfile = { ...currentProfile, ...patch };

  try {
    const updated = await syncToBackend(
      settingsStore,
      () =>
        profileUpdate(
          id,
          toProfileInput({
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
          }),
        ),
      {
        optimistic: (s) => ({
          agentProfiles: s.agentProfiles.map((p) =>
            p.id === id ? { ...p, ...patch } : p,
          ),
        }),
        onSuccess: (_s, profile) => {
          const mapped = mapProfileDto(profile);
          return {
            agentProfiles: _s.agentProfiles.map((entry) =>
              entry.id === id ? mapped : entry,
            ),
          };
        },
        dedupe: { key: `profile:${id}`, strategy: "last" },
      },
    );
    return updated;
  } catch (error) {
    // Ghost-profile upgrade: if the profile does not exist in DB yet,
    // create it instead and replace the ghost entry.
    if (
      !(error instanceof SyncError) ||
      !isTauriNotFoundError(error.raw)
    ) {
      // For non-ghost errors, rollback already happened in syncToBackend
      return;
    }

    try {
      const createdProfile = await profileCreate(
        toProfileInput(nextProfile, false),
      );
      const mapped = mapProfileDto(createdProfile);

      await settingsSet(
        ACTIVE_AGENT_PROFILE_SETTING_KEY,
        JSON.stringify(mapped.id),
      );

      settingsStore.setState((prev) => {
        const found = prev.agentProfiles.some((entry) => entry.id === id);
        const nextProfiles = found
          ? prev.agentProfiles.map((entry) =>
              entry.id === id ? mapped : entry,
            )
          : [...prev.agentProfiles, mapped];

        return {
          agentProfiles: nextProfiles,
          activeAgentProfileId:
            prev.activeAgentProfileId === id
              ? mapped.id
              : prev.activeAgentProfileId,
        };
      });

      return mapped;
    } catch (createError) {
      console.warn(
        "Failed to create missing profile during update",
        createError,
      );
    }
  }
}

export function setActiveAgentProfile(id: string) {
  settingsStore.setState({ activeAgentProfileId: id });

  if (!isTauri()) return;

  void settingsSet(ACTIVE_AGENT_PROFILE_SETTING_KEY, JSON.stringify(id)).catch(
    (error) => {
      console.warn("Failed to persist active profile", error);
    },
  );
}

export function duplicateAgentProfile(id: string) {
  if (!isTauri()) {
    settingsStore.setState((prev) => {
      const source = prev.agentProfiles.find((p) => p.id === id);
      if (!source) return {};
      const newId = crypto.randomUUID();
      const copy: AgentProfile = {
        ...source,
        id: newId,
        name: `${source.name} Copy`,
      };
      return {
        agentProfiles: [...prev.agentProfiles, copy],
        activeAgentProfileId: newId,
      };
    });
    return;
  }

  const source = settingsStore
    .getState()
    .agentProfiles.find((profile) => profile.id === id);
  if (!source) return;

  void profileCreate(
    toProfileInput({
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
    }),
  )
    .then(async (profile) => {
      const mapped = mapProfileDto(profile);
      await settingsSet(
        ACTIVE_AGENT_PROFILE_SETTING_KEY,
        JSON.stringify(mapped.id),
      );
      settingsStore.setState((prev) => ({
        agentProfiles: [...prev.agentProfiles, mapped],
        activeAgentProfileId: mapped.id,
      }));
    })
    .catch((error) => {
      console.warn("Failed to duplicate profile", error);
    });
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

export function updatePolicySetting<Key extends keyof PolicySettings>(
  key: Key,
  value: PolicySettings[Key],
) {
  settingsStore.setState((prev) => {
    const nextPolicy = { ...prev.policy, [key]: value };

    if (isTauri()) {
      void persistPolicyState(nextPolicy).catch((error) => {
        console.warn("Failed to update policy setting", error);
      });
    }

    return { policy: nextPolicy };
  });
}

export function addAllowEntry(entry: Omit<PatternEntry, "id">) {
  const nextEntry = { ...entry, id: crypto.randomUUID() };

  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      allowList: [...prev.policy.allowList, nextEntry],
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      allowList: [...settingsStore.getState().policy.allowList, nextEntry],
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to add allow list entry", error);
    });
  }
}

export function removeAllowEntry(id: string) {
  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      allowList: prev.policy.allowList.filter((entry) => entry.id !== id),
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      allowList: settingsStore
        .getState()
        .policy.allowList.filter((entry) => entry.id !== id),
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to remove allow list entry", error);
    });
  }
}

export function updateAllowEntry(
  id: string,
  patch: Partial<Omit<PatternEntry, "id">>,
) {
  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      allowList: prev.policy.allowList.map((entry) =>
        entry.id === id ? { ...entry, ...patch } : entry,
      ),
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      allowList: settingsStore
        .getState()
        .policy.allowList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to update allow list entry", error);
    });
  }
}

export function addDenyEntry(entry: Omit<PatternEntry, "id">) {
  const nextEntry = { ...entry, id: crypto.randomUUID() };

  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      denyList: [...prev.policy.denyList, nextEntry],
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      denyList: [...settingsStore.getState().policy.denyList, nextEntry],
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to add deny list entry", error);
    });
  }
}

export function removeDenyEntry(id: string) {
  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      denyList: prev.policy.denyList.filter((entry) => entry.id !== id),
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      denyList: settingsStore
        .getState()
        .policy.denyList.filter((entry) => entry.id !== id),
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to remove deny list entry", error);
    });
  }
}

export function updateDenyEntry(
  id: string,
  patch: Partial<Omit<PatternEntry, "id">>,
) {
  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      denyList: prev.policy.denyList.map((entry) =>
        entry.id === id ? { ...entry, ...patch } : entry,
      ),
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      denyList: settingsStore
        .getState()
        .policy.denyList.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to update deny list entry", error);
    });
  }
}

export function addWritableRoot(entry: Omit<WritableRootEntry, "id">) {
  const nextEntry = { ...entry, id: crypto.randomUUID() };

  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      writableRoots: [...prev.policy.writableRoots, nextEntry],
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      writableRoots: [
        ...settingsStore.getState().policy.writableRoots,
        nextEntry,
      ],
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to add writable root", error);
    });
  }
}

export function removeWritableRoot(id: string) {
  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      writableRoots: prev.policy.writableRoots.filter(
        (entry) => entry.id !== id,
      ),
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      writableRoots: settingsStore
        .getState()
        .policy.writableRoots.filter((entry) => entry.id !== id),
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to remove writable root", error);
    });
  }
}

export function updateWritableRoot(
  id: string,
  patch: Partial<Omit<WritableRootEntry, "id">>,
) {
  settingsStore.setState((prev) => ({
    policy: {
      ...prev.policy,
      writableRoots: prev.policy.writableRoots.map((entry) =>
        entry.id === id ? { ...entry, ...patch } : entry,
      ),
    },
  }));

  if (isTauri()) {
    const nextPolicy = {
      ...settingsStore.getState().policy,
      writableRoots: settingsStore
        .getState()
        .policy.writableRoots.map((entry) =>
          entry.id === id ? { ...entry, ...patch } : entry,
        ),
    };
    void persistPolicyState(nextPolicy).catch((error) => {
      console.warn("Failed to update writable root", error);
    });
  }
}

// ---------------------------------------------------------------------------
// Workspaces
// ---------------------------------------------------------------------------

export function addWorkspace(entry: Omit<WorkspaceEntry, "id">) {
  if (!isTauri()) {
    settingsStore.setState((prev) => ({
      workspaces: [
        ...prev.workspaces,
        { ...entry, id: crypto.randomUUID() },
      ],
    }));
    return;
  }

  void syncToBackend(settingsStore, () => workspaceAdd(entry.path, entry.name), {
    onSuccess: (_s, workspace) => ({
      workspaces: [..._s.workspaces, mapWorkspaceDto(workspace)],
    }),
  }).catch((error) => {
    console.warn("Failed to add workspace", error);
  });
}

export function removeWorkspace(id: string) {
  if (!isTauri()) {
    settingsStore.setState((prev) => ({
      workspaces: prev.workspaces.filter((workspace) => workspace.id !== id),
    }));
    return;
  }

  void syncToBackend(settingsStore, () => workspaceRemove(id), {
    optimistic: (s) => ({
      workspaces: s.workspaces.filter((w) => w.id !== id),
    }),
  }).catch((error) => {
    console.warn("Failed to remove workspace", error);
  });
}

export function setDefaultWorkspace(id: string) {
  if (!isTauri()) {
    settingsStore.setState((prev) => ({
      workspaces: prev.workspaces.map((workspace) => ({
        ...workspace,
        isDefault: workspace.id === id,
      })),
    }));
    return;
  }

  void syncToBackend(settingsStore, () => workspaceSetDefault(id), {
    optimistic: (s) => ({
      workspaces: s.workspaces.map((w) => ({
        ...w,
        isDefault: w.id === id,
      })),
    }),
  }).catch((error) => {
    console.warn("Failed to set default workspace", error);
  });
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

export function addProvider(entry: Omit<ProviderEntry, "id">) {
  if (!isTauri()) {
    settingsStore.setState((prev) => ({
      providers: [...prev.providers, { ...entry, id: crypto.randomUUID() }],
    }));
    return;
  }

  void syncToBackend(
    settingsStore,
    () =>
      providerSettingsCreateCustom({
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
      }),
    {
      onSuccess: (_s, provider) => ({
        providers: [..._s.providers, mapProviderDto(provider)],
      }),
    },
  ).catch((error) => {
    console.warn("Failed to create provider", error);
  });
}

export function removeProvider(id: string) {
  const target = settingsStore
    .getState()
    .providers.find((provider) => provider.id === id);
  if (!target) return;

  if (!isTauri()) {
    settingsStore.setState((prev) => ({
      providers: prev.providers.filter((provider) => provider.id !== id),
    }));
    return;
  }

  if (target.kind !== "custom") return;

  void syncToBackend(settingsStore, () => providerSettingsDeleteCustom(id), {
    optimistic: (s) => ({
      providers: s.providers.filter((p) => p.id !== id),
    }),
  }).catch((error) => {
    console.warn("Failed to delete provider", error);
  });
}

export function updateProvider(
  id: string,
  patch: Partial<Omit<ProviderEntry, "id">>,
) {
  const currentProvider = settingsStore
    .getState()
    .providers.find((provider) => provider.id === id);
  if (!currentProvider) return;

  const input = {
    ...(Object.prototype.hasOwnProperty.call(patch, "displayName")
      ? { displayName: patch.displayName ?? currentProvider.displayName }
      : {}),
    ...(Object.prototype.hasOwnProperty.call(patch, "providerType")
      ? { providerType: patch.providerType ?? currentProvider.providerType }
      : {}),
    ...(Object.prototype.hasOwnProperty.call(patch, "baseUrl")
      ? { baseUrl: patch.baseUrl ?? currentProvider.baseUrl }
      : {}),
    ...(Object.prototype.hasOwnProperty.call(patch, "apiKey")
      ? { apiKey: patch.apiKey ?? currentProvider.apiKey }
      : {}),
    ...(Object.prototype.hasOwnProperty.call(patch, "enabled")
      ? { enabled: patch.enabled ?? currentProvider.enabled }
      : {}),
    ...(Object.prototype.hasOwnProperty.call(patch, "customHeaders")
      ? { customHeaders: patch.customHeaders ?? currentProvider.customHeaders }
      : {}),
    ...(Object.prototype.hasOwnProperty.call(patch, "models")
      ? {
          models: (patch.models ?? currentProvider.models).map((model) => ({
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

  if (!isTauri()) {
    settingsStore.setState((prev) => ({
      providers: prev.providers.map((p) =>
        p.id === id ? { ...p, ...patch } : p,
      ),
    }));
    return;
  }

  const ipcCall =
    currentProvider.kind === "builtin"
      ? () => providerSettingsUpsertBuiltin(currentProvider.providerKey, input)
      : () => providerSettingsUpdateCustom(id, input);

  void syncToBackend(settingsStore, ipcCall, {
    optimistic: (s) => ({
      providers: s.providers.map((p) =>
        p.id === id ? { ...p, ...patch } : p,
      ),
    }),
    onSuccess: (s, provider) => ({
      providers: s.providers.map((p) =>
        p.id === id ? mapProviderDto(provider) : p,
      ),
    }),
    dedupe: { key: `provider:${id}`, strategy: "last" },
  }).catch((error) => {
    console.warn("Failed to update provider", error);
  });
}

export async function fetchProviderModels(id: string) {
  if (!isTauri()) return;

  try {
    const provider = await providerSettingsFetchModels(id);
    settingsStore.setState((prev) => ({
      providers: prev.providers.map((entry) =>
        entry.id === id ? mapProviderDto(provider) : entry,
      ),
    }));
  } catch (error) {
    console.warn("Failed to fetch provider models", error);
    throw error;
  }
}

export async function testProviderModelConnection(
  providerId: string,
  modelId: string,
): Promise<ProviderModelConnectionTestResultDto> {
  if (!isTauri()) {
    return {
      success: false,
      unsupported: false,
      message: "Test Connection requires Tauri runtime.",
      detail: null,
    };
  }

  return providerModelTestConnection(providerId, modelId);
}

// ---------------------------------------------------------------------------
// Prompt Commands
// ---------------------------------------------------------------------------

export function addCommand(entry: Omit<CommandEntry, "id">) {
  const tempId = crypto.randomUUID();
  settingsStore.setState((prev) => ({
    commands: [
      ...prev.commands,
      { ...entry, id: tempId, pendingCreate: isTauri() ? true : undefined },
    ],
  }));
}

export function removeCommand(id: string) {
  const current = settingsStore.getState();
  const cmd = current.commands.find((c) => c.id === id);

  // Always remove from local state immediately.
  settingsStore.setState((prev) => ({
    commands: prev.commands.filter((c) => c.id !== id),
  }));

  // Pending entries were never persisted — skip backend delete.
  if (!isTauri() || cmd?.pendingCreate) return;

  void promptCommandDelete(id).catch((error) => {
    console.warn("Failed to delete prompt command", error);
  });
}

export function updateCommand(
  id: string,
  patch: Partial<Omit<CommandEntry, "id">>,
) {
  const current = settingsStore.getState();
  const currentCommand = current.commands.find(
    (command) => command.id === id,
  );
  if (!currentCommand) return;

  const nextCommand = { ...currentCommand, ...patch };
  const isPending = currentCommand.pendingCreate;
  const shouldKeepPending = isPending && !nextCommand.name.trim();
  const cleanCommand = shouldKeepPending
    ? { ...nextCommand, pendingCreate: true as const }
    : (({ pendingCreate: _, ...rest }: CommandEntry) => rest)({
        ...nextCommand,
      });

  settingsStore.setState((prev) => ({
    commands: prev.commands.map((cmd) =>
      cmd.id === id ? cleanCommand : cmd,
    ),
  }));

  if (!isTauri()) return;

  if (currentCommand.pendingCreate) {
    if (!nextCommand.name.trim()) return;

    if (inflightCreateIds.has(id)) return;

    inflightCreateIds.add(id);
    void promptCommandCreate({
      name: nextCommand.name,
      path: nextCommand.path,
      argumentHint: nextCommand.argumentHint,
      description: nextCommand.description,
      prompt: nextCommand.prompt,
      source: nextCommand.source ?? "user",
      enabled: nextCommand.enabled ?? true,
      version: nextCommand.version ?? 1,
    })
      .then((command) => {
        const mapped = mapPromptCommandDto(command);
        settingsStore.setState((prev) => ({
          commands: prev.commands.map((entry) =>
            entry.id === id ? mapped : entry,
          ),
        }));
      })
      .catch((error) => {
        console.warn("Failed to create prompt command", error);
      })
      .finally(() => {
        inflightCreateIds.delete(id);
      });
    return;
  }

  void promptCommandUpdate(id, {
    id,
    name: nextCommand.name,
    path: nextCommand.path,
    argumentHint: nextCommand.argumentHint,
    description: nextCommand.description,
    prompt: nextCommand.prompt,
    source: nextCommand.source ?? "user",
    enabled: nextCommand.enabled ?? true,
    version: nextCommand.version ?? 1,
  })
    .then((command) => {
      const mapped = mapPromptCommandDto(command);
      settingsStore.setState((prev) => ({
        commands: prev.commands.map((entry) =>
          entry.id === id ? mapped : entry,
        ),
      }));
    })
    .catch((error) => {
      console.warn("Failed to update prompt command", error);
    });
}
