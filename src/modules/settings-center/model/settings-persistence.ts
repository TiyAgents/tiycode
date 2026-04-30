import { settingsStore } from "./settings-store";
import {
  persistLocalUiSettings,
} from "@/modules/settings-center/model/settings-storage";

// ---------------------------------------------------------------------------
// Single-subscription guard
// ---------------------------------------------------------------------------

let persistenceInitialized = false;

/**
 * Register the store subscriber that auto-persists `general` and `terminal`
 * to localStorage.  Safe to call from multiple places — only the first call
 * actually creates the subscription.
 */
export function initializeSettingsPersistenceOnce(): void {
  if (persistenceInitialized) return;
  persistenceInitialized = true;

  let prevGeneral = settingsStore.getState().general;
  let prevTerminal = settingsStore.getState().terminal;

  settingsStore.subscribe(() => {
    const { general, terminal, hydrationPhase } = settingsStore.getState();

    // Crucial guard: never write to localStorage before hydration is
    // complete (or at least phase-1 is done), otherwise defaults would
    // overwrite the user's stored preferences.
    if (hydrationPhase !== "hydrated" && hydrationPhase !== "phase1_ready") {
      return;
    }

    if (general !== prevGeneral || terminal !== prevTerminal) {
      prevGeneral = general;
      prevTerminal = terminal;
      persistLocalUiSettings({ general, terminal });
    }
  });
}
