import type { PropsWithChildren } from "react";
import { LanguageProvider } from "@/app/providers/language-provider";
import { ThemeProvider } from "@/app/providers/theme-provider";

export function AppProviders({ children }: PropsWithChildren) {
  return (
    <ThemeProvider>
      <LanguageProvider>
        <div className="min-h-screen bg-background text-foreground">{children}</div>
      </LanguageProvider>
    </ThemeProvider>
  );
}
