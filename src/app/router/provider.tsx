import { RouterProvider } from "react-router-dom";
import { appRouter } from "@/app/router/router";

export function AppRouterProvider() {
  return <RouterProvider router={appRouter} />;
}
