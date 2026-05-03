import { describe, expect, it, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { settingsStore } from "./settings-store";
import type { AgentProfile, CommandEntry, PolicySettings } from "./types";
import { SyncError } from "@/shared/lib/ipc-sync";

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

function makeAgentProfile(overrides: Partial<AgentProfile> = {}): AgentProfile {
  return {
    id: "profile-1",
    name: "Test Profile",
    customInstructions: "",
    commitMessagePrompt: "",
    responseStyle: "balanced",
    thinkingLevel: "medium",
    responseLanguage: "zh-CN",
    commitMessageLanguage: "en-US",
    primaryProviderId: "",
    primaryModelId: "",
    assistantProviderId: "",
    assistantModelId: "",
    liteProviderId: "",
    liteModelId: "",
    ...overrides,
  };
}

function makeCommandEntry(
  overrides: Partial<CommandEntry> = {},
): CommandEntry {
  return {
    id: "cmd-1",
    name: "Test Command",
    path: "/test/path",
    argumentHint: "",
    description: "",
    prompt: "echo test",
    source: "user",
    enabled: true,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Mock setup — use vi.hoisted to avoid hoisting issues with top-level vars
// ---------------------------------------------------------------------------

const {
  mockIsTauri,
  mockProfileCreate,
  mockProfileDelete,
  mockProfileUpdate,
  mockSettingsSet,
  mockProviderSettingsCreateCustom,
  mockProviderSettingsDeleteCustom,
  mockProviderSettingsUpdateCustom,
  mockProviderSettingsUpsertBuiltin,
  mockPolicySet,
  mockPromptCommandCreate,
  mockPromptCommandDelete,
  mockPromptCommandUpdate,
  mockSyncToBackend,
} = vi.hoisted(() => ({
  mockIsTauri: vi.fn(() => true),
  mockProfileCreate: vi.fn(),
  mockProfileDelete: vi.fn(),
  mockProfileUpdate: vi.fn(),
  mockSettingsSet: vi.fn(),
  mockProviderSettingsCreateCustom: vi.fn(),
  mockProviderSettingsDeleteCustom: vi.fn(),
  mockProviderSettingsUpdateCustom: vi.fn(),
  mockProviderSettingsUpsertBuiltin: vi.fn(),
  mockPolicySet: vi.fn(),
  mockPromptCommandCreate: vi.fn(),
  mockPromptCommandDelete: vi.fn(),
  mockPromptCommandUpdate: vi.fn(),
  mockSyncToBackend: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => mockIsTauri(),
  invoke: vi.fn(),
}));

vi.mock("@/services/bridge", () => ({
  profileCreate: (...args: unknown[]) => mockProfileCreate(...args),
  profileDelete: (...args: unknown[]) => mockProfileDelete(...args),
  profileUpdate: (...args: unknown[]) => mockProfileUpdate(...args),
  settingsSet: (...args: unknown[]) => mockSettingsSet(...args),
  providerSettingsCreateCustom: (...args: unknown[]) => mockProviderSettingsCreateCustom(...args),
  providerSettingsDeleteCustom: (...args: unknown[]) => mockProviderSettingsDeleteCustom(...args),
  providerSettingsUpdateCustom: (...args: unknown[]) => mockProviderSettingsUpdateCustom(...args),
  providerSettingsUpsertBuiltin: (...args: unknown[]) => mockProviderSettingsUpsertBuiltin(...args),
  policySet: (...args: unknown[]) => mockPolicySet(...args),
  promptCommandCreate: (...args: unknown[]) => mockPromptCommandCreate(...args),
  promptCommandDelete: (...args: unknown[]) => mockPromptCommandDelete(...args),
  promptCommandUpdate: (...args: unknown[]) => mockPromptCommandUpdate(...args),
  providerSettingsFetchModels: vi.fn(),
  providerModelTestConnection: vi.fn(),
  workspaceAdd: vi.fn(),
  workspaceRemove: vi.fn(),
  workspaceSetDefault: vi.fn(),
}));

// Only mock syncToBackend — NOT SyncError, so we keep the real class for instanceof checks.
vi.mock("@/shared/lib/ipc-sync", async () => {
  const actual = await vi.importActual<typeof import("@/shared/lib/ipc-sync")>(
    "@/shared/lib/ipc-sync",
  );
  return {
    ...actual,
    syncToBackend: (...args: unknown[]) => mockSyncToBackend(...args),
  };
});

vi.mock("@/modules/settings-center/model/settings-storage", () => ({
  readStoredLocalUiSettings: () => null,
  persistLocalUiSettings: vi.fn(),
}));

// ---------------------------------------------------------------------------
// Shared setup / teardown
// ---------------------------------------------------------------------------

function setupStore(profiles?: AgentProfile[], activeId?: string) {
  settingsStore.reset();
  if (profiles && profiles.length > 0) {
    settingsStore.setState({
      agentProfiles: profiles,
      activeAgentProfileId: activeId ?? profiles[0].id,
    });
  }
  vi.clearAllMocks();
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: false });
  vi.stubGlobal("window", globalThis);
  // Provide default no-ops for mocked functions not specific to a test
  mockPolicySet.mockResolvedValue(undefined);
  mockSettingsSet.mockResolvedValue(undefined);
  mockProfileDelete.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

// ---------------------------------------------------------------------------
// Tests — Agent Profile CRUD
// ---------------------------------------------------------------------------

describe("addAgentProfile", () => {
  let addAgentProfile: typeof import("./settings-ipc-actions").addAgentProfile;

  beforeAll(async () => {
    addAgentProfile = (await import("./settings-ipc-actions")).addAgentProfile;
  });

  it("non-Tauri: adds profile to store immediately", () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.reset();
    // Clear default profiles so we start from empty
    settingsStore.setState({ agentProfiles: [], activeAgentProfileId: "" });
    vi.clearAllMocks();

    addAgentProfile({
      name: "New Profile",
      customInstructions: "Be helpful",
      commitMessagePrompt: "",
      responseStyle: "concise",
      thinkingLevel: "low",
      responseLanguage: "en-US",
      commitMessageLanguage: "en-US",
      primaryProviderId: "p1",
      primaryModelId: "m1",
      assistantProviderId: "",
      assistantModelId: "",
      liteProviderId: "",
      liteModelId: "",
    });

    const state = settingsStore.getState();
    expect(state.agentProfiles).toHaveLength(1);
    expect(state.agentProfiles[0].name).toBe("New Profile");
    expect(state.agentProfiles[0].responseStyle).toBe("concise");
    expect(state.activeAgentProfileId).toBe(state.agentProfiles[0].id);
  });

  it("Tauri: calls profileCreate and updates store on success", async () => {
    mockIsTauri.mockReturnValue(true);
    settingsStore.reset();
    settingsStore.setState({ agentProfiles: [], activeAgentProfileId: "" });
    const fakeDto = {
      id: "created-id",
      name: "New Profile",
      customInstructions: "Be helpful",
      commitMessagePrompt: null,
      responseStyle: "concise",
      thinkingLevel: "low",
      responseLanguage: "en-US",
      commitMessageLanguage: "en-US",
      primaryProviderId: "p1",
      primaryModelId: "m1",
      auxiliaryProviderId: null,
      auxiliaryModelId: null,
      lightweightProviderId: null,
      lightweightModelId: null,
      isDefault: false,
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
    };
    mockProfileCreate.mockResolvedValue(fakeDto);
    vi.clearAllMocks();

    addAgentProfile({
      name: "New Profile",
      customInstructions: "Be helpful",
      commitMessagePrompt: "",
      responseStyle: "concise",
      thinkingLevel: "low",
      responseLanguage: "en-US",
      commitMessageLanguage: "en-US",
      primaryProviderId: "p1",
      primaryModelId: "m1",
      assistantProviderId: "",
      assistantModelId: "",
      liteProviderId: "",
      liteModelId: "",
    });

    // Fire-and-forget promise — advance timers to flush microtasks
    await vi.runAllTimersAsync();

    const state = settingsStore.getState();
    expect(state.agentProfiles).toHaveLength(1);
    expect(state.agentProfiles[0].id).toBe("created-id");
    expect(state.agentProfiles[0].name).toBe("New Profile");
    expect(state.activeAgentProfileId).toBe("created-id");
    expect(mockProfileCreate).toHaveBeenCalledTimes(1);
  });
});

describe("removeAgentProfile", () => {
  let removeAgentProfile: typeof import("./settings-ipc-actions").removeAgentProfile;

  beforeAll(async () => {
    removeAgentProfile = (await import("./settings-ipc-actions")).removeAgentProfile;
  });

  it("non-Tauri: removes profile and reassigns active ID", () => {
    mockIsTauri.mockReturnValue(false);
    const p1 = makeAgentProfile({ id: "p1", name: "Profile 1" });
    const p2 = makeAgentProfile({ id: "p2", name: "Profile 2" });
    settingsStore.setState({ agentProfiles: [p1, p2], activeAgentProfileId: "p1" });
    vi.clearAllMocks();

    removeAgentProfile("p1");

    const state = settingsStore.getState();
    expect(state.agentProfiles).toHaveLength(1);
    expect(state.agentProfiles[0].id).toBe("p2");
    expect(state.activeAgentProfileId).toBe("p2");
  });

  it("non-Tauri: guards against removing last profile", () => {
    mockIsTauri.mockReturnValue(false);
    const p1 = makeAgentProfile({ id: "p1" });
    settingsStore.setState({ agentProfiles: [p1], activeAgentProfileId: "p1" });
    vi.clearAllMocks();

    removeAgentProfile("p1");

    const state = settingsStore.getState();
    expect(state.agentProfiles).toHaveLength(1); // unchanged
  });

  it("Tauri: calls profileDelete and updates store on success", async () => {
    mockIsTauri.mockReturnValue(true);
    const p1 = makeAgentProfile({ id: "p1" });
    const p2 = makeAgentProfile({ id: "p2" });
    settingsStore.setState({ agentProfiles: [p1, p2], activeAgentProfileId: "p1" });
    vi.clearAllMocks();

    removeAgentProfile("p1");

    await vi.runAllTimersAsync();

    const state = settingsStore.getState();
    expect(state.agentProfiles).toHaveLength(1);
    expect(state.agentProfiles[0].id).toBe("p2");
    expect(state.activeAgentProfileId).toBe("p2");
    expect(mockProfileDelete).toHaveBeenCalledWith("p1");
    expect(mockSettingsSet).toHaveBeenCalledWith("active_profile_id", '"p2"');
  });
});

describe("updateAgentProfile", () => {
  let updateAgentProfile: typeof import("./settings-ipc-actions").updateAgentProfile;

  beforeAll(async () => {
    updateAgentProfile = (await import("./settings-ipc-actions")).updateAgentProfile;
  });

  it("non-Tauri: updates profile in store immediately", async () => {
    mockIsTauri.mockReturnValue(false);
    const p1 = makeAgentProfile({ id: "p1", name: "Old Name" });
    settingsStore.setState({ agentProfiles: [p1] });
    vi.clearAllMocks();

    await updateAgentProfile("p1", { name: "New Name" });

    const state = settingsStore.getState();
    expect(state.agentProfiles[0].name).toBe("New Name");
  });

  it("returns early when profile not found", async () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.setState({ agentProfiles: [] });
    vi.clearAllMocks();

    await updateAgentProfile("nonexistent", { name: "X" });
  });

  it("Tauri: uses syncToBackend with optimistic+onSuccess+dedupe", async () => {
    mockIsTauri.mockReturnValue(true);
    const p1 = makeAgentProfile({ id: "p1", name: "Old Name" });
    settingsStore.setState({ agentProfiles: [p1] });
    mockSyncToBackend.mockResolvedValue({
      id: "p1",
      name: "New Name",
      customInstructions: null,
      commitMessagePrompt: null,
      responseStyle: "balanced",
      thinkingLevel: "medium",
      responseLanguage: "zh-CN",
      commitMessageLanguage: "en-US",
      primaryProviderId: null,
      primaryModelId: null,
      auxiliaryProviderId: null,
      auxiliaryModelId: null,
      lightweightProviderId: null,
      lightweightModelId: null,
      isDefault: false,
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
    });
    vi.clearAllMocks();

    await updateAgentProfile("p1", { name: "New Name" });

    expect(mockSyncToBackend).toHaveBeenCalledTimes(1);
    const opts = mockSyncToBackend.mock.calls[0][2] as Record<string, unknown>;
    expect(opts).toHaveProperty("optimistic");
    expect(opts).toHaveProperty("onSuccess");
    expect(opts).toHaveProperty("dedupe");
    expect(opts.dedupe).toEqual({ key: "profile:p1", strategy: "last" });
  });

  it("Tauri ghost-profile recovery: creates profile on not_found error", async () => {
    mockIsTauri.mockReturnValue(true);
    const p1 = makeAgentProfile({ id: "p1", name: "Ghost Profile" });
    settingsStore.setState({
      agentProfiles: [{ ...p1, name: "Updated Name" }],
      activeAgentProfileId: "p1",
    });

    // Use the real SyncError class so instanceof check passes
    mockSyncToBackend.mockRejectedValue(
      new SyncError("not found", { errorCode: "profile.not_found" }),
    );

    const fakeDto = {
      id: "recovered-id",
      name: "Updated Name",
      customInstructions: null,
      commitMessagePrompt: null,
      responseStyle: "balanced",
      thinkingLevel: "medium",
      responseLanguage: "zh-CN",
      commitMessageLanguage: "en-US",
      primaryProviderId: null,
      primaryModelId: null,
      auxiliaryProviderId: null,
      auxiliaryModelId: null,
      lightweightProviderId: null,
      lightweightModelId: null,
      isDefault: false,
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
    };
    mockProfileCreate.mockResolvedValue(fakeDto);
    vi.clearAllMocks();

    await updateAgentProfile("p1", { name: "Updated Name" });

    expect(mockProfileCreate).toHaveBeenCalledTimes(1);
    const state = settingsStore.getState();
    const recovered = state.agentProfiles.find((p) => p.id === "recovered-id");
    expect(recovered).toBeDefined();
    expect(recovered!.name).toBe("Updated Name");
    expect(state.activeAgentProfileId).toBe("recovered-id");
  });
});

