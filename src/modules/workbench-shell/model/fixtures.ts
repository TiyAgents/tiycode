import {
  ALargeSmall,
  Check,
  CircleX,
  Languages,
  LoaderCircle,
  MessageCircleMore,
  Monitor,
  Moon,
  Sun,
} from "lucide-react";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import type {
  GitChangeFile,
  GitHistoryItem,
  MockUserSession,
  PanelVisibilityState,
  ProjectOption,
  ProjectTreeItem,
  ThreadItem,
  ThreadStatus,
} from "@/modules/workbench-shell/model/types";

type WorkspaceSeed = {
  id: string;
  name: string;
  defaultOpen: boolean;
  threads: ReadonlyArray<ThreadItem>;
};

export const DRAWER_LIST_STACK_CLASS = "space-y-1";
export const DRAWER_LIST_ROW_CLASS = "w-full rounded-lg px-2.5 py-1.5 text-left text-[13px] leading-5 transition-colors";
export const DRAWER_LIST_LABEL_CLASS = "min-w-0 flex-1 truncate text-[13px] leading-5";
export const DRAWER_LIST_META_CLASS = "shrink-0 text-[11px] text-app-subtle";
export const DRAWER_ICON_ACTION_CLASS =
  "flex size-6 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground";
export const DRAWER_OVERFLOW_ACTION_CLASS =
  "absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-subtle opacity-0 transition-all duration-200 hover:bg-app-surface-hover hover:text-app-foreground group-hover:opacity-100";
export const DRAWER_SECTION_HEADER_CLASS = "flex items-center justify-between gap-3 px-1.5";

const THREAD_ITEMS = [
  { name: "创建 Tauri 2 React+TS+shadcn/ui 模块化脚手架", time: "59m", active: true, status: "running" },
  { name: "设计 Codex 风格工作台布局", time: "12m", active: false, status: "needs-reply" },
  { name: "配置打包与签名信息", time: "24h", active: false, status: "failed" },
  { name: "实现 Agent 会话与任务抽屉", time: "2d", active: false, status: "completed" },
] satisfies ReadonlyArray<ThreadItem>;

export const WORKSPACE_ITEMS: ReadonlyArray<WorkspaceSeed> = [
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
      { name: "梳理命令执行链路与窗口通信", time: "7m", active: false, status: "running" },
      { name: "对齐 macOS 标题栏与红绿灯细节", time: "2h", active: false, status: "completed" },
      { name: "补充命令失败后的 toast 反馈", time: "6h", active: false, status: "needs-reply" },
      { name: "整理窗口生命周期与能力清单", time: "3d", active: false, status: "completed" },
    ],
  },
  {
    id: "design-notes",
    name: "design-notes",
    defaultOpen: false,
    threads: [
      { name: "整理 Codex app 布局参考截图", time: "9d", active: false, status: "completed" },
      { name: "对比 VS Code 三栏工作台细节", time: "14d", active: false, status: "completed" },
      { name: "收敛 sidebar 的 hover 与选中态", time: "21d", active: false, status: "needs-reply" },
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
      { name: "验证流式响应在桌面端的渲染节奏", time: "18m", active: false, status: "running" },
      { name: "统一模型选择入口与会话状态存储", time: "4h", active: false, status: "needs-reply" },
      { name: "梳理 API 错误码映射与重试策略", time: "8h", active: false, status: "failed" },
      { name: "增加 tool call 执行中的状态提示", time: "11h", active: false, status: "running" },
      { name: "整理 token 用量与计费展示草图", time: "4d", active: false, status: "completed" },
    ],
  },
  {
    id: "release-prep",
    name: "release-prep",
    defaultOpen: false,
    threads: [
      { name: "补齐图标资源与打包元信息", time: "5d", active: false, status: "running" },
      { name: "核对 macOS 签名与 notarization 流程", time: "12d", active: false, status: "needs-reply" },
    ],
  },
  {
    id: "research-lab",
    name: "research-lab",
    defaultOpen: false,
    threads: [
      { name: "测试多 Agent 视图与分屏信息密度", time: "16d", active: false, status: "completed" },
      { name: "探索 prompt 历史版本回溯交互", time: "27d", active: false, status: "needs-reply" },
      { name: "记录 terminal 与右侧面板联动方案", time: "32d", active: false, status: "failed" },
    ],
  },
];

