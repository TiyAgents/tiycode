import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";

function resolveProviderModel(
  providers: ReadonlyArray<ProviderEntry>,
  providerId: string,
  modelRecordId: string,
) {
  for (const provider of providers) {
    if (provider.id !== providerId) {
      continue;
    }

    for (const model of provider.models) {
      if (model.id === modelRecordId) {
        return {
          displayName: model.displayName || model.modelId,
          modelId: model.modelId,
        };
      }
    }
  }

  return null;
}

function getFallbackProviderModel(providers: ReadonlyArray<ProviderEntry>) {
  for (const provider of providers) {
    if (!provider.enabled) {
      continue;
    }

    for (const model of provider.models) {
      if (!model.enabled) {
        continue;
      }

      return {
        displayName: model.displayName || model.modelId,
        modelId: model.modelId,
      };
    }
  }

  return null;
}

function resolveProfilePrimaryModel(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry> = [],
) {
  const providerId = profile.primaryProviderId || profile.assistantProviderId || profile.liteProviderId || "";
  const modelRecordId = profile.primaryModelId || profile.assistantModelId || profile.liteModelId || "";

  if (providerId && modelRecordId) {
    const providerModel = resolveProviderModel(providers, providerId, modelRecordId);
    if (providerModel) {
      return providerModel;
    }

    return {
      displayName: modelRecordId,
      modelId: modelRecordId,
    };
  }

  const fallbackProviderModel = getFallbackProviderModel(providers);
  if (fallbackProviderModel) {
    return fallbackProviderModel;
  }

  return {
    displayName: profile.name || "Current Profile",
    modelId: profile.name || "Current Profile",
  };
}

export function getProfilePrimaryModelId(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry> = [],
) {
  return resolveProfilePrimaryModel(profile, providers).modelId;
}

export function getProfilePrimaryModelLabel(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry> = [],
) {
  return resolveProfilePrimaryModel(profile, providers).displayName;
}
