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
    supportsImageInput: model.capabilityOverrides.vision ?? null,
    customHeaders: isNonEmptyRecord(provider.customHeaders) ? provider.customHeaders : null,
    providerOptions: isNonEmptyRecord(model.providerOptions) ? model.providerOptions : null,
  };
}

export function buildRunModelPlan(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry>,
): RunModelPlanDto | null {
  const primarySelection = findSelectedEnabledModel(
    providers,
    profile.primaryProviderId,
    profile.primaryModelId,
  );

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
  ) ?? auxiliarySelection ?? primarySelection;

  return {
    profileId: profile.id,
    profileName: profile.name,
    customInstructions: profile.customInstructions || null,
    responseStyle: profile.responseStyle || null,
    thinkingLevel: profile.thinkingLevel && profile.thinkingLevel !== "off" ? profile.thinkingLevel : null,
    responseLanguage: profile.responseLanguage || null,
    primary: toRunModelPlanRole(primarySelection),
    auxiliary: auxiliarySelection ? toRunModelPlanRole(auxiliarySelection) : null,
    lightweight: lightweightSelection ? toRunModelPlanRole(lightweightSelection) : null,
    toolProfileByMode: {
      default: "default_full",
      plan: "plan_read_only",
    },
  };
}

export function buildProfileModelPlan(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry>,
): RunModelPlanDto | null {
  const primarySelection = findSelectedEnabledModel(
    providers,
    profile.primaryProviderId,
    profile.primaryModelId,
  );
  const auxiliarySelection = findSelectedEnabledModel(
    providers,
    profile.assistantProviderId,
    profile.assistantModelId,
  );
  const explicitLightweightSelection = findSelectedEnabledModel(
    providers,
    profile.liteProviderId,
    profile.liteModelId,
  );
  const lightweightSelection =
    explicitLightweightSelection ?? auxiliarySelection ?? primarySelection;

  if (!primarySelection && !auxiliarySelection && !lightweightSelection) {
    return null;
  }

  return {
    profileId: profile.id,
    profileName: profile.name,
    customInstructions: profile.customInstructions || null,
    responseStyle: profile.responseStyle || null,
    thinkingLevel: profile.thinkingLevel && profile.thinkingLevel !== "off" ? profile.thinkingLevel : null,
    responseLanguage: profile.responseLanguage || null,
    primary: primarySelection ? toRunModelPlanRole(primarySelection) : null,
    auxiliary: auxiliarySelection ? toRunModelPlanRole(auxiliarySelection) : null,
    lightweight: lightweightSelection ? toRunModelPlanRole(lightweightSelection) : null,
    toolProfileByMode: {
      default: "default_full",
      plan: "plan_read_only",
    },
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
