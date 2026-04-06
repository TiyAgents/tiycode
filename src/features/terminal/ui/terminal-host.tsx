import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "xterm";
import "xterm/css/xterm.css";
import { useT } from "@/i18n";
import { useThreadTerminal } from "@/features/terminal/model/use-thread-terminal";

type TerminalHostProps = {
  threadId: string | null;
  active: boolean;
  bootstrapError?: string | null;
  idleMessage?: string;
};

export type TerminalHostHandle = {
  restart: () => Promise<void>;
  close: () => Promise<void>;
};

export const TerminalHost = forwardRef<TerminalHostHandle, TerminalHostProps>(function TerminalHost(
  { threadId, active, bootstrapError, idleMessage },
  ref,
) {
  const t = useT();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const geometryRef = useRef({ cols: 120, rows: 36 });
  const syncGeometryRef = useRef<(() => void) | null>(null);
  const writeInputRef = useRef<(data: string) => Promise<void>>(async () => {});
  const isReplayingRef = useRef(false);
  const [geometry, setGeometry] = useState({ cols: 120, rows: 36 });
  const [isTerminalReady, setTerminalReady] = useState(false);
  const [isGeometrySettled, setGeometrySettled] = useState(false);

  useEffect(() => {
    if (!containerRef.current || terminalRef.current) {
      return;
    }

    const fitAddon = new FitAddon();
    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: false,
      allowTransparency: true,
      fontFamily: '"SFMono-Regular", "JetBrains Mono", "Menlo", monospace',
      fontSize: 12,
      lineHeight: 1.35,
      scrollback: 5000,
      theme: {
        background: "#0b1220",
        foreground: "#d8e1f3",
        cursor: "#7dd3fc",
        selectionBackground: "rgba(125, 211, 252, 0.24)",
      },
    });

    terminal.loadAddon(fitAddon);
    terminal.open(containerRef.current);

    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;
    syncGeometryRef.current = () => {
      fitAddon.fit();
      const next = { cols: terminal.cols, rows: terminal.rows };
      geometryRef.current = next;
      setGeometry((current) =>
        current.cols === next.cols && current.rows === next.rows ? current : next,
      );
    };

    const frameId = window.requestAnimationFrame(() => {
      syncGeometryRef.current?.();
      setTerminalReady(true);
    });

    return () => {
      window.cancelAnimationFrame(frameId);
      syncGeometryRef.current = null;
      terminalRef.current = null;
      fitAddonRef.current = null;
      terminal.dispose();
    };
  }, []);

  useEffect(() => {
    if (!active || !isTerminalReady) {
      setGeometrySettled(false);
      return;
    }

    setGeometrySettled(false);
    const timeoutId = window.setTimeout(() => {
      setGeometrySettled(true);
    }, 120);

    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [active, geometry.cols, geometry.rows, isTerminalReady]);

  const terminalApi = useThreadTerminal({
    threadId,
    active: active && isTerminalReady && isGeometrySettled,
    cols: geometry.cols,
    rows: geometry.rows,
    onReplay: (replay) => {
      const terminal = terminalRef.current;
      if (!terminal) {
        return;
      }

      isReplayingRef.current = true;
      terminal.reset();
      if (replay) {
        terminal.write(replay, () => {
          isReplayingRef.current = false;
          syncGeometryRef.current?.();
        });
        return;
      }

      isReplayingRef.current = false;
    },
    onStdout: (data) => {
      terminalRef.current?.write(data);
    },
    onStderr: (data) => {
      terminalRef.current?.write(data);
    },
    onExit: (exitCode) => {
      terminalRef.current?.writeln(
        `\r\n[terminal exited${exitCode === null ? "" : ` with code ${exitCode}`}]`,
      );
    },
  });
  writeInputRef.current = (data: string) => terminalApi.writeInput(data);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) {
      return;
    }

    const disposable = terminal.onData((data) => {
      if (isReplayingRef.current) {
        return;
      }
      void writeInputRef.current(data).catch(() => {});
    });

    return () => {
      disposable.dispose();
    };
  }, []);

  useEffect(() => {
    if (threadId) {
      return;
    }

    terminalRef.current?.reset();
  }, [threadId]);

  useEffect(() => {
    if (!active || !containerRef.current || !terminalRef.current || !fitAddonRef.current) {
      return;
    }

    const resizeObserver = new ResizeObserver(() => {
      syncGeometryRef.current?.();
    });

    resizeObserver.observe(containerRef.current);
    syncGeometryRef.current?.();

    return () => resizeObserver.disconnect();
  }, [active, isTerminalReady]);

  const placeholder = useMemo(() => {
    if (bootstrapError) {
      return bootstrapError;
    }

    if (!threadId) {
      return idleMessage ?? t("terminal.noAttachableThread");
    }

    if (terminalApi.isConnecting) {
      return t("terminal.connecting");
    }

    return terminalApi.error ?? null;
  }, [bootstrapError, idleMessage, terminalApi.error, terminalApi.isConnecting, threadId]);
  const isTerminalCanvasVisible = placeholder === null;

  useImperativeHandle(
    ref,
    () => ({
      restart: async () => {
        await terminalApi.restart(geometryRef.current.cols, geometryRef.current.rows);
      },
      close: async () => {
        await terminalApi.close();
        terminalRef.current?.reset();
      },
    }),
    [terminalApi],
  );

  return (
    <div className="relative h-full min-h-0">
      <div
        ref={containerRef}
        aria-hidden={!isTerminalCanvasVisible}
        className={`h-full min-h-0 transition-opacity duration-150 [&_.xterm]:h-full [&_.xterm-viewport]:overscroll-contain ${
          isTerminalCanvasVisible ? "opacity-100" : "pointer-events-none opacity-0"
        }`}
      />
      {placeholder ? (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center px-6 text-center text-xs text-app-muted">
          <div className="max-w-md">
            {placeholder}
          </div>
        </div>
      ) : null}
    </div>
  );
});
