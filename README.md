# TiyCode

TiyCode 是一款开源、自由、便利的跨平台 Vibe-Coding Agent。

项目当前基于 `Tauri 2 + TypeScript + React + shadcn/ui` 构建。

## 启动

```bash
npm install
npm run dev
```

## 构建

```bash
npm run build
```

## 目录结构

```text
src/
  app/       # 入口、Provider、路由、全局样式
  modules/   # 工作台、设置中心、扩展中心、项目树、Git 等领域模块
  pages/     # 页面级组装
  features/  # 与运行时/平台强相关的轻量能力模块
  shared/    # 共享 UI / lib / types / config
src-tauri/
  src/commands/   # Rust 命令模块
  src/extensions/ # 扩展宿主、注册表与插件/MCP/Skill 运行时
```

## 当前脚手架包含

- Tauri 2 官方 `react-ts` 模板为基础
- React Hash Router
- Tailwind CSS v4
- shadcn/ui 基础配置与通用组件
- 内置主题系统（跟随系统 / 明亮 / 暗黑）与运行时切换
- 内置工作台设置中心，包含 `Account / General / Providers / Commands / Permissions / Workspace / About` 分类页与本地持久化
- 内置基于 AI Elements 原生组件的 existing-thread Demo，覆盖 `Plan / Queue / Reasoning / Chain of Thought / Tool / Confirmation / Sources / Suggestion / PromptInput`，并支持基于 Settings 的 Profile 切换
- Agent 运行时支持通过 `clarify` 在执行过程中发起结构化提问：当需求存在不确定信息时，可展示推荐选项供用户点选，或直接在 composer 中补充自由输入后继续当前 run
- 内置 `Extensions Center` 全屏浮层，统一承载 `Plugins / MCP / Skills / Marketplace / Activity` 五个 tab；其中 Rust 侧已提供扩展注册、详情、启停、活动流、插件导入、MCP 配置与技能索引 IPC
- Git Drawer 已接入真实仓库状态、Diff、History，并支持 `stage / unstage / commit / fetch / pull / push`；其中 `commit / fetch / pull / push` 依赖本地 Git CLI，缺失时会自动降级为只读
- Agent Run 主链路已切换为内置 Rust runtime：`Frontend -> Rust Core -> BuiltInAgentRuntime -> tiy-core -> LLM`，不再依赖独立 sidecar 进程
- Rust 端示例命令 `get_system_metadata`

## Visual System

当前项目的视觉规范已整理到 `docs/design-spec.md`。

- 该文档是工作台布局、主题 token、组件语气、动效状态和实施约定的主要维护位置。
- README 仅保留项目概览，避免出现两份重复规范。

## 常用脚本

```bash
npm run dev       # 启动完整 Tauri 桌面应用
npm run dev:web   # 仅启动前端 Vite
npm run build     # 构建桌面应用
npm run build:web # 构建前端资源
npm run typecheck # TypeScript 类型检查
```
