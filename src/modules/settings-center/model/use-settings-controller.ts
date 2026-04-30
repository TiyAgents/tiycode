import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import {
  GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY,
  GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY,
  GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
} from "@/modules/settings-center/model/defaults";
import {
  settingsSet,
} from "@/services/bridge";
import { useStore, shallowEqual } from "@/shared/lib/create-store";
import { settingsStore } from "./settings-store";
import { hydrateSettingsOnce } from "./settings-hydration";
import { initializeSettingsPersistenceOnce } from "./settings-persistence";
import {
  updateGeneralPreference,
  updateTerminalSetting,
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
  addWorkspace,
  removeWorkspace,
  setDefaultWorkspace,
  addProvider,
  removeProvider,
  updateProvider,
  fetchProviderModels,
  testProviderModelConnection,
  addCommand,
  removeCommand,
  updateCommand,
} from "./settings-ipc-actions";

export * from "@/modules/settings-center/model/types";

export function useSettingsController() {
  // ── One-shot init on first mount ────────────────────────────────────
  useEffect(() => {
    initializeSettingsPersistenceOnce();

    const phase = settingsStore.getState().hydrationPhase;
    if (phase === "uninitialized" || phase === "error") {
      void hydrateSettingsOnce();
    }
  }, []);

  // ── General preferences → backend sync ──────────────────────────────
  const launchAtLogin = useStore(settingsStore, (s) => s.general.launchAtLogin);
  const preventSleep = useStore(settingsStore, (s) => s.general.preventSleepWhileRunning);
  const minimizeToTray = useStore(settingsStore, (s) => s.general.minimizeToTray);
  const hydrationPhase = useStore(settingsStore, (s) => s.hydrationPhase);
  const backendHydrated =
    hydrationPhase === "hydrated" || hydrationPhase === "phase1_ready";

  useEffect(() => {
    if (!isTauri() || !backendHydrated) return;

    void Promise.all([
      settingsSet(GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY, JSON.stringify(launchAtLogin)),
      settingsSet(GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY, JSON.stringify(preventSleep)),
      settingsSet(GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY, JSON.stringify(minimizeToTray)),
    ]).catch((error) => {
      console.warn("Failed to sync general settings", error);
    });
  }, [backendHydrated, launchAtLogin, preventSleep, minimizeToTray]);

  // ── Subscribe to store fields ───────────────────────────────────────
  const general = useStore(settingsStore, (s) => s.general, shallowEqual);
  const workspacesSettings = useStore(settingsStore, (s) => s.workspaces, shallowEqual);
  const providerCatalog = useStore(settingsStore, (s) => s.providerCatalog, shallowEqual);
  const providers = useStore(settingsStore, (s) => s.providers, shallowEqual);
  const commandEntries = useStore(settingsStore, (s) => s.commands, shallowEqual);
  const terminal = useStore(settingsStore, (s) => s.terminal, shallowEqual);
  const availableShells = useStore(settingsStore, (s) => s.availableShells, shallowEqual);
  const policy = useStore(settingsStore, (s) => s.policy, shallowEqual);
  const agentProfiles = useStore(settingsStore, (s) => s.agentProfiles, shallowEqual);
  const activeAgentProfileId = useStore(settingsStore, (s) => s.activeAgentProfileId);

  // ── Backward-compatible shape ───────────────────────────────────────
  return {
    general,
    workspaces: workspacesSettings,
    providerCatalog,
    providers,
    commands: { commands: commandEntries },
    terminal,
    availableShells,
    policy,
    backendHydrated,
    updateGeneralPreference,
    addWorkspace,
    removeWorkspace,
    setDefaultWorkspace,
    addProvider,
    removeProvider,
    updateProvider,
    fetchProviderModels,
    testProviderModelConnection,
    updateTerminalSetting,
    agentProfiles,
    activeAgentProfileId,
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