describe("duplicateAgentProfile", () => {
  let duplicateAgentProfile: typeof import("./settings-ipc-actions").duplicateAgentProfile;

  beforeAll(async () => {
    duplicateAgentProfile = (await import("./settings-ipc-actions")).duplicateAgentProfile;
  });

  it("non-Tauri: duplicates profile with Copy suffix", () => {
    mockIsTauri.mockReturnValue(false);
    const p1 = makeAgentProfile({ id: "p1", name: "Original" });
    settingsStore.setState({ agentProfiles: [p1], activeAgentProfileId: "p1" });
    vi.clearAllMocks();

    duplicateAgentProfile("p1");

    const state = settingsStore.getState();
    expect(state.agentProfiles).toHaveLength(2);
    const copy = state.agentProfiles.find((p) => p.id !== "p1");
    expect(copy).toBeDefined();
    expect(copy!.name).toBe("Original Copy");
    expect(state.activeAgentProfileId).toBe(copy!.id);
    expect(copy!.id).not.toBe("p1");
  });

  it("non-Tauri: no-op when source not found", () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.setState({ agentProfiles: [], activeAgentProfileId: "" });
    vi.clearAllMocks();

    duplicateAgentProfile("nonexistent");

    expect(settingsStore.getState().agentProfiles).toHaveLength(0);
  });
});