export const RECENT_PROJECTS: ReadonlyArray<ProjectOption> = [
  {
    id: "tiy-desktop",
    name: "tiy-desktop",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/tiy-desktop",
    lastOpenedLabel: "刚刚",
  },
  {
    id: "agent-runtime",
    name: "agent-runtime",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/agent-runtime",
    lastOpenedLabel: "12 分钟前",
  },
  {
    id: "openai-integration",
    name: "openai-integration",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/openai-integration",
    lastOpenedLabel: "今天",
  },
  {
    id: "design-system-lab",
    name: "design-system-lab",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/design-system-lab",
    lastOpenedLabel: "昨天",
  },
  {
    id: "prompt-evals",
    name: "prompt-evals",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/prompt-evals",
    lastOpenedLabel: "昨天",
  },
  {
    id: "internal-docs",
    name: "internal-docs",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/internal-docs",
    lastOpenedLabel: "2 天前",
  },
  {
    id: "billing-portal",
    name: "billing-portal",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/billing-portal",
    lastOpenedLabel: "2 天前",
  },
  {
    id: "desktop-shell",
    name: "desktop-shell",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/desktop-shell",
    lastOpenedLabel: "3 天前",
  },
  {
    id: "thread-orchestrator",
    name: "thread-orchestrator",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/thread-orchestrator",
    lastOpenedLabel: "4 天前",
  },
  {
    id: "markdown-export",
    name: "markdown-export",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/markdown-export",
    lastOpenedLabel: "上周",
  },
  {
    id: "release-checklist",
    name: "release-checklist",
    path: "/Users/jorben/Documents/Codespace/TiyAgents/release-checklist",
    lastOpenedLabel: "上周",
  },
];

