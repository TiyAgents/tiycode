import { useCallback } from "react";
import { useLanguage, type LanguagePreference } from "@/app/providers/language-provider";
import zhCN, { type TranslationKey } from "@/i18n/locales/zh-CN";
import en from "@/i18n/locales/en";

const LOCALE_MAP: Record<LanguagePreference, Record<TranslationKey, string>> = {
  "zh-CN": zhCN,
  en,
};

export function translate(
  language: LanguagePreference,
  key: TranslationKey,
  params?: Record<string, string | number>,
): string {
  const table = LOCALE_MAP[language];
  let text = table[key] ?? zhCN[key] ?? key;

  if (params) {
    for (const [paramKey, paramValue] of Object.entries(params)) {
      text = text.split(`{{${paramKey}}}`).join(String(paramValue));
    }
  }

  return text;
}

export function useT() {
  const { language } = useLanguage();

  const t = useCallback(
    (key: TranslationKey, params?: Record<string, string | number>) =>
      translate(language, key, params),
    [language],
  );

  return t;
}

export type { TranslationKey };