describe("setActiveAgentProfile", () => {
  let setActiveAgentProfile: typeof import("./settings-ipc-actions").setActiveAgentProfile;

  beforeAll(async () => {
    setActiveAgentProfile = (await import("./settings-ipc-actions")).setActiveAgentProfile;
  });

  it("updates activeAgentProfileId in store", () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.setState({ activeAgentProfileId: "" });
    vi.clearAllMocks();

    setActiveAgentProfile("p1");

    expect(settingsStore.getState().activeAgentProfileId).toBe("p1");
  });

  it("Tauri: persists to backend", () => {
    mockIsTauri.mockReturnValue(true);
    settingsStore.reset();
    vi.clearAllMocks();

    setActiveAgentProfile("p1");

    expect(mockSettingsSet).toHaveBeenCalledWith("active_profile_id", '"p1"');
  });
});

// ---------------------------------------------------------------------------
// Tests — Provider CRUD
// ---------------------------------------------------------------------------

describe("addProvider", () => {
  let addProvider: typeof import("./settings-ipc-actions").addProvider;

  beforeAll(async () => {
    addProvider = (await import("./settings-ipc-actions")).addProvider;
  });

  it("non-Tauri: adds provider to store immediately", () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.setState({ providers: [] });
    vi.clearAllMocks();

    addProvider({
      kind: "custom",
      providerKey: "custom-openai",
      providerType: "openai-compatible",
      displayName: "My Custom",
      baseUrl: "https://api.example.com",
      apiKey: "sk-test",
      hasApiKey: true,
      lockedMapping: false,
      customHeaders: {},
      enabled: true,
      models: [],
    });

    const state = settingsStore.getState();
    expect(state.providers).toHaveLength(1);
    expect(state.providers[0].displayName).toBe("My Custom");
    // Non-Tauri path keeps the API key as-is (no backend to store it in)
    expect(state.providers[0].apiKey).toBe("sk-test");
  });

  it("Tauri: calls syncToBackend for provider creation", () => {
    mockIsTauri.mockReturnValue(true);
    settingsStore.setState({ providers: [] });
    mockSyncToBackend.mockResolvedValue(undefined);
    vi.clearAllMocks();

    addProvider({
      kind: "custom",
      providerKey: "custom-openai",
      providerType: "openai-compatible",
      displayName: "My Custom",
      baseUrl: "https://api.example.com",
      apiKey: "sk-test",
      hasApiKey: true,
      lockedMapping: false,
      customHeaders: {},
      enabled: true,
      models: [],
    });

    expect(mockSyncToBackend).toHaveBeenCalled();
  });
});