export const MESSAGE_SECTIONS = [
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

export const TERMINAL_LINES = [
  "1:20:15 AM [vite] client hmr update /src/widgets/dashboard-overview/ui/dashboard-overview.tsx",
  "Info File src-tauri/tauri.conf.json changed. Rebuilding application...",
  "Running DevCommand (`cargo run --no-default-features --color always --`)",
  "Compiling tiy-agent v0.1.0 (/Users/jorben/Documents/Codespace/TiyAgents/tiy-desktop/src-tauri)",
  "Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.27s",
  "Running `target/debug/tiy-agent`",
] as const;

export const PROJECT_TREE_ITEMS: ReadonlyArray<ProjectTreeItem> = [
  { id: "dist", name: "dist", kind: "folder", icon: "folder", ignored: true },
  { id: "docs", name: "docs", kind: "folder", icon: "folder" },
  { id: "node-modules", name: "node_modules", kind: "folder", icon: "folder", ignored: true },
  { id: "public", name: "public", kind: "folder", icon: "folder" },
  { id: "src", name: "src", kind: "folder", icon: "folder" },
  { id: "src-tauri", name: "src-tauri", kind: "folder", icon: "folder" },
  { id: "gitignore", name: ".gitignore", kind: "file", icon: "git" },
  { id: "components-json", name: "components.json", kind: "file", icon: "json" },
  { id: "index-html", name: "index.html", kind: "file", icon: "html" },
  { id: "license", name: "LICENSE", kind: "file", icon: "license" },
  { id: "package-json", name: "package.json", kind: "file", icon: "json" },
  { id: "package-lock-json", name: "package-lock.json", kind: "file", icon: "json" },
  { id: "readme-md", name: "README.md", kind: "file", icon: "readme" },
  { id: "tsconfig-json", name: "tsconfig.json", kind: "file", icon: "json" },
  { id: "tsconfig-node-json", name: "tsconfig.node.json", kind: "file", icon: "json" },
  { id: "vite-config-ts", name: "vite.config.ts", kind: "file", icon: "ts" },
];

export const GIT_CHANGE_FILES: ReadonlyArray<GitChangeFile> = [
  {
    id: "dashboard-overview",
    path: "src/widgets/dashboard-overview/ui/dashboard-overview.tsx",
    status: "M",
    icon: "ts",
    summary: "+182 -94",
    initialStaged: true,
  },
  {
    id: "globals",
    path: "src/app/styles/globals.css",
    status: "M",
    icon: "css",
    summary: "+12 -4",
    initialStaged: false,
  },
  {
    id: "tauri-conf",
    path: "src-tauri/tauri.conf.json",
    status: "M",
    icon: "json",
    summary: "+3 -0",
    initialStaged: true,
  },
  {
    id: "providers",
    path: "src/app/providers/app-providers.tsx",
    status: "A",
    icon: "ts",
    summary: "+18 -0",
    initialStaged: false,
  },
  {
    id: "composer",
    path: "src/features/chat-input/ui/composer.tsx",
    status: "M",
    icon: "ts",
    summary: "+42 -19",
    initialStaged: true,
  },
  {
    id: "git-panel-styles",
    path: "src/widgets/dashboard-overview/ui/git-panel.css",
    status: "A",
    icon: "css",
    summary: "+64 -0",
    initialStaged: false,
  },
  {
    id: "tree-utils",
    path: "src/features/project-tree/lib/tree-utils.ts",
    status: "M",
    icon: "ts",
    summary: "+27 -8",
    initialStaged: false,
  },
  {
    id: "right-drawer-hook",
    path: "src/widgets/dashboard-overview/model/use-right-drawer-state.ts",
    status: "A",
    icon: "ts",
    summary: "+33 -0",
    initialStaged: true,
  },
  {
    id: "legacy-inspector",
    path: "src/widgets/inspector/ui/legacy-panel.tsx",
    status: "D",
    icon: "ts",
    summary: "+0 -118",
    initialStaged: false,
  },
  {
    id: "terminal-view",
    path: "src/features/terminal/ui/terminal-view.tsx",
    status: "M",
    icon: "ts",
    summary: "+16 -11",
    initialStaged: true,
  },
  {
    id: "readme-workbench",
    path: "README.md",
    status: "M",
    icon: "readme",
    summary: "+9 -3",
    initialStaged: false,
  },
  {
    id: "workspace-types",
    path: "src/entities/workspace/model/types.ts",
    status: "A",
    icon: "ts",
    summary: "+21 -0",
    initialStaged: false,
  },
  {
    id: "old-mock-data",
    path: "src/shared/mock/legacy-source-control.json",
    status: "D",
    icon: "json",
    summary: "+0 -57",
    initialStaged: true,
  },
  {
    id: "theme-tokens",
    path: "src/app/styles/theme-tokens.css",
    status: "M",
    icon: "css",
    summary: "+14 -6",
    initialStaged: false,
  },
];

export const GIT_HISTORY_ITEMS: ReadonlyArray<GitHistoryItem> = [
  {
    id: "history-1",
    subject: "refactor(git-panel): align source control layout with VS Code",
    hash: "a81c2d4",
    relativeTime: "2m ago",
    author: "Jorben Zhu",
    refs: ["HEAD", "main"],
  },
  {
    id: "history-2",
    subject: "feat(project-panel): add sticky root and filter controls",
    hash: "7f92ac1",
    relativeTime: "17m ago",
    author: "Jorben Zhu",
    refs: ["origin/main"],
  },
  {
    id: "history-3",
    subject: "style(workbench): simplify right drawer icon bar",
    hash: "d42b083",
    relativeTime: "39m ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-4",
    subject: "chore(layout): tighten panel spacing and terminal split",
    hash: "3bc0ad8",
    relativeTime: "1h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-5",
    subject: "feat(treeview): add file filter and sticky root header",
    hash: "28ae114",
    relativeTime: "2h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-6",
    subject: "style(project-panel): mute ignored files and folders",
    hash: "91fd55c",
    relativeTime: "3h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-7",
    subject: "refactor(drawer): split project and git tabs into icon bar",
    hash: "5f17ab2",
    relativeTime: "4h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-8",
    subject: "feat(source-control): add compact tracked file staging list",
    hash: "bc3401d",
    relativeTime: "5h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-9",
    subject: "style(network): tighten commit node spacing in history list",
    hash: "ec8a9f0",
    relativeTime: "7h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-10",
    subject: "fix(layout): pin network panel to bottom of right drawer",
    hash: "4db926f",
    relativeTime: "9h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-11",
    subject: "feat(workbench): simplify inspector drawer interactions",
    hash: "8c3e3aa",
    relativeTime: "12h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-12",
    subject: "refactor(sidebar): rebalance panel heights and overflow behavior",
    hash: "e71bf42",
    relativeTime: "15h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-13",
    subject: "chore(ui): polish icon actions across project and git panels",
    hash: "1af24de",
    relativeTime: "18h ago",
    author: "Jorben Zhu",
  },
  {
    id: "history-14",
    subject: "feat(shell): add terminal collapse state persistence",
    hash: "0d5bc7a",
    relativeTime: "1d ago",
    author: "Jorben Zhu",
    refs: ["origin/main"],
  },
  {
    id: "history-15",
    subject: "style(theme): refine surface contrast for light workspace mode",
    hash: "6a20c18",
    relativeTime: "1d ago",
    author: "Jorben Zhu",
  },
];

export const DEFAULT_TERMINAL_HEIGHT = 260;
export const MIN_TERMINAL_HEIGHT = 180;
export const MIN_WORKBENCH_HEIGHT = 240;
export const TOPBAR_HEIGHT = 36;

export const THEME_OPTIONS: Array<{
  label: string;
  value: ThemePreference;
  icon: typeof Monitor;
}> = [
  { label: "跟随系统", value: "system", icon: Monitor },
  { label: "明亮", value: "light", icon: Sun },
  { label: "暗黑", value: "dark", icon: Moon },
];

export const LANGUAGE_OPTIONS: Array<{
  label: string;
  value: LanguagePreference;
  icon: typeof Languages;
}> = [
  { label: "English", value: "en", icon: ALargeSmall },
  { label: "简体中文", value: "zh-CN", icon: Languages },
];

export const MENU_TRIGGER_CLASS =
  "flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-sm transition-colors hover:bg-app-surface-hover";
export const MENU_TRIGGER_ICON_CLASS = "size-4 shrink-0 text-app-subtle";
export const MENU_TRIGGER_LABEL_CLASS = "min-w-0 flex-1 truncate text-left text-sm";
export const MENU_SUBMENU_GROUP_CLASS = "ml-6 mt-1 border-l border-app-border/70 pl-3";
export const MENU_SUBMENU_OPTION_CLASS =
  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors";
export const MENU_SUBMENU_ICON_CLASS = "size-3.5 shrink-0 text-app-subtle/90";
export const MENU_SUBMENU_LABEL_CLASS = "flex-1 truncate";
export const MAC_USER_MENU_OFFSET = "ml-[74px]";
export const MAC_USER_MENU_POPOVER_OFFSET = "left-[74px]";
export const AUTH_STORAGE_KEY = "tiy-agent-auth-session";
export const PANEL_VISIBILITY_STORAGE_KEY = "tiy-agent-panel-visibility";
export const UPDATE_STATUS_DURATION = 2200;

export const MOCK_USER_SESSION: MockUserSession = {
  name: "Jorben Zhu",
  avatar: "JZ",
  email: "jorbenzhu@gmail.com",
};

export const DEFAULT_PANEL_VISIBILITY_STATE: PanelVisibilityState = {
  isSidebarOpen: true,
  isDrawerOpen: false,
  isTerminalCollapsed: true,
};

export const CONTEXT_WINDOW_INFO = {
  label: "Context Window",
  used: "12K",
  total: "128K",
  usageRatio: 12 / 128,
};

export const CONTEXT_WINDOW_USAGE_DETAIL = {
  usedPercent: Math.round(CONTEXT_WINDOW_INFO.usageRatio * 100),
  leftPercent: 100 - Math.round(CONTEXT_WINDOW_INFO.usageRatio * 100),
};

export const THREAD_STATUS_META: Record<
  ThreadStatus,
  {
    icon: typeof LoaderCircle;
    label: string;
    spin?: boolean;
  }
> = {
  running: {
    icon: LoaderCircle,
    label: "进行中",
    spin: true,
  },
  completed: {
    icon: Check,
    label: "已完成",
  },
  "needs-reply": {
    icon: MessageCircleMore,
    label: "待回应",
  },
  failed: {
    icon: CircleX,
    label: "错误或失败",
  },
};
