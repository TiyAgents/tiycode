import { describe, expect, it } from "vitest";
import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import {
  buildProfileModelPlan,
  buildRunModelPlan,
  buildRunModelPlanFromSelection,
} from "./run-model-plan";

function profile(overrides: Partial<AgentProfile> = {}): AgentProfile {
  return {
    id: "profile-1",
    name: "Default",
    customInstructions: "Be concise",
    commitMessagePrompt: "",
    responseStyle: "balanced",
    responseLanguage: "zh-CN",
    commitMessageLanguage: "en",
    thinkingLevel: "medium",
    primaryProviderId: "provider-1",
    primaryModelId: "model-primary",
    assistantProviderId: "provider-1",
    assistantModelId: "model-assistant",
    liteProviderId: "provider-2",
    liteModelId: "model-lite",
    ...overrides,
  };
}

function provider(overrides: Partial<ProviderEntry> = {}): ProviderEntry {
  return {
    id: "provider-1",
    kind: "builtin",
    providerKey: "openai",
    providerType: "openai",
    displayName: "OpenAI",
    baseUrl: "https://api.example.test",
    apiKey: "",
    hasApiKey: false,
    lockedMapping: false,
    enabled: true,
    customHeaders: { "x-test": "1" },
    models: [
      {
        id: "model-primary",
        modelId: "gpt-5",
        sortIndex: 0,
        displayName: "GPT 5",
        enabled: true,
        contextWindow: "1000",
        maxOutputTokens: "200",
        capabilityOverrides: { vision: true },
        providerOptions: { effort: "medium" },
      },
      {
        id: "model-assistant",
        modelId: "gpt-5-mini",
        sortIndex: 1,
        displayName: "Mini",
        enabled: true,
        capabilityOverrides: {},
        providerOptions: {},
      },
    ],
    ...overrides,
  };
}

describe("buildRunModelPlan", () => {
  it("builds primary, auxiliary, and lightweight roles from enabled selections", () => {
    const plan = buildRunModelPlan(profile(), [
      provider(),
      provider({
        id: "provider-2",
        providerKey: "anthropic",
        providerType: "anthropic",
        displayName: "Anthropic",
        customHeaders: {},
        models: [{
          id: "model-lite",
          modelId: "claude-lite",
          sortIndex: 0,
          displayName: "Claude Lite",
          enabled: true,
          capabilityOverrides: {},
          providerOptions: {},
        }],
      }),
    ]);

    expect(plan?.profileId).toBe("profile-1");
    expect(plan?.primary?.modelDisplayName).toBe("GPT 5");
    expect(plan?.primary?.supportsImageInput).toBe(true);
    expect(plan?.primary?.supportsReasoning).toBe(true);
    expect(plan?.primary?.customHeaders).toEqual({ "x-test": "1" });
    expect(plan?.primary?.providerOptions).toEqual({ effort: "medium" });
    expect(plan?.auxiliary?.modelId).toBe("gpt-5-mini");
    expect(plan?.lightweight?.modelId).toBe("claude-lite");
    expect(plan?.toolProfileByMode?.plan).toBe("plan_read_only");
  });

  it("returns null when the primary model cannot be selected", () => {
    expect(buildRunModelPlan(profile({ primaryModelId: "missing" }), [provider()])).toBeNull();
    expect(buildRunModelPlan(profile(), [provider({ enabled: false })])).toBeNull();
  });

  it("falls back lightweight to auxiliary then primary", () => {
    const withoutLite = buildRunModelPlan(profile({ liteModelId: "missing" }), [provider()]);
    expect(withoutLite?.lightweight?.modelId).toBe("gpt-5-mini");

    const primaryOnly = buildRunModelPlan(
      profile({ assistantModelId: "missing", liteModelId: "missing" }),
      [provider()],
    );
    expect(primaryOnly?.lightweight?.modelId).toBe("gpt-5");
  });
  it("infers reasoning support when no manual capability override is stored", () => {
    const plan = buildRunModelPlan(
      profile({
        primaryModelId: "model-deepseek-reasoner",
        assistantModelId: "missing",
        liteModelId: "missing",
      }),
      [
        provider({
          models: [
            {
              id: "model-deepseek-reasoner",
              modelId: "deepseek-reasoner",
              sortIndex: 0,
              displayName: "DeepSeek Reasoner",
              enabled: true,
              capabilityOverrides: {},
              providerOptions: {},
            },
          ],
        }),
      ],
    );

    expect(plan?.primary?.supportsReasoning).toBe(true);
    expect(plan?.primary?.supportsImageInput).toBe(false);
  });

  it("allows manual reasoning capability overrides to disable inferred support", () => {
    const plan = buildRunModelPlan(
      profile({
        primaryModelId: "model-gpt-5",
        assistantModelId: "missing",
        liteModelId: "missing",
      }),
      [
        provider({
          models: [
            {
              id: "model-gpt-5",
              modelId: "gpt-5",
              sortIndex: 0,
              displayName: "GPT 5",
              enabled: true,
              capabilityOverrides: { reasoning: false },
              providerOptions: {},
            },
          ],
        }),
      ],
    );

    expect(plan?.primary?.supportsReasoning).toBe(false);
  });
});

describe("buildProfileModelPlan", () => {
  it("allows partial profile plans when primary is unavailable", () => {
    const plan = buildProfileModelPlan(
      profile({ primaryModelId: "missing", liteModelId: "missing" }),
      [provider()],
    );

    expect(plan?.primary).toBeNull();
    expect(plan?.auxiliary?.modelId).toBe("gpt-5-mini");
    expect(plan?.lightweight?.modelId).toBe("gpt-5-mini");
  });

  it("returns null when no role can be selected", () => {
    expect(buildProfileModelPlan(profile(), [])).toBeNull();
  });
});

describe("buildRunModelPlanFromSelection", () => {
  it("uses selected profile or falls back to first profile", () => {
    const first = profile({ id: "first", name: "First" });
    const second = profile({ id: "second", name: "Second" });

    expect(buildRunModelPlanFromSelection("second", [first, second], [provider()])?.profileId).toBe("second");
    expect(buildRunModelPlanFromSelection("missing", [first, second], [provider()])?.profileId).toBe("first");
    expect(buildRunModelPlanFromSelection("missing", [], [provider()])).toBeNull();
  });
});