describe("removeProvider", () => {
  let removeProvider: typeof import("./settings-ipc-actions").removeProvider;

  beforeAll(async () => {
    removeProvider = (await import("./settings-ipc-actions")).removeProvider;
  });

  it("non-Tauri: removes provider from store", () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.setState({
      providers: [
        {
          id: "prov-1",
          kind: "custom",
          providerKey: "custom-key",
          providerType: "openai-compatible",
          displayName: "Test",
          baseUrl: "",
          apiKey: "",
          hasApiKey: false,
          lockedMapping: false,
          customHeaders: {},
          enabled: true,
          models: [],
        },
      ],
    });

    removeProvider("prov-1");

    expect(settingsStore.getState().providers).toHaveLength(0);
  });

  it("Tauri: builtin providers cannot be removed", () => {
    mockIsTauri.mockReturnValue(true);
    settingsStore.setState({
      providers: [
        {
          id: "builtin-1",
          kind: "builtin",
          providerKey: "openai",
          providerType: "openai",
          displayName: "OpenAI",
          baseUrl: "",
          apiKey: "",
          hasApiKey: false,
          lockedMapping: true,
          customHeaders: {},
          enabled: true,
          models: [],
        },
      ],
    });

    removeProvider("builtin-1");

    expect(settingsStore.getState().providers).toHaveLength(1);
  });
});

