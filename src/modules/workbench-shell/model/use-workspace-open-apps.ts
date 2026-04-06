import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { useT } from "@/i18n";
import type { WorkspaceOpenApp } from "@/modules/workbench-shell/model/types";

type State = {
  data: Array<WorkspaceOpenApp>;
  error: string | null;
  isLoading: boolean;
};

const initialState: State = {
  data: [],
  error: null,
  isLoading: true,
};

export function useWorkspaceOpenApps() {
  const t = useT();
  const [state, setState] = useState<State>(initialState);

  const refetch = useCallback(async () => {
    setState((current) => ({ ...current, error: null, isLoading: true }));

    if (!isTauri()) {
      setState({ data: [], error: null, isLoading: false });
      return;
    }

    try {
      const data = await invoke<Array<WorkspaceOpenApp>>("get_workspace_open_apps");
      setState({ data, error: null, isLoading: false });
    } catch (error) {
      const message = error instanceof Error ? error.message : t("workspaceApps.error.readApps");
      setState({ data: [], error: message, isLoading: false });
    }
  }, [t]);

  useEffect(() => {
    void refetch();
  }, [refetch]);

  return {
    ...state,
    refetch,
  };
}
