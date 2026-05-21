import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { SETTINGS_STORAGE_KEY, SETTINGS_STORAGE_SCHEMA_VERSION } from "@/modules/settings-center/model/defaults";
import { persistLocalUiSettings, readStoredLocalUiSettings } from "@/modules/settings-center/model/settings-storage";

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
      general: { minimizeToTray: false, launchAtLogin: true, defaultAppendMessageKind: "steer" },
      terminal: { fontSize: 16, cursorStyle: "underline" },
    });
    setLegacySettings({
      general: { minimizeToTray: true },
      terminal: { fontSize: 99 },
    });

    const result = readStoredLocalUiSettings();

    expect(result.general.launchAtLogin).toBe(true);
    expect(result.general.minimizeToTray).toBe(false);
    expect(result.general.defaultAppendMessageKind).toBe("steer");
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
    expect(result.general.defaultAppendMessageKind).toBe("follow_up");
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
    expect(result.general.defaultAppendMessageKind).toBe("follow_up");
    expect(result.terminal.fontSize).toBe(12);
    expect(localStorage().getItem(LEGACY_SETTINGS_STORAGE_KEY)).not.toBeNull();
  });

  it("falls back to defaults when no current or legacy settings exist", () => {
    const result = readStoredLocalUiSettings();

    expect(result.general.launchAtLogin).toBe(false);
    expect(result.general.minimizeToTray).toBe(true);
    expect(result.general.defaultAppendMessageKind).toBe("follow_up");
    expect(result.terminal.fontSize).toBe(12);
    expect(result.terminal.cursorStyle).toBe("block");
  });

  it("falls back to the default append message kind when the stored value is invalid", () => {
    setCurrentLocalUiSettings({
      general: { defaultAppendMessageKind: "invalid" },
    });

    const result = readStoredLocalUiSettings();

    expect(result.general.defaultAppendMessageKind).toBe("follow_up");
  });

  it("persists and restores the append message kind", () => {
    persistLocalUiSettings({
      general: {
        launchAtLogin: false,
        preventSleepWhileRunning: false,
        minimizeToTray: true,
        defaultAppendMessageKind: "steer",
      },
      terminal: {
        shellPath: "",
        shellArgs: "",
        fontFamily: "monospace",
        fontSize: 13,
        lineHeight: 1.4,
        cursorStyle: "bar",
        cursorBlink: true,
        scrollback: 1000,
        copyOnSelect: true,
        termEnv: "xterm-256color",
      },
    });

    const result = readStoredLocalUiSettings();

    expect(result.general.defaultAppendMessageKind).toBe("steer");
  });
});
