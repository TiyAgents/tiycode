import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  DEFAULT_AGENT_PROFILES,
} from "@/modules/settings-center/model/defaults";
import {
  readStoredLocalUiSettings,
} from "@/modules/settings-center/model/settings-storage";
import type {
  ProviderCatalogEntry,
} from "@/modules/settings-center/model/types";
import {
  policyGetAll,
  profileCreate,
  profileList,
  promptCommandList,
  providerCatalogList,
  providerSettingsGetAll,
  settingsGet,
  settingsSet,
  workspaceList,
} from "@/services/bridge";
import { waitForBackendReady } from "@/shared/lib/backend-ready";
import { settingsStore } from "./settings-store";

import type { AgentProfile, PolicySettings } from "@/modules/settings-center/model/types";

const ACTIVE_AGENT_PROFILE_SETTING_KEY = "active_profile_id";

// ---------------------------------------------------------------------------
// Helpers (extracted from use-settings-controller)
// ---------------------------------------------------------------------------

/** Resolve the active agent-profile ID from the loaded profile list. */
function resolveActiveProfileId(
  profiles: ReadonlyArray<AgentProfile>,
  activeProfileId: unknown,
): string {
  if (
    typeof activeProfileId === "string" &&
    profiles.some((profile) => profile.id === activeProfileId)
  ) {
    return activeProfileId;
  }
  return profiles[0]?.id ?? DEFAULT_AGENT_PROFILES[0]?.id ?? "default-profile";
}

// Helper registry of DTO mappers needed during hydration.  These are the same
// mappers that currently live in `use-settings-controller`; they are duplicated
// here intentionally so the hydration module is self-contained and does not
// introduce a circular dependency on the IPC-actions module.
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

function mapProviderDto(
  provider: import("@/shared/types/api").ProviderSettingsDto,
) {
  return {
    id: provider.id,
    kind: provider.kind,
    providerKey: provider.providerKey,
    providerType: provider.providerType as import("@/modules/settings-center/model/types").ProviderEntry["providerType"],
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

function mapPoliciesFromDtos(
  policyDtos: Array<import("@/shared/types/api").SettingDto>,
) {
  const policyByKey = new Map(policyDtos.map((entry) => [entry.key, entry.value]));

  return {
    approvalPolicy: mapApprovalPolicyFromDb(
      policyByKey.get("approval_policy"),
    ),
    allowList: mapPatternEntriesFromDb(policyByKey.get("allow_list")),
    denyList: mapPatternEntriesFromDb(policyByKey.get("deny_list")),
    writableRoots: mapWritableRootsFromDb(policyByKey.get("writable_roots")),
  };
}

function mapApprovalPolicyFromDb(value: unknown): PolicySettings["approvalPolicy"] {
  if (typeof value === "string") {
    if (value === "require_all") return "untrusted";
    if (value === "auto") return "never";
    return "on-request";
  }

  if (value && typeof value === "object" && "mode" in value) {
    return mapApprovalPolicyFromDb(
      (value as { mode?: unknown }).mode,
    );
  }

  return "on-request";
}

function formatPolicyPatternForUi(tool: string, pattern: string) {
  const normalizedTool = tool.trim().toLowerCase();
  if (!normalizedTool) return pattern;
  if (normalizedTool === "*") return `any:${pattern}`;
  if (normalizedTool === "shell") return `shell:${pattern}`;
  return `tool:${normalizedTool} ${pattern}`;
}

function mapPatternEntriesFromDb(value: unknown) {
  if (!Array.isArray(value)) return [];

  return value.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const record = entry as { id?: unknown; pattern?: unknown; tool?: unknown };
    if (typeof record.pattern !== "string") return [];
    return [
      {
        id: typeof record.id === "string" ? record.id : crypto.randomUUID(),
        pattern: formatPolicyPatternForUi(
          typeof record.tool === "string" ? record.tool : "*",
          record.pattern,
        ),
      },
    ];
  });
}

function mapWritableRootsFromDb(value: unknown) {
  if (!Array.isArray(value)) return [];
  return value.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const record = entry as { id?: unknown; path?: unknown };
    if (typeof record.path !== "string") return [];
    return [
      {
        id: typeof record.id === "string" ? record.id : crypto.randomUUID(),
        path: record.path,
      },
    ];
  });
}

