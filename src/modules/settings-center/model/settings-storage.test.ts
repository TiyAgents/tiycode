import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { SETTINGS_STORAGE_KEY, SETTINGS_STORAGE_SCHEMA_VERSION } from "@/modules/settings-center/model/defaults";
import { readStoredLocalUiSettings } from "@/modules/settings-center/model/settings-storage";

const LEGACY_SETTINGS_STORAGE_KEY = "tiy-agent-workbench-settings";

type MemoryStorage = {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
  removeItem: (key: string) => void;
  clear: () => void;
};

function createMemoryStorage(): MemoryStorage {
  const store = new Map<string, string>();
  return {
    getItem(key) {
      return store.has(key) ? store.get(key)! : null;
    },
    setItem(key, value) {
      store.set(key, value);
    },
    removeItem(key) {
      store.delete(key);
    },
    clear() {
      store.clear();
    },
  };
}

let memoryStorage: MemoryStorage;

function localStorage() {
  return (globalThis.window as { localStorage: MemoryStorage }).localStorage;
}

function setCurrentLocalUiSettings(partial?: {
  general?: Record<string, unknown>;
  terminal?: Record<string, unknown>;
  schemaVersion?: number;
}) {
  localStorage().setItem(SETTINGS_STORAGE_KEY, JSON.stringify({
    schemaVersion: partial?.schemaVersion ?? SETTINGS_STORAGE_SCHEMA_VERSION,
    general: partial?.general ?? {},
    terminal: partial?.terminal ?? {},
  }));
}

function setLegacySettings(partial?: {
  general?: Record<string, unknown>;
  terminal?: Record<string, unknown>;
}) {
  localStorage().setItem(LEGACY_SETTINGS_STORAGE_KEY, JSON.stringify({
    general: partial?.general ?? {},
    terminal: partial?.terminal ?? {},
    workspaces: [{ id: "legacy-workspace" }],
  }));
}

beforeEach(() => {
  memoryStorage = createMemoryStorage();
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { localStorage: memoryStorage },
  });
});

afterEach(() => {
  memoryStorage.clear();
  Reflect.deleteProperty(globalThis, "window");
});

describe("readStoredLocalUiSettings", () => {
  it("prefers the current local UI settings key when it is valid", () => {
    setCurrentLocalUiSettings({
      general: { minimizeToTray: false, launchAtLogin: true },
      terminal: { fontSize: 16, cursorStyle: "underline" },
    });
    setLegacySettings({
      general: { minimizeToTray: true },
      terminal: { fontSize: 99 },
    });

    const result = readStoredLocalUiSettings();

    expect(result.general.launchAtLogin).toBe(true);
    expect(result.general.minimizeToTray).toBe(false);
    expect(result.terminal.fontSize).toBe(16);
    expect(result.terminal.cursorStyle).toBe("underline");
  });

  it("migrates general and terminal from the legacy key when the current key is missing", () => {
    setLegacySettings({
      general: { launchAtLogin: true, minimizeToTray: false },
      terminal: { fontSize: 15, cursorBlink: false },
    });

    const result = readStoredLocalUiSettings();

    expect(result.general.launchAtLogin).toBe(true);
    expect(result.general.minimizeToTray).toBe(false);
    expect(result.terminal.fontSize).toBe(15);
    expect(result.terminal.cursorBlink).toBe(false);

    const migratedRaw = localStorage().getItem(SETTINGS_STORAGE_KEY);
    expect(migratedRaw).not.toBeNull();
    expect(localStorage().getItem(LEGACY_SETTINGS_STORAGE_KEY)).toBeNull();
  });

  it("falls back to defaults when the current key exists but is malformed", () => {
    localStorage().setItem(SETTINGS_STORAGE_KEY, "{not-json");
    setLegacySettings({
      general: { launchAtLogin: true },
      terminal: { fontSize: 17 },
    });

    const result = readStoredLocalUiSettings();

    expect(result.general.launchAtLogin).toBe(false);
    expect(result.terminal.fontSize).toBe(12);
    expect(localStorage().getItem(LEGACY_SETTINGS_STORAGE_KEY)).not.toBeNull();
  });

  it("falls back to defaults when no current or legacy settings exist", () => {
    const result = readStoredLocalUiSettings();

    expect(result.general.launchAtLogin).toBe(false);
    expect(result.general.minimizeToTray).toBe(true);
    expect(result.terminal.fontSize).toBe(12);
    expect(result.terminal.cursorStyle).toBe("block");
  });
});
