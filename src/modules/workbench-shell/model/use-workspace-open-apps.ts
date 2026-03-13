import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
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
  const [state, setState] = useState<State>(initialState);

  const refetch = useCallback(async () => {
    setState((current) => ({ ...current, error: null, isLoading: true }));

    try {
      const data = await invoke<Array<WorkspaceOpenApp>>("get_workspace_open_apps");
      setState({ data, error: null, isLoading: false });
    } catch (error) {
      const message = error instanceof Error ? error.message : "读取可用应用失败";
      setState({ data: [], error: message, isLoading: false });
    }
  }, []);

  useEffect(() => {
    void refetch();
  }, [refetch]);

  return {
    ...state,
    refetch,
  };
}
