import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import type {
  TerminalAttachDto,
  TerminalSessionDto,
  TerminalStreamEvent,
} from "@/shared/types/api";
import { terminalClient } from "@/features/terminal/api/terminal-client";
import { terminalStore, useTerminalStore } from "@/features/terminal/model/terminal-store";

type UseThreadTerminalOptions = {
  threadId: string | null;
  cols: number;
  rows: number;
  active: boolean;
  onReplay?: (replay: string) => void;
  onStdout?: (data: string) => void;
  onStderr?: (data: string) => void;
  onExit?: (exitCode: number | null) => void;
};

export function useThreadTerminal({
  threadId,
  cols,
  rows,
  active,
  onReplay,
  onStdout,
  onStderr,
  onExit,
}: UseThreadTerminalOptions) {
  const [error, setError] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const attachGenerationRef = useRef(0);
  const onReplayRef = useRef(onReplay);
  const onStdoutRef = useRef(onStdout);
  const onStderrRef = useRef(onStderr);
  const onExitRef = useRef(onExit);

  onReplayRef.current = onReplay;
  onStdoutRef.current = onStdout;
  onStderrRef.current = onStderr;
  onExitRef.current = onExit;

  const session = useTerminalStore((current) =>
    threadId ? current.sessionsByThreadId[threadId] ?? null : null,
  );
  const sessionId = session?.sessionId ?? null;

  useEffect(() => {
    terminalStore.setActiveThread(active ? threadId : null);
  }, [active, threadId]);

  useEffect(() => {
    if (!active || !threadId) {
      return;
    }

    if (!isTauri()) {
      setError("Terminal 仅在 Tauri 桌面环境中可用。");
      return;
    }

    let cancelled = false;
    const attachGeneration = attachGenerationRef.current + 1;
    attachGenerationRef.current = attachGeneration;
    setIsConnecting(true);
    setError(null);

    const handleAttach = (payload: TerminalAttachDto) => {
      if (attachGeneration !== attachGenerationRef.current) {
        return;
      }
      terminalStore.upsertSession(payload.session);
      onReplayRef.current?.(payload.replay);
    };

    const handleEvent = (event: TerminalStreamEvent) => {
      if (attachGeneration !== attachGenerationRef.current) {
        return;
      }

      if (event.threadId !== threadId) {
        return;
      }

      switch (event.type) {
        case "session_created":
          terminalStore.upsertSession(event.session);
          break;
        case "stdout_chunk":
          terminalStore.setSessionMeta(threadId, {
            hasUnreadOutput: false,
            lastOutputAt: new Date().toISOString(),
          });
          onStdoutRef.current?.(event.data);
          break;
        case "stderr_chunk":
          terminalStore.setSessionMeta(threadId, {
            hasUnreadOutput: false,
            lastOutputAt: new Date().toISOString(),
          });
          onStderrRef.current?.(event.data);
          break;
        case "status_changed":
          terminalStore.setSessionMeta(threadId, { status: event.status });
          break;
        case "session_exited":
          terminalStore.setSessionMeta(threadId, {
            status: "exited",
            exitCode: event.exitCode,
          });
          onExitRef.current?.(event.exitCode);
          break;
      }
    };

    void terminalClient
      .createOrAttach(threadId, handleEvent, cols, rows)
      .then((payload) => {
        if (cancelled) {
          return;
        }
        handleAttach(payload);
      })
      .catch((attachError) => {
        if (cancelled) {
          return;
        }
        const message =
          attachError instanceof Error ? attachError.message : String(attachError);
        setError(message);
      })
      .finally(() => {
        if (!cancelled) {
          setIsConnecting(false);
        }
      });

    return () => {
      cancelled = true;
      if (attachGenerationRef.current === attachGeneration) {
        attachGenerationRef.current += 1;
      }
    };
  }, [active, threadId]);

  useEffect(() => {
    if (!active || !threadId || !sessionId || !isTauri()) {
      return;
    }

    void terminalClient.resize(threadId, cols, rows).catch((resizeError) => {
      const message =
        resizeError instanceof Error ? resizeError.message : String(resizeError);
      setError(message);
    });
  }, [active, cols, rows, sessionId, threadId]);

  const actions = useMemo(
    () => ({
      async writeInput(data: string) {
        if (!threadId) {
          return;
        }
        await terminalClient.writeInput(threadId, data);
      },
      async resize(nextCols: number, nextRows: number) {
        if (!threadId) {
          return;
        }
        await terminalClient.resize(threadId, nextCols, nextRows);
      },
      async restart(nextCols?: number, nextRows?: number) {
        if (!threadId) {
          return;
        }

        setError(null);
        const payload = await terminalClient.restart(
          threadId,
          (event) => {
            if (event.threadId !== threadId) {
              return;
            }

            switch (event.type) {
              case "session_created":
                terminalStore.upsertSession(event.session);
                break;
              case "stdout_chunk":
                onStdoutRef.current?.(event.data);
                break;
              case "stderr_chunk":
                onStderrRef.current?.(event.data);
                break;
              case "status_changed":
                terminalStore.setSessionMeta(threadId, { status: event.status });
                break;
              case "session_exited":
                terminalStore.setSessionMeta(threadId, {
                  status: "exited",
                  exitCode: event.exitCode,
                });
                onExitRef.current?.(event.exitCode);
                break;
            }
          },
          nextCols ?? cols,
          nextRows ?? rows,
        );

        terminalStore.upsertSession(payload.session);
        onReplayRef.current?.(payload.replay);
      },
      async close() {
        if (!threadId) {
          return;
        }

        await terminalClient.close(threadId);
        terminalStore.removeSession(threadId);
      },
    }),
    [cols, rows, threadId],
  );

  return {
    session: session as TerminalSessionDto | null,
    error,
    isConnecting,
    ...actions,
  };
}
