import { useMemo } from "react";
import { Brain, Check, Cpu, Zap } from "lucide-react";
import { useT } from "@/i18n";
import { matchProviderIcon } from "@/shared/lib/llm-brand-matcher";
import { cn } from "@/shared/lib/utils";
import { LocalLlmIcon } from "@/shared/ui/local-llm-icon";
import type {
  AgentProfile,
  ProviderEntry,
} from "@/modules/settings-center/model/types";
import type { TranslationKey } from "@/i18n/locales/zh-CN";

type ProfileStepProps = {
  providers: Array<ProviderEntry>;
  activeProfile: AgentProfile;
  onUpdateProfile: (id: string, patch: Partial<Omit<AgentProfile, "id">>) => void;
};

type TierConfig = {
  labelKey: TranslationKey;
  descKey: TranslationKey;
  icon: typeof Brain;
  providerIdField: "primaryProviderId" | "assistantProviderId" | "liteProviderId";
  modelIdField: "primaryModelId" | "assistantModelId" | "liteModelId";
};

const TIERS: ReadonlyArray<TierConfig> = [
  {
    labelKey: "onboarding.profile.primaryLabel",
    descKey: "onboarding.profile.primaryDesc",
    icon: Brain,
    providerIdField: "primaryProviderId",
    modelIdField: "primaryModelId",
  },
  {
    labelKey: "onboarding.profile.auxiliaryLabel",
    descKey: "onboarding.profile.auxiliaryDesc",
    icon: Cpu,
    providerIdField: "assistantProviderId",
    modelIdField: "assistantModelId",
  },
  {
    labelKey: "onboarding.profile.liteLabel",
    descKey: "onboarding.profile.liteDesc",
    icon: Zap,
    providerIdField: "liteProviderId",
    modelIdField: "liteModelId",
  },
];

type EnabledModelOption = {
  providerId: string;
  providerName: string;
  providerKey: string;
  modelRecordId: string;
  modelId: string;
  modelDisplayName: string;
};

export function ProfileStep({
  providers,
  activeProfile,
  onUpdateProfile,
}: ProfileStepProps) {
  const t = useT();

  const enabledModels = useMemo(() => {
    const options: EnabledModelOption[] = [];

    for (const provider of providers) {
      if (!provider.enabled) {
        continue;
      }

      for (const model of provider.models) {
        if (!model.enabled) {
          continue;
        }

        options.push({
          providerId: provider.id,
          providerName: provider.displayName,
          providerKey: provider.providerKey,
          modelRecordId: model.id,
          modelId: model.modelId,
          modelDisplayName: model.displayName || model.modelId,
        });
      }
    }

    return options;
  }, [providers]);

  if (enabledModels.length === 0) {
    return (
      <div className="flex min-h-[200px] items-center justify-center rounded-xl border border-dashed border-app-border bg-app-canvas/40 text-center text-[13px] text-app-muted">
        {t("onboarding.profile.noEnabledModels")}
      </div>
    );
  }

  return (
    <div className="flex min-h-[420px] flex-1 flex-col gap-5">
      {TIERS.map((tier) => {
        const currentProviderId = activeProfile[tier.providerIdField];
        const currentModelId = activeProfile[tier.modelIdField];
        const Icon = tier.icon;

        return (
          <div key={tier.modelIdField} className="shrink-0 space-y-2">
            <div className="flex items-start gap-3">
              <div className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-lg bg-app-surface-muted text-app-subtle">
                <Icon className="size-3.5" />
              </div>
              <div className="min-w-0">
                <div className="text-[13px] font-medium text-app-foreground">
                  {t(tier.labelKey)}
                </div>
                <p className="mt-0.5 text-[11px] leading-4 text-app-muted">
                  {t(tier.descKey)}
                </p>
              </div>
            </div>

            <div className="max-h-[120px] overflow-y-auto rounded-xl border border-app-border bg-app-canvas/50 [scrollbar-width:thin]">
              <div className="divide-y divide-app-border/60">
                {enabledModels.map((option) => {
                  const isSelected =
                    currentProviderId === option.providerId &&
                    currentModelId === option.modelRecordId;

                  return (
                    <button
                      key={`${option.providerId}:${option.modelRecordId}`}
                      type="button"
                      className={cn(
                        "flex w-full items-center gap-3 px-3 py-2 text-left transition-colors",
                        isSelected
                          ? "bg-app-foreground/6 text-app-foreground"
                          : "text-app-muted hover:bg-app-surface-hover/50 hover:text-app-foreground",
                      )}
                      onClick={() =>
                        onUpdateProfile(activeProfile.id, {
                          [tier.providerIdField]: option.providerId,
                          [tier.modelIdField]: option.modelRecordId,
                        })
                      }
                    >
                      <LocalLlmIcon
                        slug={matchProviderIcon(option.providerKey) ?? "default"}
                        className="size-4 shrink-0"
                      />
                      <div className="min-w-0 flex-1 flex items-baseline gap-2">
                        <span className="truncate text-[12px] font-medium">
                          {option.modelDisplayName}
                        </span>
                        <span className="shrink-0 text-[10px] text-app-subtle">
                          {option.providerName}
                        </span>
                      </div>
                      {isSelected ? (
                        <Check className="size-3.5 shrink-0 text-app-foreground" />
                      ) : null}
                    </button>
                  );
                })}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
