import { AppProviders } from "@/app/providers/app-providers";
import { AppRouterProvider } from "@/app/router/provider";

export function App() {
  return (
    <AppProviders>
      <AppRouterProvider />
    </AppProviders>
  );
}
