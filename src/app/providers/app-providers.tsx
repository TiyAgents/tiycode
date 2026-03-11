import type { PropsWithChildren } from "react";

export function AppProviders({ children }: PropsWithChildren) {
  return <div className="dark min-h-screen bg-background text-foreground">{children}</div>;
}