function mapPromptCommandDto(
  command: import("@/shared/types/api").PromptCommandDto,
) {
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

function mapWorkspaceDto(
  workspace: import("@/shared/types/api").WorkspaceDto,
) {
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

// ---------------------------------------------------------------------------
// Single-flight hydration
// ---------------------------------------------------------------------------

let hydratePromise: Promise<void> | null = null;

/**
 * Public entry point.  Guarantees a single in-flight hydration batch across
 * all callers.  Subsequent invocations while hydration is running or finished
 * return the same Promise (or an already-resolved one for `hydrated`).
 *
 * Callers outside of Tauri (browser-only / web view fallback) are immediately
 * resolved with the defaults already placed in the store.
 */
export function hydrateSettingsOnce(): Promise<void> {
  if (!isTauri()) {
    // In web-only mode the initial defaults are sufficient.
    return Promise.resolve();
  }

  // Single-flight guard: if a batch is already in-flight, return it.
  // This must be checked before hydrationPhase to prevent two concurrent
  // callers from both entering while the phase is still 'uninitialized'.
  if (hydratePromise) return hydratePromise;

  const phase = settingsStore.getState().hydrationPhase;
  if (
    phase === "hydrated" ||
    phase === "loading_phase1" ||
    phase === "phase1_ready" ||
    phase === "loading_phase2"
  ) {
    return hydratePromise ?? Promise.resolve();
  }

  hydratePromise = hydrateSettings().finally(() => {
    hydratePromise = null;
  });
  return hydratePromise;
}

// ---------------------------------------------------------------------------
// Internal hydration
// ---------------------------------------------------------------------------

async function hydrateSettings(): Promise<void> {
  // Wait for backend to signal readiness before firing IPC calls.
  await waitForBackendReady();

  const hydrateStart = performance.now();

  // Seed general / terminal from localStorage before any IPC so the UI
  // preferences are available as early as possible.
  const localUi = readStoredLocalUiSettings();
  if (localUi) {
    settingsStore.setState({
      general: localUi.general,
      terminal: localUi.terminal,
    });
  }

  // ── Phase 1: critical-path data needed for first render ──
  settingsStore.setState({ hydrationPhase: "loading_phase1" });

  // Captured for use by the deferred Phase-2 callback.
  let activeProfileSetting: { value: string } | null = null;

  try {
    const t0 = performance.now();
    console.log(
      `⏱ [settings-hydration] phase-1 (3 invokes) at ${t0.toFixed(1)}ms since page load`,
    );

    const [providers, workspaceEntries, profileSetting] =
      await Promise.all([
        providerSettingsGetAll(),
        workspaceList(),
        settingsGet(ACTIVE_AGENT_PROFILE_SETTING_KEY),
      ]);

    activeProfileSetting =
      profileSetting?.value != null ? { value: String(profileSetting.value) } : null;

    console.log(
      `⏱ [settings-hydration] phase-1 done: ${(performance.now() - t0).toFixed(1)}ms`,
    );

    const mappedProviders = providers.map(mapProviderDto);

    // Deduplicate providers by providerKey (keep first occurrence).
    const seenProviderKeys = new Set<string>();
    const dedupedProviders = mappedProviders.filter((provider) => {
      if (seenProviderKeys.has(provider.providerKey)) return false;
      seenProviderKeys.add(provider.providerKey);
      return true;
    });

    // Resolve active profile ID from the settings KV value.
    const phase1ActiveId = (() => {
      try {
        const raw = activeProfileSetting?.value;
        if (raw && typeof raw === "string") {
          const parsed: unknown = JSON.parse(raw);
          if (typeof parsed === "string" && parsed.length > 0) {
            return parsed;
          }
        }
      } catch {
        /* ignore parse errors */
      }
      return DEFAULT_AGENT_PROFILES[0]?.id ?? "default-profile";
    })();

    settingsStore.setState({
      workspaces: workspaceEntries.map(mapWorkspaceDto),
      providers: dedupedProviders,
      activeAgentProfileId: phase1ActiveId,
      hydrationPhase: "phase1_ready",
    });

    console.log(
      `⏱ [settings-hydration] phase-1 total: ${(performance.now() - hydrateStart).toFixed(1)}ms`,
    );
  } catch (error) {
    console.error("Settings phase 1 failed:", error);
    settingsStore.setState({ hydrationPhase: "error" });
    return;
  }

  // ── Phase 2: deferred data (catalog, policies, profiles, commands) ──
  const scheduleDeferred: (cb: () => void) => number =
    (
      window as unknown as {
        requestIdleCallback?: (cb: () => void) => number;
      }
    ).requestIdleCallback ?? ((cb: () => void) => window.setTimeout(cb, 50));

  scheduleDeferred(() => {
    void (async () => {
      try {
        const t2 = performance.now();
        console.log(
          `⏱ [settings-hydration] phase-2 (4 invokes) at ${t2.toFixed(1)}ms since page load`,
        );

        settingsStore.setState({ hydrationPhase: "loading_phase2" });

        const [catalog, policies, profiles, promptCommands] =
          await Promise.all([
            providerCatalogList(),
            policyGetAll(),
            profileList(),
            promptCommandList(),
          ]);

        console.log(
          `⏱ [settings-hydration] phase-2 invokes done: ${(performance.now() - t2).toFixed(1)}ms`,
        );

        const mappedCatalog = catalog.map((entry) => ({
          providerKey:
            entry.providerKey as ProviderCatalogEntry["providerKey"],
          providerType:
            entry.providerType as ProviderCatalogEntry["providerType"],
          displayName: entry.displayName,
          builtin: entry.builtin,
          supportsCustom: entry.supportsCustom,
          defaultBaseUrl: entry.defaultBaseUrl,
        }));

        const mappedProfiles = profiles.map(mapProfileDto);
        const mappedPolicy = mapPoliciesFromDtos(policies);

        // Available shells (non-critical, best-effort).
        let shells: Array<{ path: string; name: string }> = [];
        try {
          const t3 = performance.now();
          shells = await invoke<
            Array<{ path: string; name: string }>
          >("terminal_list_available_shells");
          console.log(
            `⏱ [settings-hydration] terminal_list_available_shells: ${(performance.now() - t3).toFixed(1)}ms`,
          );
        } catch (shellError) {
          console.warn("Failed to list available shells", shellError);
        }

        const resolvedActiveProfileId = resolveActiveProfileId(
          mappedProfiles,
          activeProfileSetting?.value,
        );

        // Persist the resolved active-profile ID if it differs from what was stored.
        // We read the active profile setting from the phase-1 response via closure.
        void (async () => {
          try {
            const setting = await settingsGet(
              ACTIVE_AGENT_PROFILE_SETTING_KEY,
            );
            if (
              setting?.value !== resolvedActiveProfileId &&
              mappedProfiles.length > 0
            ) {
              await settingsSet(
                ACTIVE_AGENT_PROFILE_SETTING_KEY,
                JSON.stringify(resolvedActiveProfileId),
              );
            }
          } catch {
            /* best-effort */
          }
        })();

        // Seed the default profile when the DB is empty (fresh install).
        let persistedProfiles = mappedProfiles;
        let persistedActiveId = resolvedActiveProfileId;

        if (mappedProfiles.length === 0) {
          try {
            const defaultInput = DEFAULT_AGENT_PROFILES[0];
            if (defaultInput) {
              const created = await profileCreate(
                toProfileInput(defaultInput, true),
              );
              const mapped = mapProfileDto(created);
              persistedProfiles = [mapped];
              persistedActiveId = mapped.id;
              await settingsSet(
                ACTIVE_AGENT_PROFILE_SETTING_KEY,
                JSON.stringify(mapped.id),
              );
            }
          } catch (seedError) {
            console.warn(
              "Failed to seed default profile during hydration",
              seedError,
            );
          }
        }

        // Preserve any local-only pending command entries that were added
        // before hydration completed.
        const currentCommands = settingsStore.getState().commands;
        const pendingCommands = currentCommands.filter(
          (c) => c.pendingCreate,
        );

        settingsStore.setState({
          providerCatalog: mappedCatalog,
          policy: mappedPolicy,
          agentProfiles:
            persistedProfiles.length > 0
              ? persistedProfiles
              : DEFAULT_AGENT_PROFILES,
          activeAgentProfileId:
            persistedProfiles.length > 0
              ? persistedActiveId
              : DEFAULT_AGENT_PROFILES[0]?.id ?? "default-profile",
          commands: [
            ...promptCommands.map(mapPromptCommandDto),
            ...pendingCommands,
          ],
          availableShells: shells,
          hydrationPhase: "hydrated",
        });

        console.log(
          `⏱ [settings-hydration] phase-2 total: ${(performance.now() - t2).toFixed(1)}ms`,
        );
      } catch (error) {
        console.warn("Failed to hydrate phase-2 settings", error);
        // Phase 2 is non-critical — mark hydrated even on failure (downgrade).
        settingsStore.setState({ hydrationPhase: "hydrated" });
      } finally {
        console.log(
          `⏱ [settings-hydration] total: ${(performance.now() - hydrateStart).toFixed(1)}ms`,
        );
      }
    })();
  });
}
