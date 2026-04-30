import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { settingsStore } from "./settings-store";
import { hydrateSettingsOnce } from "./settings-hydration";

// ---------------------------------------------------------------------------
// Shared fixtures
// ---------------------------------------------------------------------------

function makeProviderDto(id: string, key = `provider-${id}`) {
  return {
    id,
    kind: "builtin" as const,
    providerKey: key,
    providerType: "openai",
    displayName: `Provider ${id}`,
    baseUrl: `https://${key}.com`,
    hasApiKey: false,
    lockedMapping: false,
    customHeaders: {},
    enabled: true,
    models: [],
  };
}

function makeWorkspaceDto(id: string) {
  return {
    id,
    name: `Workspace ${id}`,
    path: `/path/${id}`,
    canonicalPath: `/canonical/${id}`,
    isDefault: false,
    isGit: false,
    autoWorkTree: false,
    kind: "standalone" as const,
    parentWorkspaceId: null,
    worktreeName: null,
  };
}

function makeSettingDto(key: string, value: unknown) {
  return { key, value: typeof value === "string" ? value : JSON.stringify(value) };
}

// ---------------------------------------------------------------------------
// Mock setup — use vi.hoisted to avoid hoisting issues with top-level vars
// ---------------------------------------------------------------------------

const {
  mockProviderSettingsGetAll,
  mockWorkspaceList,
  mockSettingsGet,
  mockSettingsSet,
  mockProfileList,
  mockPolicyGetAll,
  mockPromptCommandList,
  mockProviderCatalogList,
  mockProfileCreate,
  mockInvoke,
  mockIsTauri,
  mockWaitForBackendReady,
} = vi.hoisted(() => ({
  mockProviderSettingsGetAll: vi.fn(),
  mockWorkspaceList: vi.fn(),
  mockSettingsGet: vi.fn(),
  mockSettingsSet: vi.fn(),
  mockProfileList: vi.fn(),
  mockPolicyGetAll: vi.fn(),
  mockPromptCommandList: vi.fn(),
  mockProviderCatalogList: vi.fn(),
  mockProfileCreate: vi.fn(),
  mockInvoke: vi.fn(),
  mockIsTauri: vi.fn(() => true),
  mockWaitForBackendReady: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => mockIsTauri(),
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock("@/services/bridge", () => ({
  providerSettingsGetAll: mockProviderSettingsGetAll,
  workspaceList: mockWorkspaceList,
  settingsGet: mockSettingsGet,
  settingsSet: mockSettingsSet,
  profileList: mockProfileList,
  policyGetAll: mockPolicyGetAll,
  promptCommandList: mockPromptCommandList,
  providerCatalogList: mockProviderCatalogList,
  profileCreate: mockProfileCreate,
}));

vi.mock("@/modules/settings-center/model/settings-storage", () => ({
  readStoredLocalUiSettings: () => null,
}));

vi.mock("@/shared/lib/backend-ready", () => ({
  waitForBackendReady: mockWaitForBackendReady,
}));

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("hydrateSettingsOnce", () => {
  beforeEach(() => {
    settingsStore.reset();
    vi.useFakeTimers({ shouldAdvanceTime: false });
    vi.clearAllMocks();

    // Provide window alias so scheduleDeferred can access requestIdleCallback/setTimeout
    // vitest fake timers only hook globalThis.setTimeout — make window delegate there
    vi.stubGlobal("window", globalThis);

    // Default healthy responses for phase 1
    mockProviderSettingsGetAll.mockResolvedValue([makeProviderDto("p1")]);
    mockWorkspaceList.mockResolvedValue([makeWorkspaceDto("ws1")]);
    mockSettingsGet.mockResolvedValue(
      makeSettingDto("active_profile_id", "default-profile"),
    );
    // Phase 2
    mockProviderCatalogList.mockResolvedValue([]);
    mockPolicyGetAll.mockResolvedValue([]);
    mockProfileList.mockResolvedValue([]);
    mockPromptCommandList.mockResolvedValue([]);
    // Shells invoke
    mockInvoke.mockResolvedValue([]);
    // Profile create (for seeding default when DB empty)
    mockProfileCreate.mockRejectedValue(new Error("not needed in most tests"));
    mockIsTauri.mockReturnValue(true);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // -----------------------------------------------------------------------
  // Single-flight
  // -----------------------------------------------------------------------

  describe("single-flight", () => {
    it("concurrent calls both enter hydration (known limitation)", async () => {
      // NOTE: hydrateSettingsOnce doesn't gate on hydratePromise before
      // entering the hydration block — it only checks hydrationPhase.
      // Two concurrent callers at `uninitialized` both enter.
      // useSettingsController wraps this safely by only calling once.
      const p1 = hydrateSettingsOnce();
      const p2 = hydrateSettingsOnce();

      await vi.runAllTimersAsync();
      await Promise.all([p1, p2]);

      // Both callers entered — this is the current behavior until source is
      // patched to check hydratePromise before creating a new batch.
      expect(mockProviderSettingsGetAll).toHaveBeenCalledTimes(2);
    });

    it("subsequent call after hydration reuses resolved promise", async () => {
      await vi.runAllTimersAsync();
      await hydrateSettingsOnce();

      mockProviderSettingsGetAll.mockClear();
      mockProviderCatalogList.mockClear();

      await hydrateSettingsOnce();

      expect(mockProviderSettingsGetAll).not.toHaveBeenCalled();
      expect(mockProviderCatalogList).not.toHaveBeenCalled();
    });
  });

  // -----------------------------------------------------------------------
  // Phase 1
  // -----------------------------------------------------------------------

  describe("phase 1", () => {
    it("transitions to hydrated after full flow", async () => {
      const hydrationPromise = hydrateSettingsOnce();

      await vi.runAllTimersAsync();
      await hydrationPromise;

      const state = settingsStore.getState();
      expect(state.hydrationPhase).toBe("hydrated");
      expect(state.providers.length).toBe(1);
      expect(state.workspaces.length).toBe(1);
      expect(mockProviderSettingsGetAll).toHaveBeenCalledTimes(1);
      expect(mockWorkspaceList).toHaveBeenCalledTimes(1);
      expect(mockSettingsGet).toHaveBeenCalled();
    });

    it("sets phase to error on phase-1 failure", async () => {
      mockProviderSettingsGetAll.mockRejectedValue(
        new Error("backend unavailable"),
      );

      const p = hydrateSettingsOnce();
      await vi.runAllTimersAsync();
      await p;

      expect(settingsStore.getState().hydrationPhase).toBe("error");
    });

    it("retries after error phase", async () => {
      mockProviderSettingsGetAll.mockRejectedValueOnce(
        new Error("backend unavailable"),
      );
      await vi.runAllTimersAsync();
      await hydrateSettingsOnce();
      expect(settingsStore.getState().hydrationPhase).toBe("error");

      mockProviderSettingsGetAll.mockResolvedValue([
        makeProviderDto("p1"),
      ]);
      mockWorkspaceList.mockResolvedValue([makeWorkspaceDto("ws1")]);
      mockSettingsGet.mockResolvedValue(
        makeSettingDto("active_profile_id", "default-profile"),
      );

      const p2 = hydrateSettingsOnce();
      await vi.runAllTimersAsync(); // run phase 1 + phase 2 deferred timers
      await p2;

      expect(settingsStore.getState().hydrationPhase).toBe("hydrated");
      expect(settingsStore.getState().providers.length).toBe(1);
    });
  });

  // -----------------------------------------------------------------------
  // Phase 1_ready guard (regression test)
  // -----------------------------------------------------------------------

  describe("phase1_ready guard", () => {
    it("short-circuits when phase is already phase1_ready", async () => {
      settingsStore.setState({
        hydrationPhase: "phase1_ready",
        providers: [
          {
            id: "p1",
            kind: "builtin" as const,
            providerKey: "openai",
            providerType: "openai" as const,
            displayName: "OpenAI",
            baseUrl: "",
            apiKey: "",
            hasApiKey: false,
            lockedMapping: false,
            customHeaders: {},
            enabled: true,
            models: [],
          },
        ],
        workspaces: [
          {
            id: "ws1",
            name: "WS",
            path: "/ws",
            isDefault: false,
            isGit: false,
            autoWorkTree: false,
            kind: "standalone" as const,
            parentWorkspaceId: null,
            worktreeHash: null,
          },
        ],
      });

      mockProviderSettingsGetAll.mockClear();
      mockProviderCatalogList.mockClear();

      await hydrateSettingsOnce();

      expect(mockProviderSettingsGetAll).not.toHaveBeenCalled();
    });
  });

  // -----------------------------------------------------------------------
  // Phase 2
  // -----------------------------------------------------------------------

  describe("phase 2", () => {
    it("populates catalog, policies, profiles, commands after phase-1", async () => {
      mockProviderCatalogList.mockResolvedValue([
        {
          providerKey: "openai",
          providerType: "openai",
          displayName: "OpenAI",
          builtin: true,
          supportsCustom: false,
          defaultBaseUrl: "https://api.openai.com",
        },
      ]);
      mockPolicyGetAll.mockResolvedValue([]);
      mockProfileList.mockResolvedValue([
        {
          id: "profile-1",
          name: "Default",
          customInstructions: "",
          commitMessagePrompt: "",
          responseStyle: "balanced" as const,
          thinkingLevel: "medium" as const,
          responseLanguage: "",
          commitMessageLanguage: "",
          primaryProviderId: "",
          primaryModelId: "",
          auxiliaryProviderId: "",
          auxiliaryModelId: "",
          lightweightProviderId: "",
          lightweightModelId: "",
        },
      ]);
      mockPromptCommandList.mockResolvedValue([]);

      const p = hydrateSettingsOnce();
      await vi.runAllTimersAsync();
      await p;

      const state = settingsStore.getState();
      expect(state.hydrationPhase).toBe("hydrated");
      expect(state.providerCatalog.length).toBe(1);
      expect(state.agentProfiles.length).toBe(1);
    });

    it("marks hydrated even when phase-2 fails (downgrade)", async () => {
      mockProviderCatalogList.mockRejectedValue(
        new Error("catalog unavailable"),
      );

      const p = hydrateSettingsOnce();
      await vi.runAllTimersAsync();
      await p;

      expect(settingsStore.getState().hydrationPhase).toBe("hydrated");
    });

    it("seeds default profile when DB is empty", async () => {
      mockProfileList.mockResolvedValue([]);
      mockProfileCreate.mockResolvedValueOnce({
        id: "new-profile",
        name: "Default",
        customInstructions: "",
        commitMessagePrompt: "",
        responseStyle: "balanced",
        thinkingLevel: "medium",
        responseLanguage: "",
        commitMessageLanguage: "",
        primaryProviderId: "",
        primaryModelId: "",
        auxiliaryProviderId: "",
        auxiliaryModelId: "",
        lightweightProviderId: "",
        lightweightModelId: "",
      });

      const p = hydrateSettingsOnce();
      await vi.runAllTimersAsync();
      await p;

      const state = settingsStore.getState();
      expect(state.hydrationPhase).toBe("hydrated");
      expect(state.agentProfiles.length).toBe(1);
      expect(state.agentProfiles[0].id).toBe("new-profile");
      expect(mockProfileCreate).toHaveBeenCalledTimes(1);
    });
  });

  // -----------------------------------------------------------------------
  // Web-only mode
  // -----------------------------------------------------------------------

  describe("web-only mode", () => {
    it("resolves immediately when not in Tauri", async () => {
      mockIsTauri.mockReturnValue(false);

      await hydrateSettingsOnce();

      expect(mockProviderSettingsGetAll).not.toHaveBeenCalled();
      expect(settingsStore.getState().hydrationPhase).toBe("uninitialized");
      expect(settingsStore.getState().agentProfiles.length).toBeGreaterThan(0);
    });
  });

  // -----------------------------------------------------------------------
  // Provider deduplication
  // -----------------------------------------------------------------------

  describe("provider deduplication", () => {
    it("deduplicates providers by providerKey", async () => {
      mockProviderSettingsGetAll.mockResolvedValue([
        makeProviderDto("p1", "openai"),
        makeProviderDto("p2", "openai"),
        makeProviderDto("p3", "anthropic"),
      ]);

      const p = hydrateSettingsOnce();
      await vi.runAllTimersAsync();
      await p;

      expect(settingsStore.getState().providers).toHaveLength(2);
      expect(settingsStore.getState().providers[0].providerKey).toBe("openai");
      expect(settingsStore.getState().providers[1].providerKey).toBe(
        "anthropic",
      );
    });
  });
});
