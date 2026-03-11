import { useCallback, useEffect, useState } from "react";
import { getSystemMetadata } from "@/features/system-info/api/get-system-metadata";
import type { SystemMetadata } from "@/shared/types/system";

type State = {
  data: SystemMetadata | null;
  error: string | null;
  isLoading: boolean;
};

const initialState: State = {
  data: null,
  error: null,
  isLoading: true,
};

export function useSystemMetadata() {
  const [state, setState] = useState<State>(initialState);

  const refetch = useCallback(async () => {
    setState((current) => ({ ...current, error: null, isLoading: true }));

    try {
      const data = await getSystemMetadata();
      setState({ data, error: null, isLoading: false });
    } catch (error) {
      const message = error instanceof Error ? error.message : "读取运行时信息失败";
      setState({ data: null, error: message, isLoading: false });
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