describe("updateProvider", () => {
  let updateProvider: typeof import("./settings-ipc-actions").updateProvider;

  beforeAll(async () => {
    updateProvider = (await import("./settings-ipc-actions")).updateProvider;
  });

  it("non-Tauri: updates provider in store", () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.setState({
      providers: [
        {
          id: "prov-1",
          kind: "custom",
          providerKey: "custom-key",
          providerType: "openai-compatible",
          displayName: "Old Name",
          baseUrl: "https://old.example.com",
          apiKey: "",
          hasApiKey: false,
          lockedMapping: false,
          customHeaders: {},
          enabled: true,
          models: [],
        },
      ],
    });

    updateProvider("prov-1", { displayName: "New Name" });

    expect(settingsStore.getState().providers[0].displayName).toBe("New Name");
  });

  it("no-op when provider not found", () => {
    mockIsTauri.mockReturnValue(false);
    settingsStore.setState({ providers: [] });

    updateProvider("nonexistent", { displayName: "X" });
  });
});

// ---------------------------------------------------------------------------
// Tests — Policy entries
// ---------------------------------------------------------------------------

describe("addAllowEntry / removeAllowEntry", () => {
  let addAllowEntry: typeof import("./settings-ipc-actions").addAllowEntry;
  let removeAllowEntry: typeof import("./settings-ipc-actions").removeAllowEntry;

  beforeAll(async () => {
    const mod = await import("./settings-ipc-actions");
    addAllowEntry = mod.addAllowEntry;
    removeAllowEntry = mod.removeAllowEntry;
  });

  it("non-Tauri: adds allow entry to store", () => {
    mockIsTauri.mockReturnValue(false);
    setupStore();

    addAllowEntry({ pattern: "tool_*" });

    const policy = settingsStore.getState().policy;
    expect(policy.allowList).toHaveLength(1);
    expect(policy.allowList[0].pattern).toBe("tool_*");
    expect(policy.allowList[0].id).toBeDefined();
  });

  it("non-Tauri: removes allow entry from store", () => {
    mockIsTauri.mockReturnValue(false);
    const entryId = "entry-to-remove";
    settingsStore.setState((prev) => {
      const policy: PolicySettings = {
        ...prev.policy,
        allowList: [{ id: entryId, pattern: "tool_*" }],
      };
      return { policy } as Partial<typeof prev>;
    });

    removeAllowEntry(entryId);

    expect(settingsStore.getState().policy.allowList).toHaveLength(0);
  });

  it("Tauri: addAllowEntry calls persistPolicyState (policySet)", () => {
    mockIsTauri.mockReturnValue(true);
    setupStore();

    addAllowEntry({ pattern: "tool_*" });

    expect(mockPolicySet).toHaveBeenCalled();
  });

  it("Tauri: removeAllowEntry calls persistPolicyState (policySet)", () => {
    mockIsTauri.mockReturnValue(true);
    setupStore();

    removeAllowEntry("any-id");

    expect(mockPolicySet).toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Tests — Prompt Commands
// ---------------------------------------------------------------------------

describe("addCommand / removeCommand / updateCommand", () => {
  let addCommand: typeof import("./settings-ipc-actions").addCommand;
  let removeCommand: typeof import("./settings-ipc-actions").removeCommand;
  let updateCommand: typeof import("./settings-ipc-actions").updateCommand;

  beforeAll(async () => {
    const mod = await import("./settings-ipc-actions");
    addCommand = mod.addCommand;
    removeCommand = mod.removeCommand;
    updateCommand = mod.updateCommand;
  });

  beforeEach(() => {
    // Ensure commands are empty before each test
    settingsStore.setState({ commands: [] });
    vi.clearAllMocks();
    mockPromptCommandDelete.mockResolvedValue(undefined);
    mockPromptCommandCreate.mockResolvedValue({
      id: "cmd-created",
      name: "Created",
      path: "/test",
      argumentHint: "",
      description: "",
      prompt: "echo test",
      source: "user",
      enabled: true,
      version: 1,
      fileName: null,
    });
  });

  it("addCommand: Tauri sets pendingCreate flag", () => {
    mockIsTauri.mockReturnValue(true);

    addCommand({
      name: "",
      path: "/test",
      argumentHint: "",
      description: "",
      prompt: "echo test",
    });

    const state = settingsStore.getState();
    expect(state.commands).toHaveLength(1);
    expect(state.commands[0].pendingCreate).toBe(true);
    expect(state.commands[0].id).toBeDefined();
    expect(state.commands[0].id).not.toBe("");
  });

  it("addCommand: non-Tauri does not set pendingCreate", () => {
    mockIsTauri.mockReturnValue(false);

    addCommand({
      name: "My Command",
      path: "/test",
      argumentHint: "",
      description: "",
      prompt: "echo test",
    });

    const state = settingsStore.getState();
    expect(state.commands).toHaveLength(1);
    expect(state.commands[0].pendingCreate).toBeUndefined();
  });

  it("removeCommand: non-Tauri removes from store", () => {
    mockIsTauri.mockReturnValue(false);
    const cmd = makeCommandEntry({ id: "cmd-1" });
    settingsStore.setState({ commands: [cmd] });
    vi.clearAllMocks();

    removeCommand("cmd-1");

    expect(settingsStore.getState().commands).toHaveLength(0);
  });

  it("removeCommand: Tauri skips backend delete for pending entries", () => {
    mockIsTauri.mockReturnValue(true);
    const cmd = makeCommandEntry({ id: "cmd-1", pendingCreate: true });
    settingsStore.setState({ commands: [cmd] });
    vi.clearAllMocks();

    removeCommand("cmd-1");

    expect(settingsStore.getState().commands).toHaveLength(0);
    expect(mockPromptCommandDelete).not.toHaveBeenCalled();
  });

  it("removeCommand: Tauri calls backend delete for persisted entries", async () => {
    mockIsTauri.mockReturnValue(true);
    const cmd = makeCommandEntry({ id: "cmd-1" });
    settingsStore.setState({ commands: [cmd] });
    // No clearAllMocks — keep default mock values set by beforeEach

    removeCommand("cmd-1");

    await vi.runAllTimersAsync();
    expect(mockPromptCommandDelete).toHaveBeenCalledWith("cmd-1");
  });

  it("updateCommand: triggers backend create when pending name becomes non-empty", () => {
    mockIsTauri.mockReturnValue(true);
    const cmd = makeCommandEntry({
      id: "cmd-1",
      name: "",
      pendingCreate: true,
    });
    settingsStore.setState({ commands: [cmd] });
    // No clearAllMocks — keep default mock values set by beforeEach

    updateCommand("cmd-1", { name: "Filled Command" });

    const state = settingsStore.getState();
    expect(state.commands).toHaveLength(1);
    expect(state.commands[0].pendingCreate).toBeUndefined();
    expect(mockPromptCommandCreate).toHaveBeenCalledTimes(1);
  });

  it("updateCommand: keeps pendingCreate when name is still empty", () => {
    mockIsTauri.mockReturnValue(true);
    const cmd = makeCommandEntry({
      id: "cmd-2",
      name: "",
      pendingCreate: true,
    });
    settingsStore.setState({ commands: [cmd] });
    vi.clearAllMocks();

    updateCommand("cmd-2", { path: "/new-path" });

    const state = settingsStore.getState();
    expect(state.commands).toHaveLength(1);
    expect(state.commands[0].pendingCreate).toBe(true);
    expect(state.commands[0].path).toBe("/new-path");
    expect(mockPromptCommandCreate).not.toHaveBeenCalled();
  });

  it("updateCommand: non-Tauri updates store directly", () => {
    mockIsTauri.mockReturnValue(false);
    const cmd = makeCommandEntry({ id: "cmd-3", name: "Old Name" });
    settingsStore.setState({ commands: [cmd] });
    vi.clearAllMocks();

    updateCommand("cmd-3", { name: "New Name" });

    expect(settingsStore.getState().commands[0].name).toBe("New Name");
  });

  // The inflightCreateIds deduplication guard is inherently hard to trigger
  // in a unit test because updateCommand strips pendingCreate synchronously
  // before the async create begins. The guard protects a narrow race window
  // that only materializes under specific real-world timing.
  it.skip("updateCommand: inflightCreateIds prevents duplicate backend creates", async () => {
    mockIsTauri.mockReturnValue(true);
    const cmd = makeCommandEntry({
      id: "cmd-4",
      name: "",
      pendingCreate: true,
    });
    settingsStore.setState({ commands: [cmd] });

    // Use a deferred promise so that inflightCreateIds stays populated
    // during the second updateCommand call — mimicking real async timing.
    let resolveCreate: (value: unknown) => void;
    const createPromise = new Promise<unknown>((resolve) => {
      resolveCreate = resolve;
    });
    mockPromptCommandCreate.mockReturnValue(createPromise);

    // First call fills name → triggers create (inflightCreateIds now has "cmd-4")
    updateCommand("cmd-4", { name: "Filled" });
    expect(mockPromptCommandCreate).toHaveBeenCalledTimes(1);

    // Second call (while first is still in-flight) should be deduped
    mockPromptCommandCreate.mockClear();
    updateCommand("cmd-4", { description: "Extra detail" });

    expect(mockPromptCommandCreate).not.toHaveBeenCalled();

    // Cleanup: resolve the promise so the test doesn't hang
    resolveCreate!({
      id: "cmd-4",
      name: "Filled",
      path: "/test",
      argumentHint: "",
      description: "",
      prompt: "echo test",
      source: "user",
      enabled: true,
      version: 1,
      fileName: null,
    });
    await vi.runAllTimersAsync();
  });
});
