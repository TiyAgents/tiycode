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
  const containerRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const geometryRef = useRef({ cols: 120, rows: 36 });
  const [geometryVersion, setGeometryVersion] = useState(0);

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
    fitAddon.fit();

    geometryRef.current = { cols: terminal.cols, rows: terminal.rows };
    setGeometryVersion((current) => current + 1);

    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    return () => {
      terminalRef.current = null;
      fitAddonRef.current = null;
      terminal.dispose();
    };
  }, []);

  const terminalApi = useThreadTerminal({
    threadId,
    active,
    cols: geometryRef.current.cols,
    rows: geometryRef.current.rows,
    onReplay: (replay) => {
      const terminal = terminalRef.current;
      if (!terminal) {
        return;
      }

      terminal.reset();
      if (replay) {
        terminal.write(replay);
      }
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

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) {
      return;
    }

    const disposable = terminal.onData((data) => {
      void terminalApi.writeInput(data).catch(() => {});
    });

    return () => {
      disposable.dispose();
    };
  }, [terminalApi]);

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

    const syncGeometry = () => {
      const terminal = terminalRef.current;
      const fitAddon = fitAddonRef.current;
      if (!terminal || !fitAddon) {
        return;
      }

      fitAddon.fit();
      const next = { cols: terminal.cols, rows: terminal.rows };
      const changed =
        next.cols !== geometryRef.current.cols || next.rows !== geometryRef.current.rows;

      geometryRef.current = next;
      if (changed) {
        void terminalApi.resize(next.cols, next.rows).catch(() => {});
        setGeometryVersion((current) => current + 1);
      }
    };

    const resizeObserver = new ResizeObserver(() => {
      syncGeometry();
    });

    resizeObserver.observe(containerRef.current);
    syncGeometry();

    return () => resizeObserver.disconnect();
  }, [active, geometryVersion, terminalApi]);

  const placeholder = useMemo(() => {
    if (bootstrapError) {
      return bootstrapError;
    }

    if (!threadId) {
      return idleMessage ?? "当前没有可附着的线程。";
    }

    if (terminalApi.isConnecting) {
      return "Terminal 正在连接…";
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
        className={`h-full min-h-0 px-2 pb-2 transition-opacity duration-150 [&_.xterm]:h-full [&_.xterm-viewport]:overscroll-contain ${
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
