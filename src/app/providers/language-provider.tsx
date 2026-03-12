import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type PropsWithChildren,
} from "react";

export type LanguagePreference = "en" | "zh-CN";

type LanguageContextValue = {
  language: LanguagePreference;
  setLanguage: (language: LanguagePreference) => void;
};

const STORAGE_KEY = "tiy-agent-language";
const LanguageContext = createContext<LanguageContextValue | null>(null);

function isLanguagePreference(value: string | null): value is LanguagePreference {
  return value === "en" || value === "zh-CN";
}

function getStoredLanguagePreference(): LanguagePreference {
  if (typeof window === "undefined") {
    return "zh-CN";
  }

  const storedValue = window.localStorage.getItem(STORAGE_KEY);
  return isLanguagePreference(storedValue) ? storedValue : "zh-CN";
}

export function LanguageProvider({ children }: PropsWithChildren) {
  const [language, setLanguageState] = useState<LanguagePreference>(() => getStoredLanguagePreference());

  useEffect(() => {
    if (typeof document === "undefined") {
      return;
    }

    document.documentElement.lang = language;
  }, [language]);

  const setLanguage = (nextLanguage: LanguagePreference) => {
    setLanguageState(nextLanguage);

    if (typeof window !== "undefined") {
      window.localStorage.setItem(STORAGE_KEY, nextLanguage);
    }
  };

  const value = useMemo(
    () => ({
      language,
      setLanguage,
    }),
    [language],
  );

  return <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>;
}

export function useLanguage() {
  const context = useContext(LanguageContext);

  if (!context) {
    throw new Error("useLanguage must be used within LanguageProvider");
  }

  return context;
}
