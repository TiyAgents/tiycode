import { Globe, Moon, Monitor, Sun } from "lucide-react";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import { useT } from "@/i18n";
import { cn } from "@/shared/lib/utils";

type LanguageThemeStepProps = {
  language: LanguagePreference;
  theme: ThemePreference;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
};

const LANGUAGE_OPTIONS: ReadonlyArray<{ label: string; value: LanguagePreference; flag: string }> = [
  { label: "English", value: "en", flag: "EN" },
  { label: "简体中文", value: "zh-CN", flag: "中" },
];

const THEME_OPTIONS: ReadonlyArray<{
  value: ThemePreference;
  labelKey: "onboarding.langTheme.themeAuto" | "onboarding.langTheme.themeDark" | "onboarding.langTheme.themeLight";
  icon: typeof Monitor;
}> = [
  { value: "system", labelKey: "onboarding.langTheme.themeAuto", icon: Monitor },
  { value: "dark", labelKey: "onboarding.langTheme.themeDark", icon: Moon },
  { value: "light", labelKey: "onboarding.langTheme.themeLight", icon: Sun },
];

export function LanguageThemeStep({
  language,
  theme,
  onSelectLanguage,
  onSelectTheme,
}: LanguageThemeStepProps) {
  const t = useT();

  return (
    <div className="space-y-8">
      <div className="space-y-4">
        <div className="flex items-center gap-2 text-sm font-medium text-app-foreground">
          <Globe className="size-4" />
          <span>{t("onboarding.langTheme.languageLabel")}</span>
        </div>
        <div className="grid grid-cols-2 gap-3">
          {LANGUAGE_OPTIONS.map((option) => {
            const isSelected = language === option.value;

            return (
              <button
                key={option.value}
                type="button"
                className={cn(
                  "flex items-center gap-3 rounded-xl border px-4 py-3.5 text-left transition-all duration-200",
                  isSelected
                    ? "border-app-foreground/30 bg-app-foreground/6 text-app-foreground shadow-[0_0_0_1px_var(--app-foreground,theme(colors.foreground))]"
                    : "border-app-border bg-app-surface/60 text-app-muted hover:border-app-border-strong hover:bg-app-surface hover:text-app-foreground",
                )}
                onClick={() => onSelectLanguage(option.value)}
              >
                <div
                  className={cn(
                    "flex size-9 shrink-0 items-center justify-center rounded-lg text-[13px] font-bold",
                    isSelected
                      ? "bg-app-foreground/10 text-app-foreground"
                      : "bg-app-surface-muted text-app-subtle",
                  )}
                >
                  {option.flag}
                </div>
                <span className="text-sm font-medium">{option.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="space-y-4">
        <div className="text-sm font-medium text-app-foreground">
          {t("onboarding.langTheme.themeLabel")}
        </div>
        <div className="grid grid-cols-3 gap-3">
          {THEME_OPTIONS.map((option) => {
            const isSelected = theme === option.value;
            const Icon = option.icon;

            return (
              <button
                key={option.value}
                type="button"
                className={cn(
                  "flex flex-col items-center gap-2.5 rounded-xl border px-4 py-4 transition-all duration-200",
                  isSelected
                    ? "border-app-foreground/30 bg-app-foreground/6 text-app-foreground shadow-[0_0_0_1px_var(--app-foreground,theme(colors.foreground))]"
                    : "border-app-border bg-app-surface/60 text-app-muted hover:border-app-border-strong hover:bg-app-surface hover:text-app-foreground",
                )}
                onClick={() => onSelectTheme(option.value)}
              >
                <Icon
                  className={cn(
                    "size-5",
                    isSelected ? "text-app-foreground" : "text-app-subtle",
                  )}
                />
                <span className="text-[13px] font-medium">{t(option.labelKey)}</span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
