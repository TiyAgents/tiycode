import { describe, expect, it } from "vitest";
import type { ProviderEntry } from "@/modules/settings-center/model/types";

import {
  getAvailableProfileModelOptions,
  getModelSelectValue,
  getProfileTierPatch,
  markAndShouldRestoreInitialAttachmentData,
  parseModelSelectValue,
  type ComposerInitialAttachmentRestoreState,
} from "./workbench-prompt-composer";

const attachment = {
  dataUrl: "data:image/png;base64,AA==",
  id: "attachment-1",
  mediaType: "image/png",
  name: "image.png",
};

function provider(overrides: Partial<ProviderEntry> = {}): ProviderEntry {
  return {
    id: "provider-1",
    kind: "builtin",
    providerKey: "openai",
    providerType: "openai",
    displayName: "OpenAI",
    baseUrl: "https://example.test/v1",
    apiKey: "",
    hasApiKey: false,
    lockedMapping: false,
    customHeaders: {},
    enabled: true,
    models: [
      {
        id: "model-gpt-5",
        modelId: "gpt-5",
        sortIndex: 0,
        displayName: "GPT-5",
        enabled: true,
        capabilityOverrides: {},
        providerOptions: {},
      },
      {
        id: "model-disabled",
        modelId: "disabled-model",
        sortIndex: 1,
        displayName: "Disabled model",
        enabled: false,
        capabilityOverrides: {},
        providerOptions: {},
      },
    ],
    ...overrides,
  };
}

describe("markAndShouldRestoreInitialAttachmentData", () => {
  it("marks empty initial data as already restored", () => {
    const state: ComposerInitialAttachmentRestoreState = { hasRestored: false };

    expect(markAndShouldRestoreInitialAttachmentData(state, [])).toBe(false);
    expect(state.hasRestored).toBe(true);
    expect(markAndShouldRestoreInitialAttachmentData(state, [attachment])).toBe(false);
  });

  it("restores non-empty initial data only once", () => {
    const state: ComposerInitialAttachmentRestoreState = { hasRestored: false };

    expect(markAndShouldRestoreInitialAttachmentData(state, [attachment])).toBe(true);
    expect(state.hasRestored).toBe(true);
    expect(markAndShouldRestoreInitialAttachmentData(state, [attachment])).toBe(false);
  });

  it("treats undefined initial data as restored", () => {
    const state: ComposerInitialAttachmentRestoreState = { hasRestored: false };

    expect(markAndShouldRestoreInitialAttachmentData(state, undefined)).toBe(false);
    expect(state.hasRestored).toBe(true);
  });
});

describe("profile quick edit helpers", () => {
  it("builds tier-specific profile patches", () => {
    expect(getProfileTierPatch("primary", "provider-a", "model-a")).toEqual({
      primaryProviderId: "provider-a",
      primaryModelId: "model-a",
    });
    expect(getProfileTierPatch("assistant", "provider-b", "model-b")).toEqual({
      assistantProviderId: "provider-b",
      assistantModelId: "model-b",
    });
    expect(getProfileTierPatch("lite", "", "")).toEqual({
      liteProviderId: "",
      liteModelId: "",
    });
  });

  it("serializes model select values and falls back to none for incomplete selections", () => {
    expect(getModelSelectValue("provider-a", "model-a")).toBe('["provider-a","model-a"]');
    expect(getModelSelectValue("", "model-a")).toBe("__none__");
    expect(getModelSelectValue("provider-a", "")).toBe("__none__");
  });

  it("parses model select values defensively", () => {
    expect(parseModelSelectValue('["provider-a","model-a"]')).toEqual({
      providerId: "provider-a",
      modelRecordId: "model-a",
    });
    expect(parseModelSelectValue("__none__")).toEqual({ providerId: "", modelRecordId: "" });
    expect(parseModelSelectValue("not json")).toEqual({ providerId: "", modelRecordId: "" });
    expect(parseModelSelectValue('["provider-a"]')).toEqual({ providerId: "", modelRecordId: "" });
    expect(parseModelSelectValue('["provider-a", 42]')).toEqual({ providerId: "", modelRecordId: "" });
    expect(parseModelSelectValue('{"providerId":"provider-a","modelRecordId":"model-a"}')).toEqual({
      providerId: "",
      modelRecordId: "",
    });
  });

  it("collects only enabled provider models for quick selection", () => {
    const options = getAvailableProfileModelOptions([
      provider(),
      provider({
        id: "provider-disabled",
        displayName: "Disabled Provider",
        enabled: false,
        models: [
          {
            id: "model-hidden",
            modelId: "hidden-model",
            sortIndex: 0,
            displayName: "Hidden",
            enabled: true,
            capabilityOverrides: {},
            providerOptions: {},
          },
        ],
      }),
    ]);

    expect(options).toEqual([
      {
        providerId: "provider-1",
        providerName: "OpenAI",
        modelRecordId: "model-gpt-5",
        modelId: "gpt-5",
        displayName: "GPT-5",
      },
    ]);
  });
});
