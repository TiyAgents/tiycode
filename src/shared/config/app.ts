export const APP_MODULES = [
  {
    name: "app",
    path: "src/app",
    description: "应用入口、全局 Provider、路由与样式配置。",
  },
  {
    name: "pages",
    path: "src/pages",
    description: "页面级组装层，只做页面编排，不承载复杂业务。",
  },
  {
    name: "features",
    path: "src/features",
    description: "按能力聚合业务逻辑，例如系统信息、文件管理、自动更新。",
  },
  {
    name: "shared",
    path: "src/shared",
    description: "跨模块复用的 UI、工具、类型、配置与基础设施。",
  },
  {
    name: "src-tauri/src/commands",
    path: "src-tauri/src/commands",
    description: "Rust 命令按领域拆分，避免后续所有桌面能力堆在一个文件。",
  },
] as const;

export const BOOTSTRAP_NEXT_STEPS = [
  {
    order: "01",
    title: "定义业务域",
    description: "把核心能力拆进 features/*，例如 auth、workspace、sync、settings。",
  },
  {
    order: "02",
    title: "补状态层",
    description: "根据复杂度选择 Zustand 或 TanStack Query，而不是一开始就全局堆状态。",
  },
  {
    order: "03",
    title: "接入原生能力",
    description: "在 Rust commands 中继续扩展文件系统、系统托盘、窗口管理等能力。",
  },
  {
    order: "04",
    title: "沉淀设计系统",
    description: "继续用 shadcn/ui 生成业务组件，并统一 token、布局和交互规范。",
  },
] as const;
