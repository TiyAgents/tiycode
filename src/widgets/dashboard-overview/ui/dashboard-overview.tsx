import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import {
  ArrowUp,
  ChevronDown,
  Folder,
  FolderKanban,
  FolderOpen,
  GitBranch,
  FolderPlus,
  MoreHorizontal,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Pencil,
  Plus,
  RefreshCw,
  Settings,
  Sparkles,
  TerminalSquare,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "@/shared/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/shared/ui/card";
import { cn } from "@/shared/lib/utils";
import { useSystemMetadata } from "@/features/system-info/model/use-system-metadata";

const THREAD_ITEMS = [
  { name: "创建 Tauri 2 React+TS+shadcn/ui 模块化脚手架", time: "59m", active: true },
  { name: "设计 Codex 风格工作台布局", time: "12m", active: false },
  { name: "配置打包与签名信息", time: "24h", active: false },
  { name: "实现 Agent 会话与任务抽屉", time: "2d", active: false },
] as const;

const WORKSPACE_ITEMS = [
  {
    id: "tiy-desktop",
    name: "tiy-desktop",
    defaultOpen: true,
    threads: THREAD_ITEMS,
  },
  {
    id: "agent-runtime",
    name: "agent-runtime",
    defaultOpen: true,
    threads: [
      { name: "梳理命令执行链路与窗口通信", time: "7m", active: false },
      { name: "对齐 macOS 标题栏与红绿灯细节", time: "2h", active: false },
      { name: "补充命令失败后的 toast 反馈", time: "6h", active: false },
      { name: "整理窗口生命周期与能力清单", time: "3d", active: false },
    ],
  },
  {
    id: "design-notes",
    name: "design-notes",
    defaultOpen: false,
    threads: [
      { name: "整理 Codex app 布局参考截图", time: "9d", active: false },
      { name: "对比 VS Code 三栏工作台细节", time: "14d", active: false },
      { name: "收敛 sidebar 的 hover 与选中态", time: "21d", active: false },
    ],
  },
  {
    id: "playground",
    name: "playground",
    defaultOpen: false,
    threads: [],
  },
  {
    id: "openai-integration",
    name: "openai-integration",
    defaultOpen: true,
    threads: [
      { name: "验证流式响应在桌面端的渲染节奏", time: "18m", active: false },
      { name: "统一模型选择入口与会话状态存储", time: "4h", active: false },
      { name: "梳理 API 错误码映射与重试策略", time: "8h", active: false },
      { name: "增加 tool call 执行中的状态提示", time: "11h", active: false },
      { name: "整理 token 用量与计费展示草图", time: "4d", active: false },
    ],
  },
  {
    id: "release-prep",
    name: "release-prep",
    defaultOpen: false,
    threads: [
      { name: "补齐图标资源与打包元信息", time: "5d", active: false },
      { name: "核对 macOS 签名与 notarization 流程", time: "12d", active: false },
    ],
  },
  {
    id: "research-lab",
    name: "research-lab",
    defaultOpen: false,
    threads: [
      { name: "测试多 Agent 视图与分屏信息密度", time: "16d", active: false },
      { name: "探索 prompt 历史版本回溯交互", time: "27d", active: false },
      { name: "记录 terminal 与右侧面板联动方案", time: "32d", active: false },
    ],
  },
] as const;

const MESSAGE_SECTIONS = [
  {
    title: "运行验证",
    bullets: [
      "npm run build:web 通过",
      "cargo check --manifest-path /Users/jorben/Documents/Codespace/TiyAgents/tiy-desktop/src-tauri/Cargo.toml 通过",
    ],
  },
  {
    title: "现在启动",
    bullets: [
      "cd /Users/jorben/Documents/Codespace/TiyAgents/tiy-desktop",
      "npm run dev:app",
    ],
  },
] as const;

const TASK_ITEMS = [
  "Fix sidebar hide/show behavior",
  "Simplify top bar and drag region",
  "Remove gradient and duplicate actions",
  "Validate build and summarize",
] as const;

const PANEL_FILES = [
  { path: "src/widgets/dashboard-overview/ui/dashboard-overview.tsx", add: 128, remove: 74 },
  { path: "src-tauri/tauri.conf.json", add: 3, remove: 0 },
  { path: "src/app/providers/app-providers.tsx", add: 1, remove: 0 },
] as const;

const TERMINAL_LINES = [
  "1:20:15 AM [vite] client hmr update /src/widgets/dashboard-overview/ui/dashboard-overview.tsx",
  "Info File src-tauri/tauri.conf.json changed. Rebuilding application...",
  "Running DevCommand (`cargo run --no-default-features --color always --`)",
  "Compiling tiy-agent v0.1.0 (/Users/jorben/Documents/Codespace/TiyAgents/tiy-desktop/src-tauri)",
  "Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.27s",
  "Running `target/debug/tiy-agent`",
] as const;

const DEFAULT_TERMINAL_HEIGHT = 260;
const MIN_TERMINAL_HEIGHT = 180;
const MIN_WORKBENCH_HEIGHT = 240;
const TOPBAR_HEIGHT = 36;

export function DashboardOverview() {
  const { data, error, isLoading, refetch } = useSystemMetadata();
  const [isSidebarOpen, setSidebarOpen] = useState(true);
  const [isDrawerOpen, setDrawerOpen] = useState(true);
  const [isTerminalCollapsed, setTerminalCollapsed] = useState(false);
  const [terminalHeight, setTerminalHeight] = useState(DEFAULT_TERMINAL_HEIGHT);
  const [terminalResize, setTerminalResize] = useState<{ startY: number; startHeight: number } | null>(null);
  const [composerValue, setComposerValue] = useState("");
  const [openWorkspaces, setOpenWorkspaces] = useState<Record<string, boolean>>(() => Object.fromEntries(WORKSPACE_ITEMS.map((workspace) => [workspace.id, workspace.defaultOpen])));
  const composerRef = useRef<HTMLTextAreaElement | null>(null);

  const getMaxTerminalHeight = () => {
    if (typeof window === "undefined") {
      return DEFAULT_TERMINAL_HEIGHT;
    }

    return Math.max(MIN_TERMINAL_HEIGHT, window.innerHeight - TOPBAR_HEIGHT - MIN_WORKBENCH_HEIGHT);
  };

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const syncTerminalHeight = () => {
      setTerminalHeight((current) => Math.min(current, getMaxTerminalHeight()));
    };

    syncTerminalHeight();
    window.addEventListener("resize", syncTerminalHeight);

    return () => window.removeEventListener("resize", syncTerminalHeight);
  }, []);

  useEffect(() => {
    if (!terminalResize || typeof window === "undefined") {
      return;
    }

    const handleMouseMove = (event: MouseEvent) => {
      const deltaY = terminalResize.startY - event.clientY;
      const nextHeight = terminalResize.startHeight + deltaY;
      const clampedHeight = Math.min(getMaxTerminalHeight(), Math.max(MIN_TERMINAL_HEIGHT, nextHeight));

      setTerminalHeight(clampedHeight);
    };

    const handleMouseUp = () => {
      setTerminalResize(null);
    };

    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;

    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [terminalResize]);

  const handleTerminalResizeStart = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }

    event.preventDefault();
    setTerminalResize({ startY: event.clientY, startHeight: terminalHeight });
  };

  useEffect(() => {
    const textarea = composerRef.current;
    if (!textarea) {
      return;
    }

    textarea.style.height = "0px";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 176)}px`;
  }, [composerValue]);

  const handleWorkspaceToggle = (workspaceId: string) => {
    setOpenWorkspaces((current) => ({
      ...current,
      [workspaceId]: !current[workspaceId],
    }));
  };

  const isMacOS = data?.platform === "macos" || (typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"));

  return (
    <main className="h-screen overflow-hidden bg-[#0b0d12] text-zinc-100">
      <WorkbenchTopBar
        isMacOS={isMacOS}
        isSidebarOpen={isSidebarOpen}
        isDrawerOpen={isDrawerOpen}
        isTerminalCollapsed={isTerminalCollapsed}
        onToggleSidebar={() => setSidebarOpen((current) => !current)}
        onToggleDrawer={() => setDrawerOpen((current) => !current)}
        onToggleTerminal={() => setTerminalCollapsed((current) => !current)}
      />

      <div className="flex h-full min-h-0 pt-9">
        <aside
          className={cn(
            "overflow-hidden bg-[#101319] transition-[width,opacity,transform] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
            isSidebarOpen
              ? "w-[320px] border-r border-white/6 opacity-100 translate-x-0"
              : "w-0 border-r-0 opacity-0 -translate-x-2 pointer-events-none",
          )}
        >
          <div className="flex h-full min-h-0 flex-col px-3 pb-3 pt-4">
            <div>
              <button
                type="button"
                className="group flex w-full items-center gap-2.5 rounded-xl border border-transparent bg-transparent px-3 py-2.5 text-left text-zinc-300 transition-[transform,box-shadow,background-color,border-color,color] duration-200 hover:border-white/10 hover:bg-white/[0.07] hover:text-zinc-100 hover:shadow-[0_4px_14px_rgba(0,0,0,0.10)] active:scale-[0.99]"
              >
                <Pencil className="size-4 shrink-0 text-zinc-400 transition-colors duration-200 group-hover:text-zinc-200" />
                <span className="truncate text-sm font-medium">New thread</span>
              </button>
            </div>

            <div className="mt-6 flex items-center justify-between px-3">
              <span className="text-xs uppercase tracking-[0.14em] text-zinc-500">Threads</span>
              <FolderPlus className="size-3.5 text-zinc-500" />
            </div>

            <div className="mt-3 min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              <div className="space-y-2">
                {WORKSPACE_ITEMS.map((workspace) => {
                  const isOpen = openWorkspaces[workspace.id];
                  const FolderIcon = isOpen ? FolderOpen : Folder;

                  return (
                    <div key={workspace.id} className="space-y-1">
                      <div className="group px-1">
                        <div className="mx-auto mb-2 h-px w-[98%] bg-white/5" />
                        <div className="relative">
                          <button
                            type="button"
                            className="flex w-full items-center gap-2 rounded-xl px-3 py-2 pr-11 text-left text-zinc-300 transition-colors hover:bg-white/6"
                            onClick={() => handleWorkspaceToggle(workspace.id)}
                          >
                            <FolderIcon className="size-4 shrink-0 text-zinc-300" />
                            <span className="truncate text-sm">{workspace.name}</span>
                          </button>
                          <button
                            type="button"
                            aria-label="更多操作"
                            title="更多操作"
                            className="absolute right-2 top-1/2 flex size-7 -translate-y-1/2 items-center justify-center rounded-lg text-zinc-500 opacity-0 transition-all duration-200 hover:bg-white/6 hover:text-zinc-200 group-hover:opacity-100"
                          >
                            <MoreHorizontal className="size-4" />
                          </button>
                        </div>
                      </div>

                      {isOpen && workspace.threads.length > 0 ? (
                        <div className="space-y-1 pl-3">
                          {workspace.threads.map((thread) => (
                            <div key={`${workspace.id}-${thread.name}`} className="group relative">
                              <button
                                type="button"
                                className={cn(
                                  "w-full rounded-xl border px-3 py-2.5 pr-12 text-left transition-colors",
                                  thread.active
                                    ? "border-white/8 bg-white/[0.05] text-zinc-100"
                                    : "border-transparent bg-transparent text-zinc-300 hover:bg-white/6",
                                )}
                              >
                                <p className="truncate text-sm leading-5">{thread.name}</p>
                              </button>
                              <span
                                className={cn(
                                  "pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-xs text-zinc-500 transition-opacity duration-200 group-hover:opacity-0",
                                  thread.active && "text-zinc-500",
                                )}
                              >
                                {thread.time}
                              </span>
                              <button
                                type="button"
                                aria-label="更多操作"
                                title="更多操作"
                                className={cn(
                                  "absolute right-2 top-1/2 flex size-7 -translate-y-1/2 items-center justify-center rounded-lg text-zinc-500 opacity-0 transition-all duration-200 hover:bg-white/6 hover:text-zinc-200 group-hover:opacity-100",
                                  thread.active && "text-zinc-500",
                                )}
                              >
                                <MoreHorizontal className="size-4" />
                              </button>
                            </div>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            </div>

            <button
              type="button"
              className="mt-3 flex items-center gap-3 rounded-xl px-3 py-2.5 text-left text-zinc-300 transition-colors hover:bg-white/6"
            >
              <Settings className="size-4 shrink-0" />
              <span className="truncate text-sm">Settings</span>
            </button>
          </div>
        </aside>

        <section className="min-w-0 flex-1 min-h-0">
          <div className="flex h-full min-h-0 flex-col">
            <div className="flex min-h-0 flex-1 overflow-hidden">
              <section className="min-w-0 flex-1 min-h-0 bg-[#0b0d12]">
                <div className="flex h-full min-h-0 flex-col">
                  <div className="flex h-14 items-center gap-4 border-b border-white/6 px-5">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-semibold text-zinc-100">创建 Tauri 2 React+TS+shadcn/ui 模块化脚手架</p>
                      <p className="mt-0.5 text-xs text-zinc-500">当前工作区 · tiy-desktop</p>
                    </div>
                    <button
                      type="button"
                      className="ml-auto inline-flex items-center gap-1.5 text-xs text-zinc-500 transition-colors hover:text-zinc-300"
                    >
                      <GitBranch className="size-3.5" />
                      <span>main</span>
                      <ChevronDown className="size-3.5" />
                    </button>
                  </div>

                  <div className="relative min-h-0 flex-1">
                    <div className="h-full overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                      <div className="mx-auto flex max-w-4xl flex-col gap-6 px-6 pb-28 pt-6">
                      <div className="rounded-2xl border border-white/8 bg-white/[0.03] p-5">
                        <div className="flex items-center gap-2 text-zinc-300">
                          <Sparkles className="size-4 text-emerald-400" />
                          <span className="text-sm font-medium">Jorben，这版布局已经收敛到更接近 Codex app 的工作台结构。</span>
                        </div>
                        <p className="mt-3 text-sm leading-7 text-zinc-400">
                          左右侧边栏现在都是真正隐藏而不是缩窄；顶部仅保留应用名，且中部区域继续承担拖动窗口的能力。
                        </p>
                      </div>

                      <div className="space-y-5 pb-6">
                        {MESSAGE_SECTIONS.map((section) => (
                          <div key={section.title} className="rounded-2xl border border-white/6 bg-white/[0.02] p-5">
                            <h3 className="text-sm font-semibold text-zinc-100">{section.title}</h3>
                            <ul className="mt-4 space-y-3 text-sm text-zinc-300">
                              {section.bullets.map((bullet) => (
                                <li key={bullet} className="flex items-start gap-3">
                                  <span className="mt-2 size-1.5 shrink-0 rounded-full bg-zinc-500" />
                                  <code className="rounded bg-black/30 px-2 py-1 text-[13px] text-zinc-200">{bullet}</code>
                                </li>
                              ))}
                            </ul>
                          </div>
                        ))}
                      </div>

                      <Card className="border-white/8 bg-white/[0.03] text-zinc-100 shadow-none">
                        <CardHeader>
                          <CardTitle className="text-base">Runtime Probe</CardTitle>
                          <CardDescription className="text-zinc-400">确认桌面端命令桥接与应用元信息已经接通。</CardDescription>
                        </CardHeader>
                        <CardContent className="space-y-3 text-sm">
                          <div className="flex gap-3">
                            <Button className="gap-2" onClick={() => void refetch()}>
                              <RefreshCw className="size-4" />
                              Refresh runtime info
                            </Button>
                          </div>
                          {isLoading ? <p className="text-zinc-500">正在读取运行时信息...</p> : null}
                          {error ? <p className="text-red-400">{error}</p> : null}
                          {data ? (
                            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                              <InspectorItem label="应用名" value={data.appName} />
                              <InspectorItem label="版本" value={data.version} />
                              <InspectorItem label="平台" value={data.platform} />
                              <InspectorItem label="架构" value={data.arch} />
                              <InspectorItem label="运行时" value={data.runtime} />
                            </div>
                          ) : null}
                        </CardContent>
                      </Card>
                    </div>
                    </div>

                    <div className="pointer-events-none absolute inset-x-0 bottom-0 h-14 bg-gradient-to-b from-transparent via-[#0b0d12]/72 via-55% to-[#0b0d12]" />
                  </div>

                  <div className="shrink-0 px-6 pb-5 pt-3">
                    <div className="mx-auto max-w-4xl rounded-2xl border border-white/8 bg-[#11141b] px-4 pb-3 pt-3 text-zinc-400 transition-colors focus-within:border-white/12">
                      <textarea
                        ref={composerRef}
                        value={composerValue}
                        onChange={(event) => setComposerValue(event.target.value)}
                        rows={1}
                        placeholder="Ask for follow-up changes"
                        className="max-h-44 min-h-6 w-full resize-none overflow-y-auto bg-transparent text-sm leading-6 text-zinc-100 outline-none placeholder:text-zinc-500 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
                      />
                      <div className="mt-3 flex items-end justify-between">
                        <button type="button" className="-ml-1 mt-1 rounded-lg p-2 text-zinc-500 transition-colors hover:bg-white/6 hover:text-zinc-100">
                          <Plus className="size-4" />
                        </button>
                        <button
                          type="button"
                          className="flex size-8 items-center justify-center rounded-full bg-white/95 text-black shadow-[0_1px_2px_rgba(0,0,0,0.14)] transition-[transform,box-shadow,background-color] duration-200 hover:scale-[1.02] hover:bg-white hover:shadow-[0_4px_10px_rgba(0,0,0,0.12)] active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:scale-100 disabled:hover:shadow-[0_1px_2px_rgba(0,0,0,0.14)]"
                          disabled={!composerValue.trim()}
                        >
                          <ArrowUp className="size-3.5" />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </section>

              <aside
                className={cn(
                  "min-h-0 shrink-0 overflow-hidden bg-[#0d1015] transition-[width,opacity,transform] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
                  isDrawerOpen
                    ? "w-[360px] border-l border-white/6 opacity-100 translate-x-0"
                    : "w-0 border-l-0 opacity-0 translate-x-2 pointer-events-none",
                )}
              >
                <div className="flex h-full min-h-0 flex-col">
                  <div className="sticky top-0 z-10 flex h-14 items-center border-b border-white/6 px-4">
                    <div>
                      <p className="text-sm font-semibold text-zinc-100">Unstaged</p>
                      <p className="text-xs text-zinc-500">56 changes</p>
                    </div>
                  </div>

                  <div className="min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                    <div className="space-y-4 p-4">
                      <div className="rounded-2xl border border-white/8 bg-white/[0.03] p-4">
                        <p className="text-sm font-medium text-zinc-100">Large diff detected — showing one file at a time.</p>
                      </div>

                      {PANEL_FILES.map((file) => (
                        <div key={file.path} className="rounded-2xl border border-white/8 bg-white/[0.03] p-4">
                          <div className="flex items-start gap-3">
                            <div className="mt-1 flex size-9 shrink-0 items-center justify-center rounded-xl bg-white/6 text-zinc-300">
                              <FolderKanban className="size-4" />
                            </div>
                            <div className="min-w-0">
                              <p className="truncate text-sm text-zinc-100">{file.path}</p>
                              <div className="mt-2 flex items-center gap-2 text-xs">
                                <span className="text-emerald-400">+{file.add}</span>
                                <span className="text-rose-400">-{file.remove}</span>
                                <span className="size-1.5 rounded-full bg-sky-400" />
                              </div>
                            </div>
                          </div>
                        </div>
                      ))}

                      <Card className="border-white/8 bg-white/[0.03] text-zinc-100 shadow-none">
                        <CardHeader>
                          <CardTitle className="text-base">Tasks</CardTitle>
                          <CardDescription className="text-zinc-400">0 out of 4 tasks completed</CardDescription>
                        </CardHeader>
                        <CardContent className="space-y-3">
                          {TASK_ITEMS.map((task, index) => (
                            <div key={task} className="flex items-start gap-3 text-sm text-zinc-300">
                              <span className="mt-1 size-3 rounded-full border border-zinc-500" />
                              <span>{index + 1}. {task}</span>
                            </div>
                          ))}
                        </CardContent>
                      </Card>
                    </div>
                  </div>
                </div>
              </aside>
            </div>
            <section
              className={cn(
                "relative shrink-0 overflow-hidden bg-[#090b10] transition-[height,opacity,border-color] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
                isTerminalCollapsed ? "border-t border-transparent opacity-0 pointer-events-none" : "border-t border-white/6 opacity-100",
              )}
              style={{ height: isTerminalCollapsed ? 0 : terminalHeight }}
            >
              <div
                className={cn(
                  "group absolute inset-x-0 top-0 z-10 flex h-4 -translate-y-1/2 items-start justify-center transition-opacity duration-200",
                  isTerminalCollapsed ? "opacity-0" : "cursor-row-resize opacity-100",
                )}
                role="presentation"
                onMouseDown={handleTerminalResizeStart}
              >
                <div className="mt-1.5 h-[2px] w-9 rounded-full bg-white/7 opacity-50 transition-all duration-200 ease-out group-hover:w-14 group-hover:bg-white/20 group-hover:opacity-100" />
              </div>
              <div
                className={cn(
                  "flex h-full min-h-0 flex-col transition-opacity duration-200",
                  isTerminalCollapsed ? "opacity-0" : "opacity-100 delay-75",
                )}
              >
                <div className="flex h-[38px] shrink-0 items-center justify-between px-4 text-xs text-zinc-400">
                  <div className="flex items-center gap-2">
                    <TerminalSquare className="size-3.5" />
                    <span>Terminal</span>
                  </div>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-7 text-zinc-400 hover:bg-white/6 hover:text-zinc-100"
                    aria-label="收起 terminal"
                    title="收起 terminal"
                    onClick={() => setTerminalCollapsed(true)}
                  >
                    <ChevronDown className="size-4" />
                  </Button>
                </div>
                <div className="min-h-0 flex-1 overflow-auto overscroll-contain px-4 py-3 font-mono text-[12px] leading-6 text-zinc-300 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                  {TERMINAL_LINES.map((line, index) => (
                    <div key={line} className="flex gap-3">
                      <span className={cn("text-zinc-500", index === 0 ? "text-sky-400" : "")}>›</span>
                      <span>{line}</span>
                    </div>
                  ))}
                </div>
              </div>
            </section>
          </div>
        </section>
      </div>
    </main>
  );
}

function WorkbenchTopBar({
  isMacOS,
  isSidebarOpen,
  isDrawerOpen,
  isTerminalCollapsed,
  onToggleSidebar,
  onToggleDrawer,
  onToggleTerminal,
}: {
  isMacOS: boolean;
  isSidebarOpen: boolean;
  isDrawerOpen: boolean;
  isTerminalCollapsed: boolean;
  onToggleSidebar: () => void;
  onToggleDrawer: () => void;
  onToggleTerminal: () => void;
}) {
  const handleTitleBarMouseDown = async (event: ReactMouseEvent<HTMLElement>) => {
    if (event.button !== 0) {
      return;
    }

    const target = event.target as HTMLElement | null;
    if (target?.closest("button, a, input, textarea, select, [role='button']")) {
      return;
    }

    try {
      await getCurrentWindow().startDragging();
    } catch {
      // no-op
    }
  };

  return (
    <header className="fixed inset-x-0 top-0 z-30 h-9 border-b border-white/6 bg-[#0b0d12]/95 backdrop-blur-xl">
      <div className="grid h-full grid-cols-[auto_1fr_auto] items-center gap-2 px-3">
        <div className={cn("relative z-10 flex items-center gap-2", isMacOS ? "pl-[74px]" : "") }>
          <Button
            size="icon"
            variant="ghost"
            className="size-7 text-zinc-300 hover:bg-white/6 hover:text-zinc-100"
            aria-label={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            title={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            onClick={onToggleSidebar}
          >
            {isSidebarOpen ? <PanelLeftClose className="size-3.5" /> : <PanelLeftOpen className="size-3.5" />}
          </Button>
        </div>

        <div
          className="relative z-10 flex h-full items-center justify-center"
          data-tauri-drag-region=""
          onMouseDown={handleTitleBarMouseDown}
        >
          <span className="select-none text-[13px] font-semibold tracking-[0.02em] text-zinc-100" data-tauri-drag-region="">Tiy Agent</span>
        </div>

        <div className="relative z-10 flex items-center justify-end gap-2">
          <Button
            size="icon"
            variant="ghost"
            className={cn(
              "size-7 text-zinc-300 hover:bg-white/6 hover:text-zinc-100",
              !isTerminalCollapsed && "bg-white/6 text-zinc-100",
            )}
            aria-label={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            title={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            onClick={onToggleTerminal}
          >
            <TerminalSquare className="size-3.5" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-7 text-zinc-300 hover:bg-white/6 hover:text-zinc-100"
            aria-label={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            title={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            onClick={onToggleDrawer}
          >
            {isDrawerOpen ? <PanelRightClose className="size-3.5" /> : <PanelRightOpen className="size-3.5" />}
          </Button>
        </div>
      </div>
    </header>
  );
}

function InspectorItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-white/8 bg-black/20 p-3">
      <p className="text-xs uppercase tracking-[0.14em] text-zinc-500">{label}</p>
      <p className="mt-2 text-sm text-zinc-100">{value}</p>
    </div>
  );
}
