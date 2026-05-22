import { describe, expect, it } from "vitest";
import type { ProviderEntry } from "@/modules/settings-center/model/types";

import {
  getAvailableProfileModelOptions,
  getProfileTierPatch,
  markAndShouldRestoreInitialAttachmentData,
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
