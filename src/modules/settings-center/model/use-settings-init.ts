import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import {
  GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY,
  GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY,
  GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
} from "@/modules/settings-center/model/defaults";
import { settingsSet } from "@/services/bridge";
import { useStore } from "@/shared/lib/create-store";
import { settingsStore } from "./settings-store";
import { hydrateSettingsOnce } from "./settings-hydration";
import { initializeSettingsPersistenceOnce } from "./settings-persistence";

/**
 * One-shot settings initialization + general preferences → backend sync.
 *
 * Safe to call from multiple components — hydration and persistence each have
 * their own single-flight guards. General settings sync is idempotent.
 */
export function useSettingsInit(): void {
  // ── One-shot hydration + persistence on first mount ─────────────────
  useEffect(() => {
    initializeSettingsPersistenceOnce();

    const phase = settingsStore.getState().hydrationPhase;
    if (phase === "uninitialized" || phase === "error") {
      void hydrateSettingsOnce();
    }
  }, []);

  // ── General preferences → backend sync ─────────────────────────────
  const launchAtLogin = useStore(settingsStore, (s) => s.general.launchAtLogin);
  const preventSleep = useStore(
    settingsStore,
    (s) => s.general.preventSleepWhileRunning,
  );
  const minimizeToTray = useStore(
    settingsStore,
    (s) => s.general.minimizeToTray,
  );
  const hydrationPhase = useStore(settingsStore, (s) => s.hydrationPhase);
  const backendHydrated =
    hydrationPhase === "hydrated" || hydrationPhase === "phase1_ready";

  useEffect(() => {
    if (!isTauri() || !backendHydrated) return;

    void Promise.all([
      settingsSet(
        GENERAL_LAUNCH_AT_LOGIN_SETTING_KEY,
        JSON.stringify(launchAtLogin),
      ),
      settingsSet(
        GENERAL_PREVENT_SLEEP_WHILE_RUNNING_SETTING_KEY,
        JSON.stringify(preventSleep),
      ),
      settingsSet(
        GENERAL_MINIMIZE_TO_TRAY_SETTING_KEY,
        JSON.stringify(minimizeToTray),
      ),
    ]).catch((error) => {
      console.warn("Failed to sync general settings", error);
    });
  }, [backendHydrated, launchAtLogin, preventSleep, minimizeToTray]);
}
