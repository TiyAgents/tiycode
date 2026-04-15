import {
  DEFAULT_GENERAL_PREFERENCES,
  DEFAULT_TERMINAL_SETTINGS,
  SETTINGS_STORAGE_KEY,
  SETTINGS_STORAGE_SCHEMA_VERSION,
} from "@/modules/settings-center/model/defaults";
import type {
  LocalUiSettingsState,
  TerminalCursorStyle,
} from "@/modules/settings-center/model/types";

const LEGACY_SETTINGS_STORAGE_KEY = "tiy-agent-workbench-settings";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isTerminalCursorStyle(value: unknown): value is TerminalCursorStyle {
  return value === "block" || value === "underline" || value === "bar";
}

function parseGeneralPreferences(raw: Record<string, unknown>) {
  return {
    launchAtLogin: typeof raw.launchAtLogin === "boolean" ? raw.launchAtLogin : DEFAULT_GENERAL_PREFERENCES.launchAtLogin,
    preventSleepWhileRunning:
      typeof raw.preventSleepWhileRunning === "boolean"
        ? raw.preventSleepWhileRunning
        : DEFAULT_GENERAL_PREFERENCES.preventSleepWhileRunning,
    minimizeToTray: typeof raw.minimizeToTray === "boolean" ? raw.minimizeToTray : DEFAULT_GENERAL_PREFERENCES.minimizeToTray,
  };
}

function parseTerminalSettings(raw: Record<string, unknown>) {
  return {
    shellPath: typeof raw.shellPath === "string" ? raw.shellPath : DEFAULT_TERMINAL_SETTINGS.shellPath,
    shellArgs: typeof raw.shellArgs === "string" ? raw.shellArgs : DEFAULT_TERMINAL_SETTINGS.shellArgs,
    fontFamily: typeof raw.fontFamily === "string" ? raw.fontFamily : DEFAULT_TERMINAL_SETTINGS.fontFamily,
    fontSize: typeof raw.fontSize === "number" ? raw.fontSize : DEFAULT_TERMINAL_SETTINGS.fontSize,
    lineHeight: typeof raw.lineHeight === "number" ? raw.lineHeight : DEFAULT_TERMINAL_SETTINGS.lineHeight,
    cursorStyle: isTerminalCursorStyle(raw.cursorStyle) ? raw.cursorStyle : DEFAULT_TERMINAL_SETTINGS.cursorStyle,
    cursorBlink: typeof raw.cursorBlink === "boolean" ? raw.cursorBlink : DEFAULT_TERMINAL_SETTINGS.cursorBlink,
    scrollback: typeof raw.scrollback === "number" ? raw.scrollback : DEFAULT_TERMINAL_SETTINGS.scrollback,
    copyOnSelect: typeof raw.copyOnSelect === "boolean" ? raw.copyOnSelect : DEFAULT_TERMINAL_SETTINGS.copyOnSelect,
    termEnv: typeof raw.termEnv === "string" ? raw.termEnv : DEFAULT_TERMINAL_SETTINGS.termEnv,
  };
}

function defaultLocalUiSettings(): LocalUiSettingsState {
  return {
    general: DEFAULT_GENERAL_PREFERENCES,
    terminal: DEFAULT_TERMINAL_SETTINGS,
  };
}

function parseLocalUiSettings(rawValue: string): LocalUiSettingsState | null {
  try {
    const parsed = JSON.parse(rawValue) as unknown;
    if (!isRecord(parsed)) {
      return null;
    }

    const schemaVersion = typeof parsed.schemaVersion === "number" ? parsed.schemaVersion : 0;
    if (schemaVersion < SETTINGS_STORAGE_SCHEMA_VERSION) {
      return null;
    }

    return {
      general: parseGeneralPreferences(isRecord(parsed.general) ? parsed.general : {}),
      terminal: parseTerminalSettings(isRecord(parsed.terminal) ? parsed.terminal : {}),
    };
  } catch {
    return null;
  }
}

function parseLegacyLocalUiSettings(rawValue: string): LocalUiSettingsState | null {
  try {
    const parsed = JSON.parse(rawValue) as unknown;
    if (!isRecord(parsed)) {
      return null;
    }

    return {
      general: parseGeneralPreferences(isRecord(parsed.general) ? parsed.general : {}),
      terminal: parseTerminalSettings(isRecord(parsed.terminal) ? parsed.terminal : {}),
    };
  } catch {
    return null;
  }
}

export function readStoredLocalUiSettings(): LocalUiSettingsState {
  if (typeof window === "undefined") {
    return defaultLocalUiSettings();
  }

  const currentRawValue = window.localStorage.getItem(SETTINGS_STORAGE_KEY);
  if (currentRawValue !== null) {
    const parsedCurrent = parseLocalUiSettings(currentRawValue);
    return parsedCurrent ?? defaultLocalUiSettings();
  }

  const legacyRawValue = window.localStorage.getItem(LEGACY_SETTINGS_STORAGE_KEY);
  if (legacyRawValue) {
    const parsedLegacy = parseLegacyLocalUiSettings(legacyRawValue);
    if (parsedLegacy) {
      persistLocalUiSettings(parsedLegacy);
      window.localStorage.removeItem(LEGACY_SETTINGS_STORAGE_KEY);
      return parsedLegacy;
    }
  }

  return defaultLocalUiSettings();
}

export function persistLocalUiSettings(settings: LocalUiSettingsState) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify({
    schemaVersion: SETTINGS_STORAGE_SCHEMA_VERSION,
    general: settings.general,
    terminal: settings.terminal,
  }));
}
