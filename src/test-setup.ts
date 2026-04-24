import { vi } from "vitest";

// ---------------------------------------------------------------------------
// Global localStorage mock (Map-based, matching existing test patterns)
// ---------------------------------------------------------------------------
const store = new Map<string, string>();
const memoryStorage = {
  getItem: (key: string) => (store.has(key) ? store.get(key)! : null),
  setItem: (key: string, value: string) => {
    store.set(key, value);
  },
  removeItem: (key: string) => {
    store.delete(key);
  },
  clear: () => {
    store.clear();
  },
  get length() {
    return store.size;
  },
  key: (index: number) => [...store.keys()][index] ?? null,
};

Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: memoryStorage,
});

// ---------------------------------------------------------------------------
// @tauri-apps/api mocks
// ---------------------------------------------------------------------------
vi.mock("@tauri-apps/api/core", () => {
  let channelId = 0;
  class MockChannel<T = unknown> {
    id = `ch-${++channelId}`;
    onmessage: ((data: T) => void) | null = null;
  }
  return {
    invoke: vi.fn().mockResolvedValue(undefined),
    isTauri: vi.fn().mockReturnValue(false),
    Channel: MockChannel as unknown as typeof import("@tauri-apps/api/core").Channel,
  };
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
  once: vi.fn().mockResolvedValue(vi.fn()),
  emit: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
  message: vi.fn().mockResolvedValue(undefined),
  ask: vi.fn().mockResolvedValue(false),
  confirm: vi.fn().mockResolvedValue(false),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  exit: vi.fn().mockResolvedValue(undefined),
  relaunch: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn().mockResolvedValue(null),
}));
