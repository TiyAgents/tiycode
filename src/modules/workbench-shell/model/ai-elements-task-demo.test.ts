import { describe, expect, it } from "vitest";
import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import {
  resolveProfileModelByTier,
  getProfilePrimaryModelId,
  getProfilePrimaryModelLabel,
} from "@/modules/workbench-shell/model/ai-elements-task-demo";

function createProvider(overrides: Partial<ProviderEntry> = {}): ProviderEntry {
  return {
    id: "prov-1",
    kind: "builtin",
    providerKey: "openai",
    providerType: "openai",
    displayName: "OpenAI",
    baseUrl: "https://api.openai.com",
    apiKey: "sk-test",
    hasApiKey: true,
    lockedMapping: false,
    customHeaders: {},
    enabled: true,
    models: [
      {
        id: "model-1",
        modelId: "gpt-4o",
        sortIndex: 0,
        displayName: "GPT-4o",
        enabled: true,
        capabilityOverrides: {},
        providerOptions: {},
      },
    ],
    ...overrides,
  };
}

function createProfile(overrides: Partial<AgentProfile> = {}): AgentProfile {
  return {
    id: "profile-1",
    name: "Default Profile",
    customInstructions: "",
    commitMessagePrompt: "",
    responseStyle: "balanced",
    thinkingLevel: "off",
    responseLanguage: "",
    commitMessageLanguage: "",
    primaryProviderId: "prov-1",
    primaryModelId: "model-1",
    assistantProviderId: "",
    assistantModelId: "",
    liteProviderId: "",
    liteModelId: "",
    ...overrides,
  };
}

describe("resolveProfileModelByTier", () => {
  it("resolves primary model from providers", () => {
    const result = resolveProfileModelByTier("primary", createProfile(), [createProvider()]);
    expect(result).not.toBeNull();
    expect(result!.displayName).toBe("GPT-4o");
    expect(result!.modelId).toBe("gpt-4o");
  });

  it("resolves assistant model from providers", () => {
    const provider = createProvider({
      id: "prov-2",
      models: [
        {
          id: "model-2",
          modelId: "claude-3",
          sortIndex: 0,
          displayName: "Claude 3",
          enabled: true,
          capabilityOverrides: {},
          providerOptions: {},
        },
      ],
    });
    const profile = createProfile({
      assistantProviderId: "prov-2",
      assistantModelId: "model-2",
    });
    const result = resolveProfileModelByTier("assistant", profile, [createProvider(), provider]);
    expect(result).not.toBeNull();
    expect(result!.displayName).toBe("Claude 3");
  });

  it("resolves lite model from providers", () => {
    const profile = createProfile({
      liteProviderId: "prov-1",
      liteModelId: "model-1",
    });
    const result = resolveProfileModelByTier("lite", profile, [createProvider()]);
    expect(result).not.toBeNull();
    expect(result!.modelId).toBe("gpt-4o");
  });

  it("returns null when tier provider/model ids are empty", () => {
    const profile = createProfile({
      assistantProviderId: "",
      assistantModelId: "",
    });
    const result = resolveProfileModelByTier("assistant", profile, [createProvider()]);
    expect(result).toBeNull();
  });

  it("falls back to modelRecordId when provider is not found", () => {
    const profile = createProfile({
      primaryProviderId: "nonexistent",
      primaryModelId: "some-model-id",
    });
    const result = resolveProfileModelByTier("primary", profile, [createProvider()]);
    expect(result).not.toBeNull();
    expect(result!.displayName).toBe("some-model-id");
    expect(result!.modelId).toBe("some-model-id");
  });

  it("falls back to modelRecordId when model is not found in provider", () => {
    const profile = createProfile({
      primaryProviderId: "prov-1",
      primaryModelId: "nonexistent-model",
    });
    const result = resolveProfileModelByTier("primary", profile, [createProvider()]);
    expect(result).not.toBeNull();
    expect(result!.displayName).toBe("nonexistent-model");
  });
});

describe("getProfilePrimaryModelId", () => {
  it("returns the modelId of the primary model", () => {
    const result = getProfilePrimaryModelId(createProfile(), [createProvider()]);
    expect(result).toBe("gpt-4o");
  });

  it("falls back to assistant when primary ids are empty", () => {
    const profile = createProfile({
      primaryProviderId: "",
      primaryModelId: "",
      assistantProviderId: "prov-1",
      assistantModelId: "model-1",
    });
    const result = getProfilePrimaryModelId(profile, [createProvider()]);
    expect(result).toBe("gpt-4o");
  });

  it("falls back through assistant to lite", () => {
    const profile = createProfile({
      primaryProviderId: "",
      primaryModelId: "",
      assistantProviderId: "",
      assistantModelId: "",
      liteProviderId: "prov-1",
      liteModelId: "model-1",
    });
    const result = getProfilePrimaryModelId(profile, [createProvider()]);
    expect(result).toBe("gpt-4o");
  });

  it("returns profile name as fallback when no models match and no providers", () => {
    const profile = createProfile({
      primaryProviderId: "",
      primaryModelId: "",
      name: "My Profile",
    });
    const result = getProfilePrimaryModelId(profile, []);
    expect(result).toBe("My Profile");
  });

  it("returns 'Current Profile' when profile name is empty and no models", () => {
    const profile = createProfile({
      primaryProviderId: "",
      primaryModelId: "",
      name: "",
    });
    const result = getProfilePrimaryModelId(profile, []);
    expect(result).toBe("Current Profile");
  });

  it("falls back to first enabled model from first enabled provider when ids are empty", () => {
    const provider = createProvider({
      enabled: true,
      models: [
        {
          id: "m-fallback",
          modelId: "fallback-model",
          sortIndex: 0,
          displayName: "Fallback",
          enabled: true,
          capabilityOverrides: {},
          providerOptions: {},
        },
      ],
    });
    const profile = createProfile({
      primaryProviderId: "",
      primaryModelId: "",
    });
    const result = getProfilePrimaryModelId(profile, [provider]);
    expect(result).toBe("fallback-model");
  });

  it("skips disabled providers in fallback", () => {
    const disabledProvider = createProvider({
      id: "prov-disabled",
      enabled: false,
      models: [
        {
          id: "m-disabled",
          modelId: "disabled-model",
          sortIndex: 0,
          displayName: "Disabled",
          enabled: true,
          capabilityOverrides: {},
          providerOptions: {},
        },
      ],
    });
    const enabledProvider = createProvider({
      id: "prov-enabled",
      enabled: true,
      models: [
        {
          id: "m-enabled",
          modelId: "enabled-model",
          sortIndex: 0,
          displayName: "Enabled",
          enabled: true,
          capabilityOverrides: {},
          providerOptions: {},
        },
      ],
    });
    const profile = createProfile({
      primaryProviderId: "",
      primaryModelId: "",
    });
    const result = getProfilePrimaryModelId(profile, [disabledProvider, enabledProvider]);
    expect(result).toBe("enabled-model");
  });
});

describe("getProfilePrimaryModelLabel", () => {
  it("returns the displayName of the primary model", () => {
    const result = getProfilePrimaryModelLabel(createProfile(), [createProvider()]);
    expect(result).toBe("GPT-4o");
  });

  it("falls back to modelId when displayName is empty", () => {
    const provider = createProvider({
      models: [
        {
          id: "model-1",
          modelId: "gpt-4o",
          sortIndex: 0,
          displayName: "",
          enabled: true,
          capabilityOverrides: {},
          providerOptions: {},
        },
      ],
    });
    const result = getProfilePrimaryModelLabel(createProfile(), [provider]);
    expect(result).toBe("gpt-4o");
  });
});
