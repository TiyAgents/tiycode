import {
  useDeferredValue,
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import {
  ALargeSmall,
  ArrowDownToLine,
  ArrowUpFromLine,
  ArrowUp,
  Bot,
  BookOpen,
  Boxes,
  Braces,
  Check,
  CircleX,
  CircleUserRound,
  ChevronDown,
  Code2,
  Download,
  Folder,
  FolderOpen,
  Globe,
  GitBranch,
  FolderPlus,
  Languages,
  LoaderCircle,
  LogIn,
  LogOut,
  MessageCircleMore,
  MessageSquarePlus,
  MoreHorizontal,
  Monitor,
  Moon,
  Palette,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Plus,
  RefreshCw,
  Sparkles,
  Sun,
  TerminalSquare,
  Undo2,
  Minus,
  Square,
  Copy,
  X,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import {
  WorkbenchSettingsOverlay,
} from "@/features/settings/ui/workbench-settings-overlay";
import {
  type SettingsCategory,
  useWorkbenchSettings,
} from "@/features/settings/model/use-workbench-settings";
import { useLanguage, type LanguagePreference } from "@/app/providers/language-provider";
import { useTheme, type ThemePreference } from "@/app/providers/theme-provider";
import { Button } from "@/shared/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/shared/ui/card";
import { Input } from "@/shared/ui/input";
import { cn } from "@/shared/lib/utils";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import { useSystemMetadata } from "@/features/system-info/model/use-system-metadata";

type ThreadStatus = "running" | "completed" | "needs-reply" | "failed";

type ThreadItem = {
  name: string;
  time: string;
  active: boolean;
  status: ThreadStatus;
};

type WorkspaceThreadItem = ThreadItem & {
  id: string;
};

type WorkspaceItem = {
  id: string;
  name: string;
  defaultOpen: boolean;
  threads: Array<WorkspaceThreadItem>;
  path?: string;
};

type ProjectOption = {
  id: string;
  name: string;
  path: string;
  lastOpenedLabel: string;
};

type DrawerPanel = "project" | "git";

type ProjectTreeItem = {
  id: string;
  name: string;
  kind: "folder" | "file";
  icon: "folder" | "git" | "json" | "html" | "css" | "license" | "readme" | "ts";
  ignored?: boolean;
};

type GitChangeFile = {
  id: string;
  path: string;
  status: "M" | "A" | "D";
  icon: ProjectTreeItem["icon"];
  summary: string;
  initialStaged: boolean;
};

type GitHistoryItem = {
  id: string;
  subject: string;
  hash: string;
  relativeTime: string;
  author: string;
  refs?: ReadonlyArray<string>;
};

type GitDiffLine = {
  kind: "context" | "add" | "remove";
  oldNumber: number | null;
  newNumber: number | null;
  text: string;
};

type GitDiffPreview = {
  meta: ReadonlyArray<string>;
  lines: ReadonlyArray<GitDiffLine>;
};

type GitSplitDiffRow = {
  kind: "context" | "modified" | "add" | "remove";
  leftNumber: number | null;
  rightNumber: number | null;
  leftText: string;
  rightText: string;
};

const DRAWER_LIST_STACK_CLASS = "space-y-1";
const DRAWER_LIST_ROW_CLASS = "w-full rounded-lg px-2.5 py-1.5 text-left text-[13px] leading-5 transition-colors";
const DRAWER_LIST_LABEL_CLASS = "min-w-0 flex-1 truncate text-[13px] leading-5";
const DRAWER_LIST_META_CLASS = "shrink-0 text-[11px] text-app-subtle";
const DRAWER_ICON_ACTION_CLASS =
  "flex size-6 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground";
const DRAWER_OVERFLOW_ACTION_CLASS =
  "absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-subtle opacity-0 transition-all duration-200 hover:bg-app-surface-hover hover:text-app-foreground group-hover:opacity-100";
const DRAWER_SECTION_HEADER_CLASS = "flex items-center justify-between gap-3 px-1.5";

const THREAD_ITEMS = [
  { name: "创建 Tauri 2 React+TS+shadcn/ui 模块化脚手架", time: "59m", active: true, status: "running" },
  { name: "设计 Codex 风格工作台布局", time: "12m", active: false, status: "needs-reply" },
  { name: "配置打包与签名信息", time: "24h", active: false, status: "failed" },
  { name: "实现 Agent 会话与任务抽屉", time: "2d", active: false, status: "completed" },
] satisfies ReadonlyArray<ThreadItem>;

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
] as const;

const RECENT_PROJECTS: ReadonlyArray<ProjectOption> = [
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

const TERMINAL_LINES = [
  "1:20:15 AM [vite] client hmr update /src/widgets/dashboard-overview/ui/dashboard-overview.tsx",
  "Info File src-tauri/tauri.conf.json changed. Rebuilding application...",
  "Running DevCommand (`cargo run --no-default-features --color always --`)",
  "Compiling tiy-agent v0.1.0 (/Users/jorben/Documents/Codespace/TiyAgents/tiy-desktop/src-tauri)",
  "Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.27s",
  "Running `target/debug/tiy-agent`",
] as const;

const PROJECT_TREE_ITEMS: ReadonlyArray<ProjectTreeItem> = [
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

const GIT_CHANGE_FILES: ReadonlyArray<GitChangeFile> = [
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
] as const;

const GIT_HISTORY_ITEMS: ReadonlyArray<GitHistoryItem> = [
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
] as const;

function getDiffTemplate(file: GitChangeFile) {
  const fileName = file.path.split("/").pop() ?? file.path;

  if (file.path.endsWith(".tsx") || file.path.endsWith(".ts")) {
    return {
      before: [
        'const panelDensity = "comfortable";',
        "const enablePreview = false;",
        `return <section data-file="${fileName}">{panelDensity}</section>;`,
      ],
      after: [
        'const panelDensity = "compact";',
        "const enablePreview = true;",
        'const previewMode = "diff";',
        `return <section data-file="${fileName}" data-preview="diff">{panelDensity}</section>;`,
      ],
    };
  }

  if (file.path.endsWith(".css")) {
    return {
      before: [
        ".tracked-row {",
        "  gap: 10px;",
        "  padding: 8px 10px;",
        "}",
      ],
      after: [
        ".tracked-row {",
        "  gap: 8px;",
        "  padding: 6px 8px;",
        "}",
        ".tracked-row:hover { background: var(--app-surface-hover); }",
      ],
    };
  }

  if (file.path.endsWith(".json")) {
    return {
      before: [
        "{",
        '  "beforeDevCommand": "npm run dev:web",',
        '  "beforeBuildCommand": "npm run build:web"',
        "}",
      ],
      after: [
        "{",
        '  "beforeDevCommand": "npm run dev:web",',
        '  "beforeBuildCommand": "npm run build:web",',
        '  "sourceControlPreview": true',
        "}",
      ],
    };
  }

  if (file.path.endsWith(".md")) {
    return {
      before: [
        "# Tiy Desktop",
        "",
        "- Project Panel",
        "- Git Panel",
      ],
      after: [
        "# Tiy Desktop",
        "",
        "- Project Panel",
        "- Git Panel",
        "- Diff preview overlay",
      ],
    };
  }

  return {
    before: [
      `// ${fileName}`,
      "export const panelState = {",
      '  density: "comfortable",',
      "};",
    ],
    after: [
      `// ${fileName}`,
      "export const panelState = {",
      '  density: "compact",',
      '  preview: "diff",',
      "};",
    ],
  };
}

function buildGitDiffPreview(file: GitChangeFile): GitDiffPreview {
  const template = getDiffTemplate(file);
  const startLine = 18;

  if (file.status === "A") {
    return {
      meta: [
        `diff --git a/${file.path} b/${file.path}`,
        "new file mode 100644",
        "--- /dev/null",
        `+++ b/${file.path}`,
        `@@ -0,0 +1,${template.after.length} @@`,
      ],
      lines: template.after.map((text, index) => ({
        kind: "add",
        oldNumber: null,
        newNumber: index + 1,
        text,
      })),
    };
  }

  if (file.status === "D") {
    return {
      meta: [
        `diff --git a/${file.path} b/${file.path}`,
        "deleted file mode 100644",
        `--- a/${file.path}`,
        "+++ /dev/null",
        `@@ -1,${template.before.length} +0,0 @@`,
      ],
      lines: template.before.map((text, index) => ({
        kind: "remove",
        oldNumber: index + 1,
        newNumber: null,
        text,
      })),
    };
  }

  const lines: GitDiffLine[] = [];
  let oldLine = startLine;
  let newLine = startLine;
  const maxLength = Math.max(template.before.length, template.after.length);

  for (let index = 0; index < maxLength; index += 1) {
    const previous = template.before[index];
    const next = template.after[index];

    if (previous !== undefined && next !== undefined && previous === next) {
      lines.push({
        kind: "context",
        oldNumber: oldLine,
        newNumber: newLine,
        text: previous,
      });
      oldLine += 1;
      newLine += 1;
      continue;
    }

    if (previous !== undefined) {
      lines.push({
        kind: "remove",
        oldNumber: oldLine,
        newNumber: null,
        text: previous,
      });
      oldLine += 1;
    }

    if (next !== undefined) {
      lines.push({
        kind: "add",
        oldNumber: null,
        newNumber: newLine,
        text: next,
      });
      newLine += 1;
    }
  }

  return {
    meta: [
      `diff --git a/${file.path} b/${file.path}`,
      `--- a/${file.path}`,
      `+++ b/${file.path}`,
      `@@ -${startLine},${template.before.length} +${startLine},${template.after.length} @@`,
    ],
    lines,
  };
}

function buildGitSplitDiffRows(file: GitChangeFile): ReadonlyArray<GitSplitDiffRow> {
  const template = getDiffTemplate(file);

  if (file.status === "A") {
    return template.after.map((text, index) => ({
      kind: "add",
      leftNumber: null,
      rightNumber: index + 1,
      leftText: "",
      rightText: text,
    }));
  }

  if (file.status === "D") {
    return template.before.map((text, index) => ({
      kind: "remove",
      leftNumber: index + 1,
      rightNumber: null,
      leftText: text,
      rightText: "",
    }));
  }

  const rows: GitSplitDiffRow[] = [];
  let leftNumber = 18;
  let rightNumber = 18;
  const maxLength = Math.max(template.before.length, template.after.length);

  for (let index = 0; index < maxLength; index += 1) {
    const previous = template.before[index];
    const next = template.after[index];

    if (previous !== undefined && next !== undefined && previous === next) {
      rows.push({
        kind: "context",
        leftNumber,
        rightNumber,
        leftText: previous,
        rightText: next,
      });
      leftNumber += 1;
      rightNumber += 1;
      continue;
    }

    if (previous !== undefined && next !== undefined) {
      rows.push({
        kind: "modified",
        leftNumber,
        rightNumber,
        leftText: previous,
        rightText: next,
      });
      leftNumber += 1;
      rightNumber += 1;
      continue;
    }

    if (previous !== undefined) {
      rows.push({
        kind: "remove",
        leftNumber,
        rightNumber: null,
        leftText: previous,
        rightText: "",
      });
      leftNumber += 1;
    }

    if (next !== undefined) {
      rows.push({
        kind: "add",
        leftNumber: null,
        rightNumber,
        leftText: "",
        rightText: next,
      });
      rightNumber += 1;
    }
  }

  return rows;
}

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

const MENU_TRIGGER_CLASS =
  "flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-sm transition-colors hover:bg-app-surface-hover";
const MENU_TRIGGER_ICON_CLASS = "size-4 shrink-0 text-app-subtle";
const MENU_TRIGGER_LABEL_CLASS = "min-w-0 flex-1 truncate text-left text-sm";
const MENU_SUBMENU_GROUP_CLASS =
  "ml-6 mt-1 border-l border-app-border/70 pl-3";
const MENU_SUBMENU_OPTION_CLASS =
  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors";
const MENU_SUBMENU_ICON_CLASS = "size-3.5 shrink-0 text-app-subtle/90";
const MENU_SUBMENU_LABEL_CLASS = "flex-1 truncate";
const MAC_USER_MENU_OFFSET = "ml-[74px]";
const MAC_USER_MENU_POPOVER_OFFSET = "left-[74px]";
const AUTH_STORAGE_KEY = "tiy-agent-auth-session";
const PANEL_VISIBILITY_STORAGE_KEY = "tiy-agent-panel-visibility";
const UPDATE_STATUS_DURATION = 2200;

type MockUserSession = {
  name: string;
  avatar: string;
  email: string;
};

type PanelVisibilityState = {
  isSidebarOpen: boolean;
  isDrawerOpen: boolean;
  isTerminalCollapsed: boolean;
};

const MOCK_USER_SESSION: MockUserSession = {
  name: "Jorben Zhu",
  avatar: "JZ",
  email: "jorbenzhu@gmail.com",
};

const DEFAULT_PANEL_VISIBILITY_STATE: PanelVisibilityState = {
  isSidebarOpen: true,
  isDrawerOpen: false,
  isTerminalCollapsed: true,
};

const CONTEXT_WINDOW_INFO = {
  label: "Context Window",
  used: "12K",
  total: "128K",
  usageRatio: 12 / 128,
};

const CONTEXT_WINDOW_USAGE_DETAIL = {
  usedPercent: Math.round(CONTEXT_WINDOW_INFO.usageRatio * 100),
  leftPercent: 100 - Math.round(CONTEXT_WINDOW_INFO.usageRatio * 100),
};

const THREAD_STATUS_META: Record<
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

function getStoredUserSession(): MockUserSession | null {
  if (typeof window === "undefined") {
    return null;
  }

  const rawValue = window.localStorage.getItem(AUTH_STORAGE_KEY);

  if (!rawValue) {
    return null;
  }

  try {
    const parsed = JSON.parse(rawValue) as Partial<MockUserSession>;

    if (
      typeof parsed.name === "string" &&
      typeof parsed.avatar === "string" &&
      typeof parsed.email === "string"
    ) {
      return {
        name: parsed.name,
        avatar: parsed.avatar,
        email: parsed.email,
      };
    }
  } catch {
    // ignore malformed cached session
  }

  return null;
}

function getStoredPanelVisibilityState(): PanelVisibilityState {
  if (typeof window === "undefined") {
    return DEFAULT_PANEL_VISIBILITY_STATE;
  }

  const rawValue = window.localStorage.getItem(PANEL_VISIBILITY_STORAGE_KEY);

  if (!rawValue) {
    return DEFAULT_PANEL_VISIBILITY_STATE;
  }

  try {
    const parsed = JSON.parse(rawValue) as Partial<PanelVisibilityState>;

    return {
      isSidebarOpen:
        typeof parsed.isSidebarOpen === "boolean"
          ? parsed.isSidebarOpen
          : DEFAULT_PANEL_VISIBILITY_STATE.isSidebarOpen,
      isDrawerOpen:
        typeof parsed.isDrawerOpen === "boolean"
          ? parsed.isDrawerOpen
          : DEFAULT_PANEL_VISIBILITY_STATE.isDrawerOpen,
      isTerminalCollapsed:
        typeof parsed.isTerminalCollapsed === "boolean"
          ? parsed.isTerminalCollapsed
          : DEFAULT_PANEL_VISIBILITY_STATE.isTerminalCollapsed,
    };
  } catch {
    // ignore malformed cached panel visibility state
  }

  return DEFAULT_PANEL_VISIBILITY_STATE;
}

function buildInitialWorkspaces(): Array<WorkspaceItem> {
  return WORKSPACE_ITEMS.map((workspace) => ({
    ...workspace,
    threads: workspace.threads.map((thread, index) => ({
      ...thread,
      id: `${workspace.id}-thread-${index + 1}`,
      active: false,
    })),
  }));
}

function clearActiveThreads(workspaces: ReadonlyArray<WorkspaceItem>): Array<WorkspaceItem> {
  return workspaces.map((workspace) => ({
    ...workspace,
    threads: workspace.threads.map((thread) => ({
      ...thread,
      active: false,
    })),
  }));
}

function isEditableSelectionTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  return Boolean(target.closest("input, textarea, select, [contenteditable=''], [contenteditable='true'], [contenteditable='plaintext-only']"));
}

function isNodeInsideContainer(container: HTMLElement | null, node: Node | null) {
  return Boolean(container && node && container.contains(node));
}

function selectContainerContents(container: HTMLElement) {
  const selection = window.getSelection();
  if (!selection) {
    return;
  }

  const range = document.createRange();
  range.selectNodeContents(container);
  selection.removeAllRanges();
  selection.addRange(range);
}

function getActiveThread(workspaces: ReadonlyArray<WorkspaceItem>): WorkspaceThreadItem | null {
  for (const workspace of workspaces) {
    const activeThread = workspace.threads.find((thread) => thread.active);

    if (activeThread) {
      return activeThread;
    }
  }

  return null;
}

function activateThread(workspaces: ReadonlyArray<WorkspaceItem>, threadId: string): Array<WorkspaceItem> {
  return workspaces.map((workspace) => ({
    ...workspace,
    threads: workspace.threads.map((thread) => ({
      ...thread,
      active: thread.id === threadId,
    })),
  }));
}

function buildThreadTitle(prompt: string) {
  const compactPrompt = prompt.trim().replace(/\s+/g, " ");

  if (compactPrompt.length <= 30) {
    return compactPrompt;
  }

  return `${compactPrompt.slice(0, 30)}...`;
}

function mergeRecentProjects(
  currentProjects: ReadonlyArray<ProjectOption>,
  nextProject: ProjectOption,
): Array<ProjectOption> {
  return [
    nextProject,
    ...currentProjects.filter(
      (project) =>
        !(project.id === nextProject.id || (project.name === nextProject.name && project.path === nextProject.path)),
    ),
  ].slice(0, 6);
}

function buildProjectOptionFromPath(path: string | null): ProjectOption | null {
  if (!path) {
    return null;
  }

  const normalizedPath = path.replace(/\\/g, "/").replace(/\/+$/g, "");
  const segments = normalizedPath.split("/");
  const folderName = segments[segments.length - 1] || "new-project";
  const normalizedId = `${folderName}-${normalizedPath}`
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

  return {
    id: normalizedId || `project-${Date.now()}`,
    name: folderName,
    path: normalizedPath,
    lastOpenedLabel: "刚刚",
  };
}

function formatProjectPathLabel(path: string) {
  const normalizedPath = path.replace(/\\/g, "/").replace(/\/+$/g, "");
  const segments = normalizedPath.split("/").filter(Boolean);

  if (segments.length <= 4) {
    return normalizedPath;
  }

  return `.../${segments.slice(-4).join("/")}`;
}

function ThreadStatusIndicator({
  status,
  emphasis = "default",
}: {
  status: ThreadStatus;
  emphasis?: "default" | "subtle";
}) {
  const meta = THREAD_STATUS_META[status];
  const Icon = meta.icon;
  const isSubtle = emphasis === "subtle";
  const containerClassName = cn(
    "flex size-[1.15rem] shrink-0 items-center justify-center rounded-md border",
    status === "failed"
      ? isSubtle
        ? "border-app-danger/10 bg-app-danger/8 text-app-danger/80 dark:border-app-danger/16 dark:bg-app-danger/12 dark:text-app-danger/82"
        : "border-app-danger/15 bg-app-danger/12 text-app-danger dark:border-app-danger/20 dark:bg-app-danger/16"
      : isSubtle
        ? "border-app-border/70 bg-app-surface-muted/65 text-app-subtle dark:border-app-border dark:bg-app-surface-muted/55 dark:text-app-muted"
        : status === "running"
          ? "border-app-info/15 bg-app-info/12 text-app-info dark:border-app-info/20 dark:bg-app-info/16"
          : status === "completed"
            ? "border-app-success/15 bg-app-success/12 text-app-success dark:border-app-success/20 dark:bg-app-success/16"
            : "border-app-warning/20 bg-app-warning/14 text-app-warning dark:border-app-warning/22 dark:bg-app-warning/18",
  );

  return (
    <span className={containerClassName} title={meta.label} aria-label={meta.label}>
      <Icon className={cn("size-3.5", meta.spin && "animate-spin")} />
    </span>
  );
}

export function DashboardOverview() {
  const { data, error, isLoading, refetch } = useSystemMetadata();
  const { theme, setTheme } = useTheme();
  const { language, setLanguage } = useLanguage();
  const { general: generalPreferences, workspaces: settingsWorkspaces, providers, commands, policy, updateGeneralPreference, addWorkspace, removeWorkspace, updateWorkspace, setDefaultWorkspace, addProvider, removeProvider, updateProvider, agentProfiles, activeAgentProfileId, addAgentProfile, removeAgentProfile, updateAgentProfile, setActiveAgentProfile, duplicateAgentProfile, updatePolicySetting, addAllowEntry, removeAllowEntry, updateAllowEntry, addDenyEntry, removeDenyEntry, updateDenyEntry, addWritableRoot, removeWritableRoot, updateWritableRoot, addCommand, removeCommand, updateCommand } = useWorkbenchSettings();
  const [workspaces, setWorkspaces] = useState<Array<WorkspaceItem>>(() => buildInitialWorkspaces());
  const [recentProjects, setRecentProjects] = useState<Array<ProjectOption>>(() => [...RECENT_PROJECTS]);
  const [selectedProject, setSelectedProject] = useState<ProjectOption | null>(() => RECENT_PROJECTS[0] ?? null);
  const [isNewThreadMode, setNewThreadMode] = useState(true);
  const [isSettingsOpen, setSettingsOpen] = useState(false);
  const [activeSettingsCategory, setActiveSettingsCategory] = useState<SettingsCategory>("account");
  const [panelVisibilityState, setPanelVisibilityState] = useState<PanelVisibilityState>(() => getStoredPanelVisibilityState());
  const [terminalHeight, setTerminalHeight] = useState(DEFAULT_TERMINAL_HEIGHT);
  const [terminalResize, setTerminalResize] = useState<{ startY: number; startHeight: number } | null>(null);
  const [composerValue, setComposerValue] = useState("");
  const [openSettingsSection, setOpenSettingsSection] = useState<"theme" | "language" | null>(null);
  const [isUserMenuOpen, setUserMenuOpen] = useState(false);
  const [userSession, setUserSession] = useState<MockUserSession | null>(() => getStoredUserSession());
  const [isCheckingUpdates, setCheckingUpdates] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [isComposerProfileMenuOpen, setComposerProfileMenuOpen] = useState(false);
  const [openWorkspaces, setOpenWorkspaces] = useState<Record<string, boolean>>(() => Object.fromEntries(WORKSPACE_ITEMS.map((workspace) => [workspace.id, workspace.defaultOpen])));
  const [activeDrawerPanel, setActiveDrawerPanel] = useState<DrawerPanel>("project");
  const [selectedDiffFilePreview, setSelectedDiffFilePreview] = useState<{ fileId: string; isStaged: boolean } | null>(null);
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const composerProfileMenuRef = useRef<HTMLDivElement | null>(null);
  const mainContentRef = useRef<HTMLElement | null>(null);
  const settingsContentRef = useRef<HTMLDivElement | null>(null);
  const userMenuRef = useRef<HTMLDivElement | null>(null);
  const selectedDiffFile = GIT_CHANGE_FILES.find((file) => file.id === selectedDiffFilePreview?.fileId) ?? null;
  const activeThread = getActiveThread(workspaces);
  const activeComposerProfile = agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0];
  const { isSidebarOpen, isDrawerOpen, isTerminalCollapsed } = panelVisibilityState;

  const setSidebarOpen = (nextState: boolean | ((current: boolean) => boolean)) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isSidebarOpen: typeof nextState === "function" ? nextState(current.isSidebarOpen) : nextState,
    }));
  };

  const setDrawerOpen = (nextState: boolean | ((current: boolean) => boolean)) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isDrawerOpen: typeof nextState === "function" ? nextState(current.isDrawerOpen) : nextState,
    }));
  };

  const setTerminalCollapsed = (nextState: boolean | ((current: boolean) => boolean)) => {
    setPanelVisibilityState((current) => ({
      ...current,
      isTerminalCollapsed: typeof nextState === "function" ? nextState(current.isTerminalCollapsed) : nextState,
    }));
  };

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
    if (!isUserMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && userMenuRef.current?.contains(target)) {
        return;
      }

      setUserMenuOpen(false);
      setOpenSettingsSection(null);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isUserMenuOpen]);

  useEffect(() => {
    if (!isComposerProfileMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && composerProfileMenuRef.current?.contains(target)) {
        return;
      }

      setComposerProfileMenuOpen(false);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setComposerProfileMenuOpen(false);
      }
    };

    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [isComposerProfileMenuOpen]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    if (userSession) {
      window.localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(userSession));
      return;
    }

    window.localStorage.removeItem(AUTH_STORAGE_KEY);
  }, [userSession]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.localStorage.setItem(PANEL_VISIBILITY_STORAGE_KEY, JSON.stringify(panelVisibilityState));
  }, [panelVisibilityState]);

  useEffect(() => {
    if (!updateStatus || typeof window === "undefined") {
      return;
    }

    const timeout = window.setTimeout(() => {
      setUpdateStatus(null);
    }, UPDATE_STATUS_DURATION);

    return () => window.clearTimeout(timeout);
  }, [updateStatus]);

  useEffect(() => {
    if (!selectedDiffFile || typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSelectedDiffFilePreview(null);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [selectedDiffFile]);

  useEffect(() => {
    if (!isSettingsOpen || typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSettingsOpen(false);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isSettingsOpen]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.key.toLowerCase() !== "a") {
        return;
      }

      if (isEditableSelectionTarget(event.target)) {
        return;
      }

      const selection = window.getSelection();
      const selectionInsideSettingsContent =
        isNodeInsideContainer(settingsContentRef.current, selection?.anchorNode ?? null) ||
        isNodeInsideContainer(settingsContentRef.current, selection?.focusNode ?? null);
      const targetInsideSettingsContent = isNodeInsideContainer(
        settingsContentRef.current,
        event.target instanceof Node ? event.target : null,
      );
      const selectionInsideMainContent =
        isNodeInsideContainer(mainContentRef.current, selection?.anchorNode ?? null) ||
        isNodeInsideContainer(mainContentRef.current, selection?.focusNode ?? null);
      const targetInsideMainContent = isNodeInsideContainer(
        mainContentRef.current,
        event.target instanceof Node ? event.target : null,
      );

      if (settingsContentRef.current && (targetInsideSettingsContent || selectionInsideSettingsContent)) {
        event.preventDefault();
        selectContainerContents(settingsContentRef.current);
        return;
      }

      if (mainContentRef.current && (targetInsideMainContent || selectionInsideMainContent)) {
        event.preventDefault();
        selectContainerContents(mainContentRef.current);
        return;
      }

      event.preventDefault();
      selection?.removeAllRanges();
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const handleWorkspaceToggle = (workspaceId: string) => {
    setOpenWorkspaces((current) => ({
      ...current,
      [workspaceId]: !current[workspaceId],
    }));
  };

  const handleEnterNewThreadMode = () => {
    setNewThreadMode(true);
    setWorkspaces((current) => clearActiveThreads(current));
  };

  const handleThreadSelect = (threadId: string) => {
    setNewThreadMode(false);
    setWorkspaces((current) => activateThread(current, threadId));
  };

  const handleProjectSelect = (project: ProjectOption) => {
    const nextProject = {
      ...project,
      lastOpenedLabel: "刚刚",
    };

    setSelectedProject(nextProject);
    setRecentProjects((current) => mergeRecentProjects(current, nextProject));
  };

  const handleComposerSubmit = () => {
    const trimmedValue = composerValue.trim();

    if (!trimmedValue) {
      return;
    }

    if (isNewThreadMode) {
      if (!selectedProject) {
        return;
      }

      const project = {
        ...selectedProject,
        lastOpenedLabel: "刚刚",
      };
      const existingWorkspace = workspaces.find(
        (workspace) =>
          workspace.id === project.id ||
          workspace.name === project.name ||
          (workspace.path && workspace.path === project.path),
      );
      const nextThread: WorkspaceThreadItem = {
        id: `${project.id}-thread-${Date.now()}`,
        name: buildThreadTitle(trimmedValue),
        time: "刚刚",
        active: true,
        status: "running",
      };

      setSelectedProject(project);
      setRecentProjects((current) => mergeRecentProjects(current, project));
      setWorkspaces((current) => {
        const cleared = clearActiveThreads(current);

        if (existingWorkspace) {
          return cleared.map((workspace) =>
            workspace.id === existingWorkspace.id
              ? {
                  ...workspace,
                  name: project.name,
                  path: project.path,
                  threads: [nextThread, ...workspace.threads],
                }
              : workspace,
          );
        }

        return [
          {
            id: project.id,
            name: project.name,
            defaultOpen: true,
            path: project.path,
            threads: [nextThread],
          },
          ...cleared,
        ];
      });
      setOpenWorkspaces((current) => ({
        ...current,
        [existingWorkspace?.id ?? project.id]: true,
      }));
      setNewThreadMode(false);
      setComposerValue("");
      return;
    }

    setComposerValue("");
  };

  const handleThemeSelect = (nextTheme: ThemePreference) => {
    setTheme(nextTheme);
    setOpenSettingsSection("theme");
  };

  const handleLanguageSelect = (nextLanguage: LanguagePreference) => {
    setLanguage(nextLanguage);
    setOpenSettingsSection("language");
  };

  const handleOpenSettings = (category: SettingsCategory = "account") => {
    setActiveSettingsCategory(category);
    setSettingsOpen(true);
    setUserMenuOpen(false);
    setOpenSettingsSection(null);
  };

  const handleCloseSettings = () => {
    setSettingsOpen(false);
  };

  const handleUserMenuToggle = () => {
    setUserMenuOpen((current) => {
      const nextOpen = !current;
      setOpenSettingsSection(null);

      return nextOpen;
    });
  };

  const handleLogin = () => {
    setUserSession(MOCK_USER_SESSION);
    setOpenSettingsSection(null);
    setUserMenuOpen(!isSettingsOpen);
  };

  const handleLogout = () => {
    setUserSession(null);
    setOpenSettingsSection(null);
    setUserMenuOpen(false);
  };

  const handleCheckUpdates = () => {
    if (isCheckingUpdates) {
      return;
    }

    setCheckingUpdates(true);

    window.setTimeout(() => {
      setCheckingUpdates(false);
      setUpdateStatus(`当前已是最新版本 v${data?.version ?? "0.1.0"}`);
    }, 900);
  };

  const isMacOS = data?.platform === "macos" || (typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"));
  const isWindows = data?.platform === "windows" || (typeof navigator !== "undefined" && navigator.userAgent.includes("Windows"));
  const selectedThemeOption = THEME_OPTIONS.find((option) => option.value === theme) ?? THEME_OPTIONS[0];
  const selectedThemeSummary = theme === "system" ? "跟随系统" : selectedThemeOption.label;
  const selectedLanguageOption = LANGUAGE_OPTIONS.find((option) => option.value === language) ?? LANGUAGE_OPTIONS[1];

  return (
    <main className="h-screen overflow-hidden select-none bg-app-canvas text-app-foreground">
      <WorkbenchTopBar
        isMacOS={isMacOS}
        isWindows={isWindows}
        isSidebarOpen={isSidebarOpen}
        isDrawerOpen={isDrawerOpen}
        isTerminalCollapsed={isTerminalCollapsed}
        isUserMenuOpen={isUserMenuOpen}
        isSettingsOpen={isSettingsOpen}
        isLoggedIn={Boolean(userSession)}
        userSession={userSession}
        isCheckingUpdates={isCheckingUpdates}
        updateStatus={updateStatus}
        openSettingsSection={openSettingsSection}
        userMenuRef={userMenuRef}
        selectedLanguageLabel={selectedLanguageOption.label}
        selectedThemeSummary={selectedThemeSummary}
        language={language}
        theme={theme}
        onToggleUserMenu={handleUserMenuToggle}
        onLogin={handleLogin}
        onLogout={handleLogout}
        onCheckUpdates={handleCheckUpdates}
        onOpenSettings={() => handleOpenSettings("account")}
        onSelectLanguage={handleLanguageSelect}
        onSelectTheme={handleThemeSelect}
        onToggleSettingsSection={setOpenSettingsSection}
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
            <div className="space-y-1">
              <button
                type="button"
                className={cn(
                  "group flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-[transform,box-shadow,background-color,border-color,color] duration-200 active:scale-[0.99]",
                  isNewThreadMode
                    ? "border-app-border-strong bg-app-surface-active text-app-foreground shadow-[0_4px_14px_rgba(15,23,42,0.08)]"
                    : "border-transparent bg-transparent text-app-muted hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)]",
                )}
                onClick={handleEnterNewThreadMode}
              >
                <MessageSquarePlus
                  className={cn(
                    "size-4 shrink-0 transition-colors duration-200",
                    isNewThreadMode ? "text-app-foreground" : "text-app-subtle group-hover:text-app-foreground",
                  )}
                />
                <span className="truncate text-sm font-medium">New thread</span>
              </button>

              <button
                type="button"
                className="group flex w-full items-center gap-2.5 rounded-xl border border-transparent bg-transparent px-3 py-2.5 text-left text-app-muted transition-[transform,box-shadow,background-color,border-color,color] duration-200 hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)] active:scale-[0.99]"
              >
                <Boxes className="size-4 shrink-0 text-app-subtle transition-colors duration-200 group-hover:text-app-foreground" />
                <span className="truncate text-sm font-medium">Marketplace</span>
              </button>
            </div>

            <div className="mt-6 flex items-center justify-between px-3">
              <span className="text-xs uppercase tracking-[0.14em] text-app-subtle">Threads</span>
              <FolderPlus className="size-3.5 text-app-subtle" />
            </div>

            <div className="mx-1 mt-3 h-px shrink-0 bg-app-border" />

            <div className="mt-3 min-h-0 flex-1 overflow-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              <div className="space-y-1.5">
                {workspaces.map((workspace) => {
                  const isOpen = openWorkspaces[workspace.id] ?? workspace.defaultOpen;
                  const FolderIcon = isOpen ? FolderOpen : Folder;

                  return (
                    <div key={workspace.id} className="space-y-1">
                      <div className="group px-1">
                        <div className="relative">
                          <button
                            type="button"
                            className={cn(
                              "flex items-center gap-2 pr-10 text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                              DRAWER_LIST_ROW_CLASS,
                            )}
                            onClick={() => handleWorkspaceToggle(workspace.id)}
                          >
                            <FolderIcon className="size-4 shrink-0 text-app-muted" />
                            <span className={DRAWER_LIST_LABEL_CLASS}>{workspace.name}</span>
                          </button>
                          <button
                            type="button"
                            aria-label="更多操作"
                            title="更多操作"
                            className={DRAWER_OVERFLOW_ACTION_CLASS}
                          >
                            <MoreHorizontal className="size-4" />
                          </button>
                        </div>
                      </div>

                      {isOpen && workspace.threads.length > 0 ? (
                        <div className={cn(DRAWER_LIST_STACK_CLASS, "pl-2.5")}>
                          {workspace.threads.map((thread) => (
                            <div key={thread.id} className="group relative">
                              <button
                                type="button"
                                className={cn(
                                  `${DRAWER_LIST_ROW_CLASS} border pr-11`,
                                  thread.active
                                    ? "border-app-border-strong bg-app-surface-active text-app-foreground"
                                    : "border-transparent bg-transparent text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                )}
                                onClick={() => handleThreadSelect(thread.id)}
                              >
                                <div className="flex items-center gap-2">
                                  <ThreadStatusIndicator
                                    status={thread.status}
                                    emphasis={thread.active ? "default" : "subtle"}
                                  />
                                  <p className={DRAWER_LIST_LABEL_CLASS}>{thread.name}</p>
                                </div>
                              </button>
                              <span
                                className={cn(
                                  "pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-[11px] text-app-subtle transition-opacity duration-200 group-hover:opacity-0",
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
                                  DRAWER_OVERFLOW_ACTION_CLASS,
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
          </div>
        </aside>

        <section className="min-w-0 flex-1 min-h-0">
          <div className="flex h-full min-h-0 flex-col">
            <div className="flex min-h-0 flex-1 overflow-hidden">
              <section ref={mainContentRef} className="min-w-0 flex-1 min-h-0 select-text bg-app-canvas">
                <div className="flex h-full min-h-0 flex-col">
                  {isNewThreadMode ? (
                    <div className="relative min-h-0 flex-1">
                      <div className="flex h-full items-center justify-center px-6 pb-8 pt-6">
                        <NewThreadEmptyState
                          recentProjects={recentProjects}
                          selectedProject={selectedProject}
                          onSelectProject={handleProjectSelect}
                        />
                      </div>
                      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-14 bg-gradient-to-b from-transparent via-app-overlay via-55% to-app-canvas" />
                    </div>
                  ) : (
                    <>
                      <div className="flex h-12 items-center gap-3 border-b border-app-border px-5">
                        <div className="min-w-0 flex-1">
                          <div className="flex min-w-0 items-center gap-2">
                            {activeThread ? <ThreadStatusIndicator status={activeThread.status} /> : null}
                            <p className="truncate text-sm font-semibold text-app-foreground">
                              {activeThread?.name ?? "创建 Tauri 2 React+TS+shadcn/ui 模块化脚手架"}
                            </p>
                          </div>
                        </div>
                        <div className="ml-auto flex shrink-0 items-center gap-1.5">
                          <div className="group/context-window relative shrink-0">
                            <span
                              tabIndex={0}
                              className="relative inline-flex overflow-hidden rounded-full border border-app-border bg-app-surface-muted text-[11px] text-app-muted outline-none"
                            >
                              <span
                                className="pointer-events-none absolute inset-y-0 left-0 rounded-full bg-primary/12"
                                style={{ width: `${CONTEXT_WINDOW_INFO.usageRatio * 100}%` }}
                              />
                              <span className="relative inline-flex items-center gap-1.5 px-2 py-0.5">
                                <span className="text-app-subtle">{CONTEXT_WINDOW_INFO.label}</span>
                                <span className="font-semibold text-app-foreground">
                                  {CONTEXT_WINDOW_INFO.used} / {CONTEXT_WINDOW_INFO.total}
                                </span>
                              </span>
                            </span>
                            <div className="pointer-events-none absolute left-1/2 top-[calc(100%+0.5rem)] z-20 w-max min-w-[190px] -translate-x-1/2 translate-y-1 rounded-xl border border-app-border bg-app-menu px-3 py-2 text-center opacity-0 shadow-[0_14px_32px_rgba(15,23,42,0.14)] transition-[opacity,transform] duration-150 group-hover/context-window:translate-y-0 group-hover/context-window:opacity-100 group-focus-within/context-window:translate-y-0 group-focus-within/context-window:opacity-100 dark:shadow-[0_16px_36px_rgba(0,0,0,0.38)]">
                              <p className="whitespace-nowrap text-[11px] font-semibold text-app-foreground">
                                {CONTEXT_WINDOW_USAGE_DETAIL.usedPercent}% used
                                <span className="font-normal text-app-subtle"> ({CONTEXT_WINDOW_USAGE_DETAIL.leftPercent}% left)</span>
                              </p>
                              <p className="mt-1 whitespace-nowrap text-[11px] text-app-muted">
                                {CONTEXT_WINDOW_INFO.used} / {CONTEXT_WINDOW_INFO.total} tokens used
                              </p>
                            </div>
                          </div>
                          <button
                            type="button"
                            className="inline-flex items-center gap-1.5 text-xs text-app-subtle transition-colors hover:text-app-foreground"
                          >
                            <GitBranch className="size-3.5" />
                            <span>main</span>
                            <ChevronDown className="size-3.5" />
                          </button>
                        </div>
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
                    </>
                  )}

                  <div className="shrink-0 px-6 pb-5 pt-3">
                    <div className="mx-auto max-w-4xl rounded-2xl border border-app-border bg-app-surface px-4 pb-3 pt-3 text-app-muted transition-colors focus-within:border-app-border-strong">
                      <textarea
                        ref={composerRef}
                        value={composerValue}
                        onChange={(event) => setComposerValue(event.target.value)}
                        rows={3}
                        placeholder={
                          isNewThreadMode
                            ? "Ask Tiy anything, @ to add files, / for commands, $ for skills"
                            : "Ask for follow-up changes"
                        }
                        className="max-h-44 min-h-[72px] w-full resize-none select-text overflow-y-auto bg-transparent text-sm leading-6 text-app-foreground outline-none placeholder:text-app-subtle [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
                      />
                      <div className="mt-3 flex items-end justify-between gap-3">
                        <div className="flex min-w-0 items-center gap-1.5">
                          <button type="button" className="-ml-1 mt-1 rounded-lg p-2 text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground">
                            <Plus className="size-4" />
                          </button>

                          {activeComposerProfile ? (
                            <div ref={composerProfileMenuRef} className="relative">
                              <button
                                type="button"
                                className={cn(
                                  "group inline-flex h-9 max-w-[220px] items-center gap-2 rounded-xl border border-app-border/80 bg-app-canvas/55 px-2.5 text-[12px] font-medium text-app-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.35)] backdrop-blur-sm transition-[border-color,background-color,box-shadow,transform] duration-200 hover:border-app-border-strong hover:bg-app-surface hover:shadow-[0_8px_18px_rgba(15,23,42,0.08)]",
                                  isComposerProfileMenuOpen && "border-app-border-strong bg-app-surface shadow-[0_10px_24px_rgba(15,23,42,0.12)]",
                                )}
                                aria-haspopup="menu"
                                aria-expanded={isComposerProfileMenuOpen}
                                aria-label={`Active profile: ${activeComposerProfile.name}`}
                                onClick={() => setComposerProfileMenuOpen((current) => !current)}
                              >
                                <span className="flex size-6 shrink-0 items-center justify-center rounded-lg bg-app-surface text-app-subtle ring-1 ring-app-border/70 transition-colors group-hover:text-app-foreground">
                                  <Bot className="size-3.5" />
                                </span>
                                <span className="truncate">{activeComposerProfile.name}</span>
                                <ChevronDown
                                  className={cn(
                                    "ml-auto size-3.5 shrink-0 text-app-subtle transition-transform duration-200",
                                    isComposerProfileMenuOpen && "rotate-180",
                                  )}
                                />
                              </button>

                              {isComposerProfileMenuOpen ? (
                                <div className="absolute bottom-[calc(100%+10px)] left-0 z-30 min-w-[240px] overflow-hidden rounded-2xl border border-app-border/80 bg-app-surface/95 p-1.5 shadow-[0_20px_48px_rgba(15,23,42,0.16)] backdrop-blur-xl">
                                  <div className="px-2.5 pb-1.5 pt-1">
                                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-app-subtle">Profiles</div>
                                  </div>
                                  <div className="space-y-1">
                                    {agentProfiles.map((profile) => {
                                      const isActive = profile.id === activeAgentProfileId;

                                      return (
                                        <button
                                          key={profile.id}
                                          type="button"
                                          className={cn(
                                            "flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-left transition-colors",
                                            isActive
                                              ? "bg-app-surface-hover text-app-foreground"
                                              : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                                          )}
                                          onClick={() => {
                                            setActiveAgentProfile(profile.id);
                                            setComposerProfileMenuOpen(false);
                                          }}
                                        >
                                          <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-app-canvas text-app-subtle ring-1 ring-app-border/70">
                                            <Bot className="size-3.5" />
                                          </span>
                                          <span className="min-w-0 flex-1 truncate text-[12px] font-medium">{profile.name}</span>
                                          {isActive ? <Check className="size-3.5 shrink-0 text-app-foreground" /> : null}
                                        </button>
                                      );
                                    })}
                                  </div>
                                </div>
                              ) : null}
                            </div>
                          ) : null}
                        </div>
                        <button
                          type="button"
                          onClick={handleComposerSubmit}
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
                  <div className="sticky top-0 z-10 bg-app-drawer/95 px-3 py-2 backdrop-blur-xl">
                    <div className="flex items-center">
                      <WorkbenchSegmentedControl
                        value={activeDrawerPanel}
                        className="w-full min-w-0"
                        options={[
                          {
                            value: "project",
                            label: "文件树",
                            title: "文件树 · Project Panel",
                            content: <FolderOpen className="size-4" />,
                          },
                          {
                            value: "git",
                            label: "版本控制",
                            title: "版本控制 · Git Panel",
                            content: <GitBranch className="size-4" />,
                          },
                        ]}
                        onValueChange={(panel) => setActiveDrawerPanel(panel)}
                      />
                    </div>
                  </div>

                  <div className="min-h-0 flex-1 overscroll-none">
                    {activeDrawerPanel === "project" ? (
                      <ProjectPanel />
                    ) : (
                      <GitPanel onOpenDiffPreview={(fileId, isStaged) => setSelectedDiffFilePreview({ fileId, isStaged })} />
                    )}
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

      {selectedDiffFile ? (
        <GitDiffPreviewPanel
          file={selectedDiffFile}
          isStaged={Boolean(selectedDiffFilePreview?.isStaged)}
          onClose={() => setSelectedDiffFilePreview(null)}
        />
      ) : null}

      {isSettingsOpen ? (
        <WorkbenchSettingsOverlay
          activeCategory={activeSettingsCategory}
          agentProfiles={agentProfiles}
          activeAgentProfileId={activeAgentProfileId}
          contentRef={settingsContentRef}
          generalPreferences={generalPreferences}
          isCheckingUpdates={isCheckingUpdates}
          language={language}
          policy={policy}
          commands={commands}
          providers={providers}
          systemMetadata={data}
          theme={theme}
          updateStatus={updateStatus}
          userSession={userSession}
          workspaces={settingsWorkspaces}
          onAddAgentProfile={addAgentProfile}
          onAddAllowEntry={addAllowEntry}
          onAddCommand={addCommand}
          onAddDenyEntry={addDenyEntry}
          onAddProvider={addProvider}
          onAddWorkspace={addWorkspace}
          onAddWritableRoot={addWritableRoot}
          onCheckUpdates={handleCheckUpdates}
          onClose={handleCloseSettings}
          onDuplicateAgentProfile={duplicateAgentProfile}
          onLogin={handleLogin}
          onLogout={handleLogout}
          onRemoveAgentProfile={removeAgentProfile}
          onRemoveAllowEntry={removeAllowEntry}
          onRemoveCommand={removeCommand}
          onRemoveDenyEntry={removeDenyEntry}
          onRemoveProvider={removeProvider}
          onRemoveWorkspace={removeWorkspace}
          onRemoveWritableRoot={removeWritableRoot}
          onSelectCategory={setActiveSettingsCategory}
          onSelectLanguage={handleLanguageSelect}
          onSelectTheme={handleThemeSelect}
          onSetActiveAgentProfile={setActiveAgentProfile}
          onSetDefaultWorkspace={setDefaultWorkspace}
          onUpdateAgentProfile={updateAgentProfile}
          onUpdateAllowEntry={updateAllowEntry}
          onUpdateCommand={updateCommand}
          onUpdateDenyEntry={updateDenyEntry}
          onUpdateGeneralPreference={updateGeneralPreference}
          onUpdatePolicySetting={updatePolicySetting}
          onUpdateProvider={updateProvider}
          onUpdateWorkspace={updateWorkspace}
          onUpdateWritableRoot={updateWritableRoot}
        />
      ) : null}
    </main>
  );
}

function NewThreadEmptyState({
  recentProjects,
  selectedProject,
  onSelectProject,
}: {
  recentProjects: ReadonlyArray<ProjectOption>;
  selectedProject: ProjectOption | null;
  onSelectProject: (project: ProjectOption) => void;
}) {
  const [isMenuOpen, setMenuOpen] = useState(false);
  const projectMenuRef = useRef<HTMLDivElement | null>(null);
  const activeProject = selectedProject ?? recentProjects[0] ?? null;

  useEffect(() => {
    if (!isMenuOpen || typeof window === "undefined") {
      return;
    }

    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;

      if (target && projectMenuRef.current?.contains(target)) {
        return;
      }

      setMenuOpen(false);
    };

    window.addEventListener("mousedown", handlePointerDown);
    return () => window.removeEventListener("mousedown", handlePointerDown);
  }, [isMenuOpen]);

  const handleChooseFolder = async () => {
    const selectedPath = await open({
      directory: true,
      multiple: false,
      title: "Choose project folder",
    });

    if (typeof selectedPath !== "string") {
      return;
    }

    const nextProject = buildProjectOptionFromPath(selectedPath);

    if (!nextProject) {
      return;
    }

    onSelectProject(nextProject);
    setMenuOpen(false);
  };

  return (
    <div className="relative isolate flex h-full min-h-0 w-full self-stretch items-center justify-center px-4 py-8">
      <div className="pointer-events-none absolute -inset-x-24 -inset-y-10 overflow-hidden">
        <div className="absolute left-[10%] top-[8%] h-64 w-64 rounded-full bg-emerald-400/10 blur-[110px] dark:bg-emerald-400/12" />
        <div className="absolute right-[12%] top-[6%] h-72 w-72 rounded-full bg-sky-400/12 blur-[120px] dark:bg-sky-400/14" />
        <div className="absolute bottom-[8%] left-[24%] h-60 w-60 rounded-full bg-lime-300/10 blur-[110px] dark:bg-lime-300/12" />
        <div className="absolute bottom-[16%] right-[22%] h-52 w-52 rounded-full bg-sky-300/8 blur-[105px] dark:bg-sky-300/10" />
        <div className="absolute inset-x-[24%] top-[12%] h-20 rounded-full bg-white/16 blur-[90px] dark:bg-white/5" />
      </div>

      <div className="relative flex w-full max-w-[28rem] flex-col items-center justify-center gap-4">
        <div className="flex size-11 items-center justify-center rounded-2xl border border-app-border bg-app-surface text-app-foreground shadow-[0_10px_28px_rgba(15,23,42,0.08)] dark:shadow-[0_14px_30px_rgba(0,0,0,0.24)]">
          <img src="/app-icon.png" alt="Tiy Agent logo" className="size-7 object-contain opacity-90" />
        </div>

        <div className="flex flex-col items-center gap-1 text-center">
          <h1 className="text-balance text-[1.45rem] font-medium tracking-[-0.035em] text-app-foreground">
            Anything you need, through conversation.
          </h1>
          <p className="max-w-[30rem] text-sm leading-6 text-app-muted">
            Pick a local workspace first so the next thread can stay grounded in files, commands, and runtime context.
          </p>
        </div>

        <div ref={projectMenuRef} className="relative w-full max-w-[24rem]">
          <button
            type="button"
            aria-haspopup="menu"
            aria-expanded={isMenuOpen}
            className="inline-flex w-full items-center gap-3 rounded-2xl border border-app-border bg-app-surface/85 px-3.5 py-3 text-left shadow-[0_10px_24px_rgba(15,23,42,0.06)] transition-[border-color,background-color,box-shadow,color] duration-200 hover:border-app-border-strong hover:bg-app-surface hover:text-app-foreground hover:shadow-[0_16px_36px_rgba(15,23,42,0.1)] dark:shadow-[0_14px_32px_rgba(0,0,0,0.22)]"
            onClick={() => setMenuOpen((current) => !current)}
          >
            <div className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-border bg-app-surface-muted text-app-subtle">
              <Folder className="size-4 shrink-0" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-[1rem] font-medium tracking-[-0.02em] text-app-foreground">
                  {activeProject?.name ?? "Choose project"}
                </span>
                {activeProject ? (
                  <span className="shrink-0 rounded-full bg-app-surface-muted px-2 py-0.5 text-[10px] font-medium text-app-subtle">
                    {activeProject.lastOpenedLabel}
                  </span>
                ) : null}
              </div>
              <p className="mt-0.5 truncate text-[12px] text-app-subtle" title={activeProject?.path}>
                {activeProject ? formatProjectPathLabel(activeProject.path) : "Select a folder to start a workspace-backed thread"}
              </p>
            </div>
            <ChevronDown
              className={cn(
                "size-4 shrink-0 text-app-subtle transition-transform duration-200",
                isMenuOpen && "rotate-180",
              )}
            />
          </button>

          {isMenuOpen ? (
            <div className="absolute inset-x-0 top-[calc(100%+0.55rem)] z-20 max-h-[15rem] overflow-hidden rounded-[1.1rem] border border-app-border bg-app-menu/98 p-1.5 shadow-[0_18px_40px_-26px_rgba(15,23,42,0.38)] backdrop-blur-xl dark:bg-app-menu/94">
              <div className="flex max-h-[calc(15rem-0.75rem)] flex-col">
                <div className="flex items-center justify-between gap-3 px-2.5 pb-1.5 pt-0.5">
                  <span className="text-[11px] font-medium text-app-subtle">Recent projects</span>
                  {activeProject ? (
                    <span className="rounded-full bg-app-surface-muted px-2 py-0.5 text-[10px] font-medium text-app-subtle">
                      Current
                    </span>
                  ) : null}
                </div>

                <div className="min-h-0 flex-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                  <div className="space-y-0.5">
                    {recentProjects.map((project) => {
                      const isSelected = activeProject?.id === project.id;

                      return (
                        <button
                          key={`${project.id}-${project.path}`}
                          type="button"
                          className={cn(
                            "flex w-full items-start gap-2.5 rounded-xl px-2.5 py-2 text-left transition-colors",
                            isSelected
                              ? "bg-app-surface/75 text-app-foreground"
                              : "text-app-muted hover:bg-app-surface-hover/70 hover:text-app-foreground",
                          )}
                          onClick={() => {
                            onSelectProject(project);
                            setMenuOpen(false);
                          }}
                        >
                          <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface-muted text-app-subtle">
                            <Folder className="size-4 shrink-0" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="min-w-0 flex-1 truncate text-sm font-medium">{project.name}</span>
                              <span className="shrink-0 text-[10px] font-medium text-app-subtle">{project.lastOpenedLabel}</span>
                            </div>
                            <p className="mt-0.5 truncate text-[11px] leading-5 text-app-subtle" title={project.path}>
                              {formatProjectPathLabel(project.path)}
                            </p>
                          </div>
                          {isSelected ? <Check className="mt-0.5 size-4 shrink-0 text-app-foreground" /> : null}
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div className="mx-2 my-1.5 h-px shrink-0 bg-app-border" />

                <button
                  type="button"
                  className="flex w-full shrink-0 items-start gap-2.5 rounded-xl px-2.5 py-2 text-left text-app-foreground transition-colors hover:bg-app-surface-hover/70"
                  onClick={() => void handleChooseFolder()}
                >
                  <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface-muted text-app-subtle">
                    <FolderPlus className="size-4 shrink-0" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium">Choose new folder</div>
                    <p className="mt-0.5 text-[11px] leading-5 text-app-subtle">Browse a local workspace that is not in the recent list</p>
                  </div>
                </button>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function ProjectPanel() {
  const [filterValue, setFilterValue] = useState("");
  const [isRefreshing, setRefreshing] = useState(false);
  const deferredFilterValue = useDeferredValue(filterValue);
  const refreshTimeoutRef = useRef<number | null>(null);
  const normalizedFilter = deferredFilterValue.trim().toLowerCase();
  const visibleItems = normalizedFilter
    ? PROJECT_TREE_ITEMS.filter((item) => item.name.toLowerCase().includes(normalizedFilter))
    : PROJECT_TREE_ITEMS;

  useEffect(() => {
    return () => {
      if (refreshTimeoutRef.current) {
        window.clearTimeout(refreshTimeoutRef.current);
      }
    };
  }, []);

  const handleRefresh = () => {
    setFilterValue("");
    setRefreshing(true);

    if (refreshTimeoutRef.current) {
      window.clearTimeout(refreshTimeoutRef.current);
    }

    refreshTimeoutRef.current = window.setTimeout(() => {
      setRefreshing(false);
      refreshTimeoutRef.current = null;
    }, 700);
  };

  return (
    <div className="flex h-full min-h-0 flex-col px-4 pb-5 pt-2">
      <div className="shrink-0 bg-app-drawer">
        <div className="flex items-center justify-between gap-3 px-1 pr-1 text-[15px] font-medium">
          <div className="flex min-w-0 items-center gap-3">
            <FolderOpen className="size-4 shrink-0 text-app-subtle" />
            <span className="truncate text-app-foreground">tiy-desktop</span>
          </div>
          <button
            type="button"
            aria-label="刷新文件树"
            title="刷新文件树"
            className="flex size-7 shrink-0 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
            onClick={handleRefresh}
          >
            <RefreshCw className={cn("size-3.5", isRefreshing && "animate-spin")} />
          </button>
        </div>

        <div className="relative mt-2.5 pl-5 pr-1 pb-2.5">
          <div className="absolute bottom-0 left-[6px] top-0 w-px bg-app-border" />
          <Input
            value={filterValue}
            onChange={(event) => setFilterValue(event.target.value)}
            placeholder="Filter files"
            aria-label="Filter files"
            className="h-8 rounded-lg border-app-border bg-app-surface-muted px-2.5 text-[13px] text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0"
          />
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto overscroll-none pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <div className="relative pl-5">
          <div className="absolute bottom-0 left-[6px] top-0 w-px bg-app-border" />
          <div className={DRAWER_LIST_STACK_CLASS}>
            {visibleItems.map((item) => (
              <button
                key={item.id}
                type="button"
                className={cn(
                  `${DRAWER_LIST_ROW_CLASS} relative flex items-center gap-2`,
                  item.ignored
                    ? "text-app-subtle/70 hover:bg-app-surface-hover/60 hover:text-app-muted"
                    : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                )}
              >
                <ProjectTreeIcon icon={item.icon} muted={Boolean(item.ignored)} />
                <span className={DRAWER_LIST_LABEL_CLASS}>{item.name}</span>
              </button>
            ))}

            {visibleItems.length === 0 ? (
              <div className="px-2.5 py-2 text-[13px] text-app-subtle">No matching files</div>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

function ProjectTreeIcon({
  icon,
  muted = false,
}: {
  icon: ProjectTreeItem["icon"];
  muted?: boolean;
}) {
  const iconClassName = muted ? "size-4 shrink-0 text-app-subtle/70" : "size-4 shrink-0 text-app-subtle";

  if (icon === "folder") {
    return <Folder className={iconClassName} />;
  }

  if (icon === "git") {
    return <GitBranch className={iconClassName} />;
  }

  if (icon === "json") {
    return <Braces className={iconClassName} />;
  }

  if (icon === "html") {
    return <Code2 className={iconClassName} />;
  }

  if (icon === "css") {
    return <Code2 className={iconClassName} />;
  }

  if (icon === "readme") {
    return <BookOpen className={iconClassName} />;
  }

  if (icon === "license") {
    return <span className={cn("text-base leading-none", muted ? "text-app-subtle/70" : "text-app-subtle")}>=</span>;
  }

  return (
    <span
      className={cn(
        "flex h-[18px] min-w-[18px] items-center justify-center rounded-[4px] px-1 text-[9px] font-semibold uppercase tracking-[0.02em]",
        muted ? "bg-app-surface-muted/60 text-app-subtle/70" : "bg-app-surface-muted text-app-subtle",
      )}
    >
      TS
    </span>
  );
}

function GitPanel({ onOpenDiffPreview }: { onOpenDiffPreview: (fileId: string, isStaged: boolean) => void }) {
  const [commitMessage, setCommitMessage] = useState("");
  const [stagedFiles, setStagedFiles] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(GIT_CHANGE_FILES.map((file) => [file.id, file.initialStaged])),
  );
  const [activeHistoryAction, setActiveHistoryAction] = useState<"fetch" | "pull" | "push" | "refresh" | null>(null);
  const historyActionTimeoutRef = useRef<number | null>(null);
  const stagedCount = GIT_CHANGE_FILES.filter((file) => stagedFiles[file.id]).length;

  useEffect(() => {
    return () => {
      if (historyActionTimeoutRef.current) {
        window.clearTimeout(historyActionTimeoutRef.current);
      }
    };
  }, []);

  const handleToggleStage = (fileId: string) => {
    setStagedFiles((current) => ({
      ...current,
      [fileId]: !current[fileId],
    }));
  };

  const handleStageAll = () => {
    setStagedFiles(Object.fromEntries(GIT_CHANGE_FILES.map((file) => [file.id, true])));
  };

  const handleUnstageAll = () => {
    setStagedFiles(Object.fromEntries(GIT_CHANGE_FILES.map((file) => [file.id, false])));
  };

  const handleGenerateCommitMessage = () => {
    setCommitMessage(
      stagedCount >= 2
        ? "feat(git-panel): align source control workflow with VS Code"
        : "chore(git-panel): update tracked changes and history panel",
    );
  };

  const handleHistoryAction = (action: "fetch" | "pull" | "push" | "refresh") => {
    setActiveHistoryAction(action);

    if (historyActionTimeoutRef.current) {
      window.clearTimeout(historyActionTimeoutRef.current);
    }

    historyActionTimeoutRef.current = window.setTimeout(() => {
      setActiveHistoryAction(null);
      historyActionTimeoutRef.current = null;
    }, 800);
  };

  return (
    <div className="relative flex h-full min-h-0 flex-col px-4 pb-4 pt-3">
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex items-center gap-2">
          <div className="relative min-w-0 flex-1">
            <Input
              value={commitMessage}
              onChange={(event) => setCommitMessage(event.target.value)}
              placeholder="Commit Message"
              aria-label="Commit Message"
              className="h-9 rounded-xl border-app-border bg-transparent px-3 pr-10 text-[13px] font-medium text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0"
            />
            <button
              type="button"
              aria-label="智能生成 Commit Message"
              title="智能生成 Commit Message"
              className="absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={handleGenerateCommitMessage}
            >
              <Sparkles className="size-3.5" />
            </button>
          </div>
          <button
            type="button"
            aria-label="Commit"
            title="Commit"
            className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-border text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
          >
            <Check className="size-4" />
          </button>
        </div>

        <section className="mt-4 flex min-h-0 flex-1 flex-col">
          <div className={DRAWER_SECTION_HEADER_CLASS}>
            <div className="flex items-center gap-2">
              <p className="text-sm font-semibold text-app-foreground">Changes</p>
              <span className="rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
                {GIT_CHANGE_FILES.length}
              </span>
            </div>
            <div className="flex items-center gap-1">
              <button
                type="button"
                aria-label="全部取消"
                title="全部取消"
                className={DRAWER_ICON_ACTION_CLASS}
                onClick={handleUnstageAll}
              >
                <Undo2 className="size-4" />
              </button>
              <button
                type="button"
                aria-label="全部加入"
                title="全部加入"
                className={DRAWER_ICON_ACTION_CLASS}
                onClick={handleStageAll}
              >
                <Plus className="size-4" />
              </button>
            </div>
          </div>

          <div className="mt-2 min-h-0 flex-1 overflow-auto overscroll-contain pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <div className={DRAWER_LIST_STACK_CLASS}>
              {GIT_CHANGE_FILES.map((file) => {
                const isStaged = Boolean(stagedFiles[file.id]);

                return (
                  <div
                    key={file.id}
                    role="button"
                    tabIndex={0}
                    title={file.path}
                    className={cn(
                      "flex cursor-pointer items-center gap-2 hover:bg-app-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-app-border-strong",
                      DRAWER_LIST_ROW_CLASS,
                    )}
                    onClick={() => onOpenDiffPreview(file.id, isStaged)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        onOpenDiffPreview(file.id, isStaged);
                      }
                    }}
                  >
                    <span
                      className={cn(
                        "inline-flex min-w-5 shrink-0 items-center justify-center rounded px-1 text-[10px] font-semibold",
                        file.status === "A"
                          ? "text-app-success"
                          : file.status === "D"
                            ? "text-app-danger"
                            : "text-app-subtle",
                      )}
                    >
                      {file.status}
                    </span>
                    <span className={cn(DRAWER_LIST_LABEL_CLASS, "text-app-muted")}>{file.path.split("/").pop()}</span>
                    <span className={DRAWER_LIST_META_CLASS}>{file.summary}</span>
                    <button
                      type="button"
                      role="checkbox"
                      aria-checked={isStaged}
                      aria-label={isStaged ? `取消暂存 ${file.path}` : `暂存 ${file.path}`}
                      title={isStaged ? "Unstage" : "Stage"}
                      className={cn(
                        "flex size-4 shrink-0 items-center justify-center rounded border shadow-[0_1px_2px_rgba(15,23,42,0.12)] transition-[background-color,border-color,color,box-shadow,transform] duration-200",
                        isStaged
                          ? "border-primary/20 bg-primary/88 text-primary-foreground hover:bg-primary/82 hover:shadow-[0_4px_10px_rgba(15,23,42,0.14)]"
                          : "border-app-border bg-transparent text-transparent hover:border-app-border-strong",
                      )}
                      onClick={(event) => {
                        event.stopPropagation();
                        handleToggleStage(file.id);
                      }}
                    >
                      <Check className="size-2.5" />
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        </section>
      </div>

      <section className="mt-3 flex h-[208px] shrink-0 flex-col">
        <div className={DRAWER_SECTION_HEADER_CLASS}>
          <p className="text-sm font-semibold text-app-foreground">Network</p>
          <div className="flex items-center gap-1">
            <button
              type="button"
              aria-label="Fetch"
              title="Fetch"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("fetch")}
            >
              <Download className={cn("size-4", activeHistoryAction === "fetch" && "animate-pulse")} />
            </button>
            <button
              type="button"
              aria-label="Pull"
              title="Pull"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("pull")}
            >
              <ArrowDownToLine className={cn("size-4", activeHistoryAction === "pull" && "animate-pulse")} />
            </button>
            <button
              type="button"
              aria-label="Push"
              title="Push"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("push")}
            >
              <ArrowUpFromLine className={cn("size-4", activeHistoryAction === "push" && "animate-pulse")} />
            </button>
            <button
              type="button"
              aria-label="刷新历史"
              title="刷新历史"
              className={DRAWER_ICON_ACTION_CLASS}
              onClick={() => handleHistoryAction("refresh")}
            >
              <RefreshCw className={cn("size-4", activeHistoryAction === "refresh" && "animate-spin")} />
            </button>
          </div>
        </div>

        <div className="mt-2.5 min-h-0 flex-1 overflow-auto overscroll-contain pr-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          <div className={DRAWER_LIST_STACK_CLASS}>
            {GIT_HISTORY_ITEMS.map((item, index) => (
              <div key={item.id} className="relative pl-4">
                {item.refs?.includes("HEAD") ? (
                  <span className="absolute inset-y-0 -left-1.5 rounded-lg bg-primary/8" />
                ) : null}
                {index < GIT_HISTORY_ITEMS.length - 1 ? (
                  <span className="absolute left-[4px] top-[18px] h-[calc(100%+0.25rem)] w-px bg-app-border" />
                ) : null}
                <span
                  className={cn(
                    "absolute left-0 top-1/2 size-2.5 -translate-y-1/2 rounded-full border",
                    item.refs?.includes("HEAD")
                      ? "border-primary/30 bg-primary/72 shadow-[0_1px_2px_rgba(15,23,42,0.14)]"
                      : "border-app-border bg-app-drawer",
                  )}
                />
                <div className="relative flex items-center justify-between gap-3 rounded-lg px-2.5 py-1.5">
                  <p className="min-w-0 flex-1 truncate text-[13px] font-medium leading-5 text-app-foreground">{item.subject}</p>
                  {item.refs?.length ? (
                    <div className="flex shrink-0 flex-wrap justify-end gap-1">
                      {item.refs.map((ref) => (
                        <span
                          key={ref}
                          className={cn(
                            "rounded-full px-2 py-1 text-[10px] transition-[background-color,color,box-shadow] duration-200",
                            ref === "HEAD"
                              ? "bg-primary/88 text-primary-foreground shadow-[0_1px_2px_rgba(15,23,42,0.14)]"
                              : "bg-app-surface-muted text-app-muted",
                          )}
                        >
                          {ref}
                        </span>
                      ))}
                    </div>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

    </div>
  );
}

function GitDiffPreviewPanel({
  file,
  isStaged,
  onClose,
}: {
  file: GitChangeFile;
  isStaged: boolean;
  onClose: () => void;
}) {
  const [isMetaExpanded, setMetaExpanded] = useState(false);
  const preview = buildGitDiffPreview(file);
  const splitRows = buildGitSplitDiffRows(file);
  const fileName = file.path.split("/").pop() ?? file.path;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-app-chrome/50 px-6 py-12 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="flex h-[min(82vh,860px)] w-full max-w-7xl flex-col overflow-hidden rounded-[24px] border border-app-border bg-app-surface shadow-[0_32px_96px_rgba(15,23,42,0.28)] dark:shadow-[0_32px_96px_rgba(0,0,0,0.56)]"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-app-border px-5 py-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="shrink-0">
                <ProjectTreeIcon icon={file.icon} />
              </span>
              <p className="truncate text-sm font-semibold text-app-foreground">{fileName}</p>
              <span className="shrink-0 rounded-md bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-subtle">
                {file.summary}
              </span>
              <span
                className={cn(
                  "shrink-0 rounded-md px-1.5 py-0.5 text-[11px]",
                  isStaged ? "bg-app-foreground text-app-drawer" : "bg-app-surface-muted text-app-subtle",
                )}
              >
                {isStaged ? "Staged" : "Unstaged"}
              </span>
            </div>
            <div className="mt-1 flex items-center gap-1">
              <p className="truncate text-[12px] text-app-subtle">{file.path}</p>
              <button
                type="button"
                aria-label={isMetaExpanded ? "折叠 diff 指令信息" : "展开 diff 指令信息"}
                title={isMetaExpanded ? "折叠 diff 指令信息" : "展开 diff 指令信息"}
                className="flex size-5 shrink-0 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                onClick={() => setMetaExpanded((current) => !current)}
              >
                <ChevronDown className={cn("size-3.5 transition-transform", !isMetaExpanded && "-rotate-90")} />
              </button>
            </div>
          </div>

          <button
            type="button"
            aria-label="关闭 Diff 预览"
            title="关闭 Diff 预览"
            className="flex size-8 shrink-0 items-center justify-center rounded-lg text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
            onClick={onClose}
          >
            <CircleX className="size-4" />
          </button>
        </div>

        {isMetaExpanded ? (
          <div className="shrink-0 border-b border-app-border bg-app-surface-muted/70 px-5 py-3 font-mono text-[11px] text-app-subtle">
            {preview.meta.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        ) : null}

        <div className="grid shrink-0 grid-cols-2 border-b border-app-border bg-app-surface-muted/50 text-[11px] uppercase tracking-[0.12em] text-app-subtle">
          <div className="border-r border-app-border px-4 py-2">Old</div>
          <div className="px-4 py-2">New</div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto overscroll-contain bg-app-drawer font-mono text-[12px] leading-6 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {splitRows.map((row, index) => (
            <div key={`${row.kind}-${index}-${row.leftText}-${row.rightText}`} className="grid grid-cols-2 border-b border-app-border/60">
              <div
                className={cn(
                  "grid min-w-0 grid-cols-[56px_1fr] items-start border-r border-app-border/70",
                  row.kind === "remove" || row.kind === "modified"
                    ? "bg-app-danger/10"
                    : "bg-transparent",
                )}
              >
                <span className="select-none border-r border-app-border/60 px-3 text-right text-app-subtle">
                  {row.leftNumber ?? ""}
                </span>
                <span
                  className={cn(
                    "overflow-x-auto px-3 whitespace-pre [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
                    row.kind === "remove" || row.kind === "modified" ? "text-app-danger" : "text-app-foreground",
                  )}
                >
                  {row.leftText}
                </span>
              </div>

              <div
                className={cn(
                  "grid min-w-0 grid-cols-[56px_1fr] items-start",
                  row.kind === "add" || row.kind === "modified"
                    ? "bg-app-success/10"
                    : "bg-transparent",
                )}
              >
                <span className="select-none border-r border-app-border/60 px-3 text-right text-app-subtle">
                  {row.rightNumber ?? ""}
                </span>
                <span
                  className={cn(
                    "overflow-x-auto px-3 whitespace-pre [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
                    row.kind === "add" || row.kind === "modified" ? "text-app-success" : "text-app-foreground",
                  )}
                >
                  {row.rightText}
                </span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function WorkbenchTopBar({
  isMacOS,
  isWindows,
  isSidebarOpen,
  isDrawerOpen,
  isTerminalCollapsed,
  isUserMenuOpen,
  isSettingsOpen,
  isLoggedIn,
  userSession,
  isCheckingUpdates,
  updateStatus,
  openSettingsSection,
  userMenuRef,
  selectedLanguageLabel,
  selectedThemeSummary,
  language,
  theme,
  onToggleUserMenu,
  onLogin,
  onLogout,
  onCheckUpdates,
  onOpenSettings,
  onSelectLanguage,
  onSelectTheme,
  onToggleSettingsSection,
  onToggleSidebar,
  onToggleDrawer,
  onToggleTerminal,
}: {
  isMacOS: boolean;
  isWindows: boolean;
  isSidebarOpen: boolean;
  isDrawerOpen: boolean;
  isTerminalCollapsed: boolean;
  isUserMenuOpen: boolean;
  isSettingsOpen: boolean;
  isLoggedIn: boolean;
  userSession: MockUserSession | null;
  isCheckingUpdates: boolean;
  updateStatus: string | null;
  openSettingsSection: "theme" | "language" | null;
  userMenuRef: { current: HTMLDivElement | null };
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
  language: LanguagePreference;
  theme: ThemePreference;
  onToggleUserMenu: () => void;
  onLogin: () => void;
  onLogout: () => void;
  onCheckUpdates: () => void;
  onOpenSettings: () => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onToggleSettingsSection: React.Dispatch<React.SetStateAction<"theme" | "language" | null>>;
  onToggleSidebar: () => void;
  onToggleDrawer: () => void;
  onToggleTerminal: () => void;
}) {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (!isWindows) return;
    const appWindow = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      setIsMaximized(await appWindow.isMaximized());
      unlisten = await appWindow.onResized(async () => {
        setIsMaximized(await appWindow.isMaximized());
      });
    };
    setup();
    return () => unlisten?.();
  }, [isWindows]);

  const handleWindowMinimize = () => getCurrentWindow().minimize();
  const handleWindowToggleMaximize = () => getCurrentWindow().toggleMaximize();
  const handleWindowClose = () => getCurrentWindow().close();

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
      <div className={cn("grid h-full grid-cols-[auto_1fr_auto] items-center gap-2 px-2.5", isWindows && "pr-0")}>
        <div className={cn("relative z-10 flex h-full shrink-0 items-center", isMacOS ? "w-[150px]" : "w-[132px]")} ref={userMenuRef}>
          <Button
            size="icon"
            variant="ghost"
            className={cn(
              "size-7 rounded-full text-app-subtle transition-[color,background-color,border-color] duration-200 hover:bg-app-surface-hover hover:text-app-foreground",
              isMacOS ? MAC_USER_MENU_OFFSET : "ml-2",
              isSettingsOpen && "pointer-events-none invisible",
              isUserMenuOpen && "bg-app-surface-hover text-app-foreground",
            )}
            aria-label={isLoggedIn ? "打开用户菜单" : "打开登录菜单"}
            title={isLoggedIn ? "打开用户菜单" : "打开登录菜单"}
            aria-expanded={isUserMenuOpen}
            aria-haspopup="menu"
            onClick={onToggleUserMenu}
          >
            <CircleUserRound className="size-4" />
          </Button>

          {isUserMenuOpen ? (
            <div className={cn("absolute left-0 top-full z-30 mt-2 w-[248px] rounded-2xl border border-app-border bg-app-menu p-1.5 shadow-[0_20px_48px_rgba(15,23,42,0.18)] dark:shadow-[0_20px_48px_rgba(0,0,0,0.42)]", isMacOS ? MAC_USER_MENU_POPOVER_OFFSET : "left-2")}>
              {userSession ? (
                <div className="mb-1 flex items-center gap-3 rounded-xl px-3 py-3 text-left">
                  <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-app-surface-active text-sm font-semibold text-app-foreground">
                    {userSession.avatar}
                  </div>
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium text-app-foreground">{userSession.name}</p>
                    <p className="truncate text-xs text-app-subtle">{userSession.email}</p>
                  </div>
                </div>
              ) : null}

              <button
                type="button"
                className={cn(
                  MENU_TRIGGER_CLASS,
                  "text-app-foreground",
                )}
                aria-expanded={openSettingsSection === "theme"}
                onClick={() => onToggleSettingsSection((current) => (current === "theme" ? null : "theme"))}
              >
                <Palette className={MENU_TRIGGER_ICON_CLASS} />
                <span className={MENU_TRIGGER_LABEL_CLASS}>主题</span>
                <span className="shrink-0 text-xs text-app-subtle">{selectedThemeSummary}</span>
              </button>

              {openSettingsSection === "theme" ? (
                <div className={MENU_SUBMENU_GROUP_CLASS}>
                  <div className="space-y-0.5">
                  {THEME_OPTIONS.map((option) => {
                    const OptionIcon = option.icon;
                    const isSelected = theme === option.value;

                    return (
                      <button
                        key={option.value}
                        type="button"
                        className={cn(
                          MENU_SUBMENU_OPTION_CLASS,
                          isSelected
                            ? "bg-app-surface-hover/80 text-app-foreground"
                            : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                        )}
                        onClick={() => onSelectTheme(option.value)}
                      >
                        <OptionIcon className={MENU_SUBMENU_ICON_CLASS} />
                        <span className={MENU_SUBMENU_LABEL_CLASS}>{option.label}</span>
                        {isSelected ? <Check className="size-3.5 shrink-0 text-app-foreground" /> : null}
                      </button>
                    );
                  })}
                  </div>
                </div>
              ) : null}

              <button
                type="button"
                className={cn(
                  MENU_TRIGGER_CLASS,
                  "mt-1 text-app-foreground",
                )}
                aria-expanded={openSettingsSection === "language"}
                onClick={() => onToggleSettingsSection((current) => (current === "language" ? null : "language"))}
              >
                <Globe className={MENU_TRIGGER_ICON_CLASS} />
                <span className={MENU_TRIGGER_LABEL_CLASS}>语言</span>
                <span className="shrink-0 text-xs text-app-subtle">{selectedLanguageLabel}</span>
              </button>

              {openSettingsSection === "language" ? (
                <div className={MENU_SUBMENU_GROUP_CLASS}>
                  <div className="space-y-0.5">
                  {LANGUAGE_OPTIONS.map((option) => {
                    const isSelected = language === option.value;
                    const OptionIcon = option.icon;

                    return (
                      <button
                        key={option.value}
                        type="button"
                        className={cn(
                          MENU_SUBMENU_OPTION_CLASS,
                          isSelected
                            ? "bg-app-surface-hover/80 text-app-foreground"
                            : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                        )}
                        onClick={() => onSelectLanguage(option.value)}
                      >
                        <OptionIcon className={MENU_SUBMENU_ICON_CLASS} />
                        <span className={MENU_SUBMENU_LABEL_CLASS}>{option.label}</span>
                        {isSelected ? <Check className="size-3.5 shrink-0 text-app-foreground" /> : null}
                      </button>
                    );
                  })}
                  </div>
                </div>
              ) : null}

              <button
                type="button"
                className={cn(
                  MENU_TRIGGER_CLASS,
                  "mt-1 text-app-foreground",
                  isCheckingUpdates && "cursor-wait",
                )}
                onClick={onCheckUpdates}
              >
                {isCheckingUpdates ? (
                  <LoaderCircle className={cn(MENU_TRIGGER_ICON_CLASS, "animate-spin")} />
                ) : (
                  <RefreshCw className={MENU_TRIGGER_ICON_CLASS} />
                )}
                <span className={MENU_TRIGGER_LABEL_CLASS}>检查更新</span>
              </button>

              <button
                type="button"
                className={cn(
                  MENU_TRIGGER_CLASS,
                  "mt-1 text-app-foreground",
                  isSettingsOpen && "bg-app-surface-hover",
                )}
                onClick={onOpenSettings}
              >
                <MoreHorizontal className={MENU_TRIGGER_ICON_CLASS} />
                <span className={MENU_TRIGGER_LABEL_CLASS}>更多设置</span>
              </button>

              {userSession ? (
                <button
                  type="button"
                  className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground")}
                  onClick={onLogout}
                >
                  <LogOut className={MENU_TRIGGER_ICON_CLASS} />
                  <span className={MENU_TRIGGER_LABEL_CLASS}>退出登录</span>
                </button>
              ) : (
                <button
                  type="button"
                  className={cn(MENU_TRIGGER_CLASS, "mt-1 text-app-foreground")}
                  onClick={onLogin}
                >
                  <LogIn className={MENU_TRIGGER_ICON_CLASS} />
                  <span className={MENU_TRIGGER_LABEL_CLASS}>登录</span>
                </button>
              )}

              {updateStatus ? (
                <div className="px-3 pb-1 pt-2 text-xs text-app-subtle">{updateStatus}</div>
              ) : null}
            </div>
          ) : null}
        </div>

        <div
          className="relative z-10 flex h-full items-center justify-center"
          data-tauri-drag-region=""
          onMouseDown={handleTitleBarMouseDown}
        >
          <img src="/app-icon.png" alt="" className="mr-1.5 size-4 shrink-0 select-none" draggable={false} data-tauri-drag-region="" />
          <span className="select-none text-[13px] font-semibold tracking-[0.02em] text-app-foreground" data-tauri-drag-region="">Tiy Agent</span>
        </div>

        <div className="relative z-10 flex items-center justify-end gap-0.5">
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isSidebarOpen && "text-app-foreground", isSettingsOpen && "pointer-events-none invisible")}
            aria-label={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            title={isSidebarOpen ? "收拢 sidebar" : "展开 sidebar"}
            onClick={onToggleSidebar}
          >
            <PanelLeft className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, !isTerminalCollapsed && "text-app-foreground", isSettingsOpen && "pointer-events-none invisible")}
            aria-label={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            title={isTerminalCollapsed ? "展开 terminal 面板" : "收起 terminal 面板"}
            onClick={onToggleTerminal}
          >
            <PanelBottom className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className={cn(panelToggleButtonClass, isDrawerOpen && "text-app-foreground", isSettingsOpen && "pointer-events-none invisible")}
            aria-label={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            title={isDrawerOpen ? "收拢右侧面板" : "展开右侧面板"}
            onClick={onToggleDrawer}
          >
            <PanelRight className="size-4" />
          </Button>

          {isWindows && (
            <>
              <div className="mx-1 h-4 w-px bg-app-border" />
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                aria-label="最小化"
                title="最小化"
                onClick={handleWindowMinimize}
              >
                <Minus className="size-3.5" />
              </button>
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                aria-label={isMaximized ? "还原" : "最大化"}
                title={isMaximized ? "还原" : "最大化"}
                onClick={handleWindowToggleMaximize}
              >
                {isMaximized ? <Copy className="size-3" /> : <Square className="size-3" />}
              </button>
              <button
                type="button"
                className="flex size-7 items-center justify-center text-app-subtle transition-colors hover:bg-red-500/90 hover:text-white"
                aria-label="关闭"
                title="关闭"
                onClick={handleWindowClose}
              >
                <X className="size-3.5" />
              </button>
            </>
          )}
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
