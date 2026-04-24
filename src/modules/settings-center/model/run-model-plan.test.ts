import { describe, expect, it } from "vitest";
import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import {
  buildRunModelPlan,
  buildProfileModelPlan,
  buildRunModelPlanFromSelection,
} from "@/modules/settings-center/model/run-model-plan";

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
        contextWindow: "128000",
        maxOutputTokens: "16384",
        capabilityOverrides: { vision: true },
        providerOptions: {},
      },
    ],
    ...overrides,
  };
}

function createProfile(overrides: Partial<AgentProfile> = {}): AgentProfile {
  return {
    id: "profile-1",
    name: "Default",
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

describe("buildRunModelPlan", () => {
  it("returns null when primary model is not found", () => {
    const profile = createProfile({ primaryProviderId: "nonexistent" });
    const result = buildRunModelPlan(profile, [createProvider()]);
    expect(result).toBeNull();
  });

  it("returns null when primary provider is disabled", () => {
    const profile = createProfile();
    const result = buildRunModelPlan(profile, [createProvider({ enabled: false })]);
    expect(result).toBeNull();
  });

  it("returns null when primary model is disabled", () => {
    const provider = createProvider({
      models: [
        {
          id: "model-1",
          modelId: "gpt-4o",
          sortIndex: 0,
          displayName: "GPT-4o",
          enabled: false,
          capabilityOverrides: {},
          providerOptions: {},
        },
      ],
    });
    const result = buildRunModelPlan(createProfile(), [provider]);
    expect(result).toBeNull();
  });

  it("returns a valid plan with primary model only", () => {
    const profile = createProfile();
    const providers = [createProvider()];
    const result = buildRunModelPlan(profile, providers);

    expect(result).not.toBeNull();
    expect(result!.profileId).toBe("profile-1");
    expect(result!.profileName).toBe("Default");
    expect(result!.primary.providerId).toBe("prov-1");
    expect(result!.primary.model).toBe("gpt-4o");
    expect(result!.primary.modelDisplayName).toBe("GPT-4o");
    expect(result!.auxiliary).toBeNull();
    // lightweight falls back to primary when no auxiliary/lite set
    expect(result!.lightweight).not.toBeNull();
    expect(result!.lightweight!.model).toBe("gpt-4o");
  });

  it("includes auxiliary when configured", () => {
    const auxProvider = createProvider({
      id: "prov-2",
      displayName: "Aux Provider",
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
    const result = buildRunModelPlan(profile, [createProvider(), auxProvider]);

    expect(result!.auxiliary).not.toBeNull();
    expect(result!.auxiliary!.model).toBe("claude-3");
  });

  it("lightweight falls back to auxiliary then primary", () => {
    const auxProvider = createProvider({
      id: "prov-2",
      displayName: "Aux Provider",
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
    const result = buildRunModelPlan(profile, [createProvider(), auxProvider]);

    // lightweight should fall back to auxiliary since lite is not set
    expect(result!.lightweight!.model).toBe("claude-3");
  });

  it("sets customHeaders when provider has them", () => {
    const provider = createProvider({
      customHeaders: { "X-Custom": "value" },
    });
    const result = buildRunModelPlan(createProfile(), [provider]);
    expect(result!.primary.customHeaders).toEqual({ "X-Custom": "value" });
  });

  it("sets customHeaders to null when provider has empty headers", () => {
    const result = buildRunModelPlan(createProfile(), [createProvider()]);
    expect(result!.primary.customHeaders).toBeNull();
  });

  it("sets providerOptions when model has them", () => {
    const provider = createProvider({
      models: [
        {
          id: "model-1",
          modelId: "gpt-4o",
          sortIndex: 0,
          displayName: "GPT-4o",
          enabled: true,
          capabilityOverrides: { vision: true },
          providerOptions: { temperature: 0.7 },
        },
      ],
    });
    const result = buildRunModelPlan(createProfile(), [provider]);
    expect(result!.primary.providerOptions).toEqual({ temperature: 0.7 });
  });

  it("nullifies thinkingLevel when set to off", () => {
    const result = buildRunModelPlan(
      createProfile({ thinkingLevel: "off" }),
      [createProvider()],
    );
    expect(result!.thinkingLevel).toBeNull();
  });

  it("preserves thinkingLevel when not off", () => {
    const result = buildRunModelPlan(
      createProfile({ thinkingLevel: "high" }),
      [createProvider()],
    );
    expect(result!.thinkingLevel).toBe("high");
  });

  it("includes toolProfileByMode", () => {
    const result = buildRunModelPlan(createProfile(), [createProvider()]);
    expect(result!.toolProfileByMode).toEqual({
      default: "default_full",
      plan: "plan_read_only",
    });
  });

  it("falls back modelDisplayName to modelId when displayName is empty", () => {
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
    const result = buildRunModelPlan(createProfile(), [provider]);
    expect(result!.primary.modelDisplayName).toBe("gpt-4o");
  });
});

describe("buildProfileModelPlan", () => {
  it("returns null when no selections are found", () => {
    const profile = createProfile({
      primaryProviderId: "",
      primaryModelId: "",
    });
    const result = buildProfileModelPlan(profile, []);
    expect(result).toBeNull();
  });

  it("returns plan with null primary when primary is not found but auxiliary exists", () => {
    const auxProvider = createProvider({
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
      primaryProviderId: "nonexistent",
      primaryModelId: "nonexistent",
      assistantProviderId: "prov-2",
      assistantModelId: "model-2",
    });
    const result = buildProfileModelPlan(profile, [auxProvider]);

    expect(result).not.toBeNull();
    expect(result!.primary).toBeNull();
    expect(result!.auxiliary).not.toBeNull();
    expect(result!.auxiliary!.model).toBe("claude-3");
  });
});

describe("buildRunModelPlanFromSelection", () => {
  it("finds the correct profile and builds a plan", () => {
    const profiles = [
      createProfile({ id: "p1", name: "First" }),
      createProfile({ id: "p2", name: "Second" }),
    ];
    const result = buildRunModelPlanFromSelection("p2", profiles, [createProvider()]);
    expect(result).not.toBeNull();
    expect(result!.profileName).toBe("Second");
  });

  it("falls back to the first profile when activeAgentProfileId is not found", () => {
    const profiles = [createProfile({ id: "p1", name: "Fallback" })];
    const result = buildRunModelPlanFromSelection("nonexistent", profiles, [createProvider()]);
    expect(result).not.toBeNull();
    expect(result!.profileName).toBe("Fallback");
  });

  it("returns null when profiles array is empty", () => {
    const result = buildRunModelPlanFromSelection("any", [], [createProvider()]);
    expect(result).toBeNull();
  });

  it("returns null when primary model is not found in providers", () => {
    const profiles = [createProfile({ primaryProviderId: "nonexistent" })];
    const result = buildRunModelPlanFromSelection("profile-1", profiles, [createProvider()]);
    expect(result).toBeNull();
  });
});
