import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Rocket, X } from "lucide-react";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import { useT } from "@/i18n";
import { Button } from "@/shared/ui/button";
import { useOnboarding } from "@/modules/onboarding/model/use-onboarding";
import { ONBOARDING_STEPS } from "@/modules/onboarding/model/types";
import { LanguageThemeStep } from "@/modules/onboarding/ui/steps/language-theme-step";
import { ProviderStep } from "@/modules/onboarding/ui/steps/provider-step";
import { ProfileStep } from "@/modules/onboarding/ui/steps/profile-step";
import { CompleteStep } from "@/modules/onboarding/ui/steps/complete-step";
import type {
  AgentProfile,
  ProviderCatalogEntry,
  ProviderEntry,
} from "@/modules/settings-center/model/types";
import type { TranslationKey } from "@/i18n/locales/zh-CN";

type OnboardingWizardProps = {
  language: LanguagePreference;
  theme: ThemePreference;
  providerCatalog: Array<ProviderCatalogEntry>;
  providers: Array<ProviderEntry>;
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onAddProvider: (entry: Omit<ProviderEntry, "id">) => void;
  onUpdateProvider: (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => void;
  onFetchProviderModels: (id: string) => Promise<void>;
  onUpdateAgentProfile: (id: string, patch: Partial<Omit<AgentProfile, "id">>) => void;
  onDismiss: () => void;
};

const STEP_LABELS: ReadonlyArray<{ step: (typeof ONBOARDING_STEPS)[number]; labelKey: TranslationKey }> = [
  { step: "language-theme", labelKey: "onboarding.langTheme.title" },
  { step: "provider", labelKey: "onboarding.provider.title" },
  { step: "profile", labelKey: "onboarding.profile.title" },
  { step: "complete", labelKey: "onboarding.complete.title" },
];

export function OnboardingWizard({
  language,
  theme,
  providerCatalog,
  providers,
  agentProfiles,
  activeAgentProfileId,
  onSelectLanguage,
  onSelectTheme,
  onAddProvider,
  onUpdateProvider,
  onFetchProviderModels,
  onUpdateAgentProfile,
  onDismiss,
}: OnboardingWizardProps) {
  const t = useT();
  const hasAppliedDefaults = useRef(false);
  const {
    currentStep,
    currentIndex,
    isFirstStep,
    isLastStep,
    goNext,
    goBack,
    complete,
    skip,
  } = useOnboarding();

  // Lifted state: survives ProviderStep unmount/remount across step switches
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(
    () => providers[0]?.id ?? null,
  );
  // Store API key drafts per provider id so they survive step switches
  const [apiKeyDrafts, setApiKeyDrafts] = useState<Record<string, string>>({});

  const activeProfile =
    agentProfiles.find((profile) => profile.id === activeAgentProfileId) ??
    agentProfiles[0] ??
    null;

  // Apply defaults on first mount: English + System theme
  useEffect(() => {
    if (hasAppliedDefaults.current) {
      return;
    }

    hasAppliedDefaults.current = true;
    onSelectLanguage("en");
    onSelectTheme("system");

    if (activeProfile) {
      onUpdateAgentProfile(activeProfile.id, { responseLanguage: "English" });
    }
  }, []);

  const handleLanguageChange = (nextLanguage: LanguagePreference) => {
    onSelectLanguage(nextLanguage);

    if (activeProfile) {
      const responseLanguage = nextLanguage === "zh-CN" ? "Chinese" : "English";
      onUpdateAgentProfile(activeProfile.id, { responseLanguage });
    }
  };

  const handleComplete = () => {
    complete();
    onDismiss();
  };

  const handleSkip = () => {
    skip();
    onDismiss();
  };

  const enabledModelsCount = providers.reduce(
    (count, provider) =>
      provider.enabled
        ? count + provider.models.filter((model) => model.enabled).length
        : count,
    0,
  );

  const canProceed = (() => {
    switch (currentStep) {
      case "language-theme":
        return true;
      case "provider":
        return enabledModelsCount > 0;
      case "profile":
        return true;
      case "complete":
        return true;
    }
  })();

  const stepDescription = (() => {
    switch (currentStep) {
      case "language-theme":
        return t("onboarding.langTheme.desc");
      case "provider":
        return t("onboarding.provider.desc");
      case "profile":
        return t("onboarding.profile.desc");
      case "complete":
        return t("onboarding.complete.desc");
    }
  })();

  // Progress: 0% at step 0, 100% at last step
  const progressPercent = (currentIndex / (ONBOARDING_STEPS.length - 1)) * 100;

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-app-canvas/95 backdrop-blur-xl">
      {/* Skip button - only visible after step 1 */}
      {!isFirstStep ? (
        <button
          type="button"
          className="absolute right-4 top-4 flex size-8 items-center justify-center rounded-lg text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
          onClick={handleSkip}
          title={t("onboarding.skip")}
        >
          <X className="size-4" />
        </button>
      ) : null}

      {/* Main card */}
      <div className="relative flex max-h-[min(90vh,720px)] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-app-border bg-app-surface shadow-[0_32px_96px_rgba(15,23,42,0.18)] dark:shadow-[0_32px_96px_rgba(0,0,0,0.48)]">
        {/* Top progress bar */}
        <div className="absolute inset-x-0 top-0 z-10 h-[3px] bg-app-border/50">
          <div
            className="h-full rounded-r-full bg-app-foreground transition-[width] duration-500 ease-out"
            style={{ width: `${progressPercent}%` }}
          />
        </div>

        {/* Header */}
        <div className="shrink-0 border-b border-app-border px-6 pb-4 pt-5">
          <div className="flex items-start gap-4">
            <div className="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-xl shadow-[0_2px_8px_rgba(15,23,42,0.08)] dark:shadow-[0_2px_8px_rgba(0,0,0,0.3)]">
              <img
                src="/app-icon.png"
                alt="TiyCode"
                className="size-full object-cover"
              />
            </div>
            <div className="min-w-0 flex-1">
              <h1 className="text-[17px] font-semibold tracking-[-0.02em] text-app-foreground">
                {t(STEP_LABELS[currentIndex].labelKey)}
              </h1>
              {currentStep !== "complete" ? (
                <p className="mt-1 text-[13px] leading-5 text-app-muted">
                  {stepDescription}
                </p>
              ) : null}
            </div>
          </div>
        </div>

        {/* Content */}
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden pr-1">
          <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-6 py-5 [scrollbar-width:thin]">
            {currentStep === "language-theme" ? (
              <LanguageThemeStep
                language={language}
                theme={theme}
                onSelectLanguage={handleLanguageChange}
                onSelectTheme={onSelectTheme}
              />
            ) : null}

            {currentStep === "provider" ? (
              <ProviderStep
                providerCatalog={providerCatalog}
                providers={providers}
                selectedProviderId={selectedProviderId}
                onSelectProvider={setSelectedProviderId}
                apiKeyDrafts={apiKeyDrafts}
                onApiKeyDraftsChange={setApiKeyDrafts}
                onAddProvider={onAddProvider}
                onUpdateProvider={onUpdateProvider}
                onFetchProviderModels={onFetchProviderModels}
              />
            ) : null}

            {currentStep === "profile" && activeProfile ? (
              <ProfileStep
                providers={providers}
                activeProfile={activeProfile}
                onUpdateProfile={onUpdateAgentProfile}
              />
            ) : null}

            {currentStep === "complete" ? <CompleteStep /> : null}
          </div>
        </div>

        {/* Footer */}
        <div className="shrink-0 border-t border-app-border px-6 py-4">
          <div className="flex items-center justify-between">
            <div>
              {!isFirstStep && !isLastStep ? (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-9 rounded-lg px-3 text-[13px]"
                  onClick={goBack}
                >
                  <ArrowLeft className="size-4" />
                  {t("onboarding.back")}
                </Button>
              ) : null}
            </div>

            <div>
              {isLastStep ? (
                <Button
                  size="sm"
                  className="h-9 rounded-lg px-5 text-[13px]"
                  onClick={handleComplete}
                >
                  <Rocket className="size-4" />
                  {t("onboarding.finish")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  className="h-9 rounded-lg px-5 text-[13px]"
                  onClick={goNext}
                  disabled={!canProceed}
                >
                  {t("onboarding.next")}
                  <ArrowRight className="size-4" />
                </Button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
