import type { AgentProfile, ProviderEntry, ProviderModel } from "@/modules/settings-center/model/types";
import type { RunModelPlanDto, RunModelPlanRoleDto } from "@/shared/types/api";

type ProviderModelSelection = {
  model: ProviderModel;
  provider: ProviderEntry;
};

function isNonEmptyRecord(value: Record<string, unknown> | Record<string, string>) {
  return Object.keys(value).length > 0;
}

function findSelectedEnabledModel(
  providers: ReadonlyArray<ProviderEntry>,
  providerId: string,
  modelRecordId: string,
): ProviderModelSelection | null {
  if (!providerId || !modelRecordId) {
    return null;
  }

  const provider = providers.find((entry) => entry.id === providerId && entry.enabled);
  if (!provider) {
    return null;
  }

  const model = provider.models.find((entry) => entry.id === modelRecordId && entry.enabled);
  if (!model) {
    return null;
  }

  return { provider, model };
}

function findFirstEnabledModel(providers: ReadonlyArray<ProviderEntry>): ProviderModelSelection | null {
  for (const provider of providers) {
    if (!provider.enabled) {
      continue;
    }

    for (const model of provider.models) {
      if (!model.enabled) {
        continue;
      }

      return { provider, model };
    }
  }

  return null;
}

function toRunModelPlanRole(selection: ProviderModelSelection): RunModelPlanRoleDto {
  const { model, provider } = selection;

  return {
    providerId: provider.id,
    modelRecordId: model.id,
    provider: provider.providerType,
    providerKey: provider.providerKey,
    providerType: provider.providerType,
    providerName: provider.displayName,
    model: model.modelId,
    modelId: model.modelId,
    modelDisplayName: model.displayName || model.modelId,
    baseUrl: provider.baseUrl,
    contextWindow: model.contextWindow ?? null,
    maxOutputTokens: model.maxOutputTokens ?? null,
    customHeaders: isNonEmptyRecord(provider.customHeaders) ? provider.customHeaders : null,
    providerOptions: isNonEmptyRecord(model.providerOptions) ? model.providerOptions : null,
  };
}

export function buildRunModelPlan(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry>,
): RunModelPlanDto | null {
  const primarySelection =
    findSelectedEnabledModel(providers, profile.primaryProviderId, profile.primaryModelId)
    ?? findSelectedEnabledModel(providers, profile.assistantProviderId, profile.assistantModelId)
    ?? findSelectedEnabledModel(providers, profile.liteProviderId, profile.liteModelId)
    ?? findFirstEnabledModel(providers);

  if (!primarySelection) {
    return null;
  }

  const auxiliarySelection = findSelectedEnabledModel(
    providers,
    profile.assistantProviderId,
    profile.assistantModelId,
  );
  const lightweightSelection = findSelectedEnabledModel(
    providers,
    profile.liteProviderId,
    profile.liteModelId,
  );

  return {
    profileId: profile.id,
    profileName: profile.name,
    primary: toRunModelPlanRole(primarySelection),
    auxiliary: auxiliarySelection ? toRunModelPlanRole(auxiliarySelection) : null,
    lightweight: lightweightSelection ? toRunModelPlanRole(lightweightSelection) : null,
  };
}

export function buildRunModelPlanFromSelection(
  activeAgentProfileId: string,
  agentProfiles: ReadonlyArray<AgentProfile>,
  providers: ReadonlyArray<ProviderEntry>,
): RunModelPlanDto | null {
  const profile = agentProfiles.find((entry) => entry.id === activeAgentProfileId) ?? agentProfiles[0] ?? null;
  if (!profile) {
    return null;
  }

  return buildRunModelPlan(profile, providers);
}
