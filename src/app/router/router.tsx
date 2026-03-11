import { createHashRouter } from "react-router-dom";
import { HomePage } from "@/pages/home/ui/home-page";

export const appRouter = createHashRouter([
  {
    path: "/",
    element: <HomePage />,
  },
]);
