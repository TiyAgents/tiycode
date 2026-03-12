import {
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import {
  ALargeSmall,
  ArrowUp,
  Check,
  ChevronDown,
  Folder,
  FolderKanban,
  FolderOpen,
  Globe,
  GitBranch,
  FolderPlus,
  Languages,
  MoreHorizontal,
  Monitor,
  Moon,
  Palette,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Pencil,
  Plus,
  RefreshCw,
  Settings,
  Sparkles,
  Sun,
  TerminalSquare,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useLanguage, type LanguagePreference } from "@/app/providers/language-provider";
import { useTheme, type ThemePreference } from "@/app/providers/theme-provider";
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

const THEME_OPTIONS: Array<{
  label: string;
  value: ThemePreference;
  icon: typeof Monitor;
}> = [
  { label: "跟随系统", value: "system", icon: Monitor },
  { label: "明亮", value: "light", icon: Sun },
  { label: "暗黑", value: "dark", icon: Moon },
];
const LANGUAGE_OPTIONS: Array<{
  label: string;
  value: LanguagePreference;
  icon: typeof Languages;
}> = [
  { label: "English", value: "en", icon: ALargeSmall },
  { label: "简体中文", value: "zh-CN", icon: Languages },
];

const SETTINGS_TRIGGER_CLASS =
  "flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-app-surface-hover";
const SETTINGS_OPTION_CLASS =
  "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors";

export function DashboardOverview() {
  const { data, error, isLoading, refetch } = useSystemMetadata();
  const { theme, setTheme } = useTheme();
  const { language, setLanguage } = useLanguage();
  const [isSidebarOpen, setSidebarOpen] = useState(true);
  const [isDrawerOpen, setDrawerOpen] = useState(true);
  const [isTerminalCollapsed, setTerminalCollapsed] = useState(false);
  const [terminalHeight, setTerminalHeight] = useState(DEFAULT_TERMINAL_HEIGHT);
  const [terminalResize, setTerminalResize] = useState<{ startY: number; startHeight: number } | null>(null);
  const [composerValue, setComposerValue] = useState("");
  const [isSettingsMenuOpen, setSettingsMenuOpen] = useState(false);
  const [openSettingsSection, setOpenSettingsSection] = useState<"theme" | "language" | null>(null);
  const [openWorkspaces, setOpenWorkspaces] = useState<Record<string, boolean>>(() => Object.fromEntries(WORKSPACE_ITEMS.map((workspace) => [workspace.id, workspace.defaultOpen])));
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const settingsMenuRef = useRef<HTMLDivElement | null>(null);

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

  useEffect(() => {
    if (!isSettingsMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && settingsMenuRef.current?.contains(target)) {
        return;
      }

      setSettingsMenuOpen(false);
      setOpenSettingsSection(null);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isSettingsMenuOpen]);

  const handleWorkspaceToggle = (workspaceId: string) => {
    setOpenWorkspaces((current) => ({
      ...current,
      [workspaceId]: !current[workspaceId],
    }));
  };

  const handleSettingsMenuToggle = () => {
    const nextOpen = !isSettingsMenuOpen;
    setSettingsMenuOpen(nextOpen);
    setOpenSettingsSection(null);
  };

  const handleThemeSelect = (nextTheme: ThemePreference) => {
    setTheme(nextTheme);
    setOpenSettingsSection("theme");
  };

  const handleLanguageSelect = (nextLanguage: LanguagePreference) => {
    setLanguage(nextLanguage);
    setOpenSettingsSection("language");
  };

  const isMacOS = data?.platform === "macos" || (typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"));
  const selectedThemeOption = THEME_OPTIONS.find((option) => option.value === theme) ?? THEME_OPTIONS[0];
  const selectedThemeSummary = theme === "system" ? "跟随系统" : selectedThemeOption.label;
  const selectedLanguageOption = LANGUAGE_OPTIONS.find((option) => option.value === language) ?? LANGUAGE_OPTIONS[1];

  return (
    <main className="h-screen overflow-hidden bg-app-canvas text-app-foreground">
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
            "overflow-hidden bg-app-sidebar transition-[width,opacity,transform] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
            isSidebarOpen
              ? "w-[320px] border-r border-app-border opacity-100 translate-x-0"
              : "w-0 border-r-0 opacity-0 -translate-x-2 pointer-events-none",
          )}
        >
          <div className="flex h-full min-h-0 flex-col px-3 pb-3 pt-4">
            <div>
              <button
                type="button"
                className="group flex w-full items-center gap-2.5 rounded-xl border border-transparent bg-transparent px-3 py-2.5 text-left text-app-muted transition-[transform,box-shadow,background-color,border-color,color] duration-200 hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)] active:scale-[0.99]"
              >
                <Pencil className="size-4 shrink-0 text-app-subtle transition-colors duration-200 group-hover:text-app-foreground" />
                <span className="truncate text-sm font-medium">New thread</span>
              </button>
            </div>

            <div className="mt-6 flex items-center justify-between px-3">
              <span className="text-xs uppercase tracking-[0.14em] text-app-subtle">Threads</span>
              <FolderPlus className="size-3.5 text-app-subtle" />
            </div>

            <div className="mt-3 min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              <div className="mx-1 mb-3 h-px bg-app-border" />
              <div className="space-y-2">
                {WORKSPACE_ITEMS.map((workspace) => {
                  const isOpen = openWorkspaces[workspace.id];
                  const FolderIcon = isOpen ? FolderOpen : Folder;

                  return (
                    <div key={workspace.id} className="space-y-1">
                      <div className="group px-1">
                        <div className="relative">
                          <button
                            type="button"
                            className="flex w-full items-center gap-2 rounded-xl px-3 py-2 pr-11 text-left text-app-muted transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                            onClick={() => handleWorkspaceToggle(workspace.id)}
                          >
                            <FolderIcon className="size-4 shrink-0 text-app-muted" />
                            <span className="truncate text-sm">{workspace.name}</span>
                          </button>
                          <button
                            type="button"
                            aria-label="更多操作"
                            title="更多操作"
                            className="absolute right-2 top-1/2 flex size-7 -translate-y-1/2 items-center justify-center rounded-lg text-app-subtle opacity-0 transition-all duration-200 hover:bg-app-surface-hover hover:text-app-foreground group-hover:opacity-100"
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
                                    ? "border-app-border-strong bg-app-surface-active text-app-foreground"
                                    : "border-transparent bg-transparent text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                )}
                              >
                                <p className="truncate text-sm leading-5">{thread.name}</p>
                              </button>
                              <span
                                className={cn(
                                  "pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-xs text-app-subtle transition-opacity duration-200 group-hover:opacity-0",
                                  thread.active && "text-app-subtle",
                                )}
                              >
                                {thread.time}
                              </span>
                              <button
                                type="button"
                                aria-label="更多操作"
                                title="更多操作"
                                className={cn(
                                  "absolute right-2 top-1/2 flex size-7 -translate-y-1/2 items-center justify-center rounded-lg text-app-subtle opacity-0 transition-all duration-200 hover:bg-app-surface-hover hover:text-app-foreground group-hover:opacity-100",
                                  thread.active && "text-app-subtle",
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

            <div ref={settingsMenuRef} className="relative mt-3">
              {isSettingsMenuOpen ? (
                <div className="absolute bottom-full left-0 z-20 mb-2 w-[240px] rounded-2xl border border-app-border bg-app-menu p-1.5 shadow-[0_20px_48px_rgba(15,23,42,0.18)] dark:shadow-[0_20px_48px_rgba(0,0,0,0.42)]">
                  <button
                    type="button"
                    className={cn(SETTINGS_TRIGGER_CLASS, "text-app-foreground")}
                    aria-expanded={openSettingsSection === "theme"}
                    onClick={() => setOpenSettingsSection((current) => (current === "theme" ? null : "theme"))}
                  >
                    <Palette className="size-4 shrink-0 text-app-subtle" />
                    <span className="min-w-0 flex-1 truncate text-sm">主题</span>
                    <span className="shrink-0 text-xs text-app-subtle">{selectedThemeSummary}</span>
                  </button>

                  {openSettingsSection === "theme" ? (
                    <div className="mt-1 space-y-1">
                      {THEME_OPTIONS.map((option) => {
                        const OptionIcon = option.icon;
                        const isSelected = theme === option.value;

                        return (
                          <button
                            key={option.value}
                            type="button"
                            className={cn(
                              SETTINGS_OPTION_CLASS,
                              isSelected ? "bg-app-surface-active text-app-foreground" : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                            )}
                            onClick={() => handleThemeSelect(option.value)}
                          >
                            <OptionIcon className="size-4 shrink-0 text-app-subtle" />
                            <span className="flex-1 truncate">{option.label}</span>
                            {isSelected ? <Check className="size-4 shrink-0 text-app-foreground" /> : null}
                          </button>
                        );
                      })}
                    </div>
                  ) : null}

                  <button
                    type="button"
                    className={cn(SETTINGS_TRIGGER_CLASS, "mt-1 text-app-foreground")}
                    aria-expanded={openSettingsSection === "language"}
                    onClick={() => setOpenSettingsSection((current) => (current === "language" ? null : "language"))}
                  >
                    <Globe className="size-4 shrink-0 text-app-subtle" />
                    <span className="min-w-0 flex-1 truncate text-sm">语言</span>
                    <span className="shrink-0 text-xs text-app-subtle">{selectedLanguageOption.label}</span>
                  </button>

                  {openSettingsSection === "language" ? (
                    <div className="mt-1 space-y-1">
                      {LANGUAGE_OPTIONS.map((option) => {
                        const isSelected = language === option.value;
                        const OptionIcon = option.icon;

                        return (
                          <button
                            key={option.value}
                            type="button"
                            className={cn(
                              SETTINGS_OPTION_CLASS,
                              isSelected ? "bg-app-surface-active text-app-foreground" : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                            )}
                            onClick={() => handleLanguageSelect(option.value)}
                          >
                            <OptionIcon className="size-4 shrink-0 text-app-subtle" />
                            <span className="flex-1 truncate">{option.label}</span>
                            {isSelected ? <Check className="size-4 shrink-0 text-app-foreground" /> : null}
                          </button>
                        );
                      })}
                    </div>
                  ) : null}

                  <button
                    type="button"
                    className={cn(SETTINGS_TRIGGER_CLASS, "mt-1 text-app-foreground")}
                  >
                    <MoreHorizontal className="size-4 shrink-0 text-app-subtle" />
                    <span className="min-w-0 flex-1 truncate text-sm">更多设置</span>
                  </button>
                </div>
              ) : null}

              <button
                type="button"
                className={cn(SETTINGS_TRIGGER_CLASS, "text-app-muted hover:text-app-foreground")}
                aria-expanded={isSettingsMenuOpen}
                aria-haspopup="menu"
                onClick={handleSettingsMenuToggle}
              >
                <Settings className="size-4 shrink-0" />
                <span className="truncate text-sm">Settings</span>
                <ChevronDown className={cn("ml-auto size-4 shrink-0 text-app-subtle transition-transform duration-200", isSettingsMenuOpen && "rotate-180")} />
              </button>
            </div>
          </div>
        </aside>

        <section className="min-w-0 flex-1 min-h-0">
          <div className="flex h-full min-h-0 flex-col">
            <div className="flex min-h-0 flex-1 overflow-hidden">
              <section className="min-w-0 flex-1 min-h-0 bg-app-canvas">
                <div className="flex h-full min-h-0 flex-col">
                  <div className="flex h-14 items-center gap-4 border-b border-app-border px-5">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-semibold text-app-foreground">创建 Tauri 2 React+TS+shadcn/ui 模块化脚手架</p>
                      <p className="mt-0.5 text-xs text-app-subtle">当前工作区 · tiy-desktop</p>
                    </div>
                    <button
                      type="button"
                      className="ml-auto inline-flex items-center gap-1.5 text-xs text-app-subtle transition-colors hover:text-app-foreground"
                    >
                      <GitBranch className="size-3.5" />
                      <span>main</span>
                      <ChevronDown className="size-3.5" />
                    </button>
                  </div>

                  <div className="relative min-h-0 flex-1">
                    <div className="h-full overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                      <div className="mx-auto flex max-w-4xl flex-col gap-6 px-6 pb-28 pt-6">
                      <div className="rounded-2xl border border-app-border bg-app-surface p-5">
                        <div className="flex items-center gap-2 text-app-muted">
                          <Sparkles className="size-4 text-app-success" />
                          <span className="text-sm font-medium">Jorben，这版布局已经收敛到更接近 Codex app 的工作台结构。</span>
                        </div>
                        <p className="mt-3 text-sm leading-7 text-app-muted">
                          左右侧边栏现在都是真正隐藏而不是缩窄；顶部仅保留应用名，且中部区域继续承担拖动窗口的能力。
                        </p>
                      </div>

                      <div className="space-y-5 pb-6">
                        {MESSAGE_SECTIONS.map((section) => (
                          <div key={section.title} className="rounded-2xl border border-app-border bg-app-surface-muted p-5">
                            <h3 className="text-sm font-semibold text-app-foreground">{section.title}</h3>
                            <ul className="mt-4 space-y-3 text-sm text-app-muted">
                              {section.bullets.map((bullet) => (
                                <li key={bullet} className="flex items-start gap-3">
                                  <span className="mt-2 size-1.5 shrink-0 rounded-full bg-app-subtle" />
                                  <code className="rounded bg-app-code px-2 py-1 text-[13px] text-app-foreground">{bullet}</code>
                                </li>
                              ))}
                            </ul>
                          </div>
                        ))}
                      </div>

                      <Card className="border-app-border bg-app-surface text-app-foreground shadow-none">
                        <CardHeader>
                          <CardTitle className="text-base">Runtime Probe</CardTitle>
                          <CardDescription className="text-app-muted">确认桌面端命令桥接与应用元信息已经接通。</CardDescription>
                        </CardHeader>
                        <CardContent className="space-y-3 text-sm">
                          <div className="flex gap-3">
                            <Button className="gap-2" onClick={() => void refetch()}>
                              <RefreshCw className="size-4" />
                              Refresh runtime info
                            </Button>
                          </div>
                          {isLoading ? <p className="text-app-subtle">正在读取运行时信息...</p> : null}
                          {error ? <p className="text-app-danger">{error}</p> : null}
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

                    <div className="pointer-events-none absolute inset-x-0 bottom-0 h-14 bg-gradient-to-b from-transparent via-app-overlay via-55% to-app-canvas" />
                  </div>

                  <div className="shrink-0 px-6 pb-5 pt-3">
                    <div className="mx-auto max-w-4xl rounded-2xl border border-app-border bg-app-surface px-4 pb-3 pt-3 text-app-muted transition-colors focus-within:border-app-border-strong">
                      <textarea
                        ref={composerRef}
                        value={composerValue}
                        onChange={(event) => setComposerValue(event.target.value)}
                        rows={1}
                        placeholder="Ask for follow-up changes"
                        className="max-h-44 min-h-6 w-full resize-none overflow-y-auto bg-transparent text-sm leading-6 text-app-foreground outline-none placeholder:text-app-subtle [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
                      />
                      <div className="mt-3 flex items-end justify-between">
                        <button type="button" className="-ml-1 mt-1 rounded-lg p-2 text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground">
                          <Plus className="size-4" />
                        </button>
                        <button
                          type="button"
                          className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-[0_1px_2px_rgba(15,23,42,0.18)] transition-[transform,box-shadow,background-color] duration-200 hover:scale-[1.02] hover:bg-primary/90 hover:shadow-[0_4px_10px_rgba(15,23,42,0.18)] active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:scale-100 disabled:hover:shadow-[0_1px_2px_rgba(15,23,42,0.18)]"
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
                  "min-h-0 shrink-0 overflow-hidden bg-app-drawer transition-[width,opacity,transform] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
                  isDrawerOpen
                    ? "w-[360px] border-l border-app-border opacity-100 translate-x-0"
                    : "w-0 border-l-0 opacity-0 translate-x-2 pointer-events-none",
                )}
              >
                <div className="flex h-full min-h-0 flex-col">
                  <div className="sticky top-0 z-10 flex h-14 items-center border-b border-app-border px-4">
                    <div>
                      <p className="text-sm font-semibold text-app-foreground">Unstaged</p>
                      <p className="text-xs text-app-subtle">56 changes</p>
                    </div>
                  </div>

                  <div className="min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                    <div className="space-y-4 p-4">
                      <div className="rounded-2xl border border-app-border bg-app-surface p-4">
                        <p className="text-sm font-medium text-app-foreground">Large diff detected — showing one file at a time.</p>
                      </div>

                      {PANEL_FILES.map((file) => (
                        <div key={file.path} className="rounded-2xl border border-app-border bg-app-surface p-4">
                          <div className="flex items-start gap-3">
                            <div className="mt-1 flex size-9 shrink-0 items-center justify-center rounded-xl bg-app-surface-hover text-app-muted">
                              <FolderKanban className="size-4" />
                            </div>
                            <div className="min-w-0">
                              <p className="truncate text-sm text-app-foreground">{file.path}</p>
                              <div className="mt-2 flex items-center gap-2 text-xs">
                                <span className="text-app-success">+{file.add}</span>
                                <span className="text-app-danger">-{file.remove}</span>
                                <span className="size-1.5 rounded-full bg-app-info" />
                              </div>
                            </div>
                          </div>
                        </div>
                      ))}

                      <Card className="border-app-border bg-app-surface text-app-foreground shadow-none">
                        <CardHeader>
                          <CardTitle className="text-base">Tasks</CardTitle>
                          <CardDescription className="text-app-muted">0 out of 4 tasks completed</CardDescription>
                        </CardHeader>
                        <CardContent className="space-y-3">
                          {TASK_ITEMS.map((task, index) => (
                            <div key={task} className="flex items-start gap-3 text-sm text-app-muted">
                              <span className="mt-1 size-3 rounded-full border border-app-subtle" />
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
                "relative shrink-0 overflow-hidden bg-app-terminal transition-[height,opacity,border-color] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
                isTerminalCollapsed ? "border-t border-transparent opacity-0 pointer-events-none" : "border-t border-app-border opacity-100",
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
                <div className="mt-1.5 h-[2px] w-9 rounded-full bg-app-border opacity-50 transition-all duration-200 ease-out group-hover:w-14 group-hover:bg-app-border-strong group-hover:opacity-100" />
              </div>
              <div
                className={cn(
                  "flex h-full min-h-0 flex-col transition-opacity duration-200",
                  isTerminalCollapsed ? "opacity-0" : "opacity-100 delay-75",
                )}
              >
                <div className="flex h-[38px] shrink-0 items-center justify-between px-4 text-xs text-app-muted">
                  <div className="flex items-center gap-2">
                    <TerminalSquare className="size-3.5" />
                    <span>Terminal</span>
                  </div>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-7 text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground"
                    aria-label="收起 terminal"
                    title="收起 terminal"
                    onClick={() => setTerminalCollapsed(true)}
                  >
                    <ChevronDown className="size-4" />
                  </Button>
                </div>
                <div className="min-h-0 flex-1 overflow-auto overscroll-contain px-4 py-3 font-mono text-[12px] leading-6 text-app-muted [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                  {TERMINAL_LINES.map((line, index) => (
                    <div key={line} className="flex gap-3">
                      <span className={cn("text-app-subtle", index === 0 ? "text-app-info" : "")}>›</span>
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
  const panelToggleButtonClass =
    "relative size-7 text-app-subtle transition-[color,background-color] duration-200 hover:bg-app-surface-hover hover:text-app-foreground";

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
    <header className="fixed inset-x-0 top-0 z-30 h-9 border-b border-app-border bg-app-chrome backdrop-blur-xl">
      <div className="grid h-full grid-cols-[auto_1fr_auto] items-center gap-2 px-2.5">
        <div className={cn("relative z-10 shrink-0", isMacOS ? "w-[98px]" : "w-[96px]")} />

        <div
          className="relative z-10 flex h-full items-center justify-center"
          data-tauri-drag-region=""
          onMouseDown={handleTitleBarMouseDown}
        >
          <span className="select-none text-[13px] font-semibold tracking-[0.02em] text-app-foreground" data-tauri-drag-region="">Tiy Agent</span>
        </div>

        <div className="relative z-10 flex items-center justify-end gap-0.5">
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isSidebarOpen && "text-app-foreground")}
            aria-label={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            title={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            onClick={onToggleSidebar}
          >
            <PanelLeft className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, !isTerminalCollapsed && "text-app-foreground")}
            aria-label={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            title={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            onClick={onToggleTerminal}
          >
            <PanelBottom className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isDrawerOpen && "text-app-foreground")}
            aria-label={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            title={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            onClick={onToggleDrawer}
          >
            <PanelRight className="size-4" />
          </Button>
        </div>
      </div>
    </header>
  );
}

function InspectorItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-app-border bg-app-surface-muted p-3">
      <p className="text-xs uppercase tracking-[0.14em] text-app-subtle">{label}</p>
      <p className="mt-2 text-sm text-app-foreground">{value}</p>
    </div>
  );
}
