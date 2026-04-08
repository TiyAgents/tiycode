import { createContext, useContext } from "react";
import type { TerminalSettings } from "@/modules/settings-center/model/types";
import { DEFAULT_TERMINAL_SETTINGS } from "@/modules/settings-center/model/defaults";

export const TerminalSettingsContext = createContext<TerminalSettings>(DEFAULT_TERMINAL_SETTINGS);

export function useTerminalSettings(): TerminalSettings {
  return useContext(TerminalSettingsContext);
}
