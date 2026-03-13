# Tiy Agent

一个基于 `Tauri 2 + TypeScript + React + shadcn/ui` 的跨平台桌面应用脚手架。

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
  modules/   # 工作台、设置中心、项目树、Git 等领域模块
  pages/     # 页面级组装
  features/  # 与运行时/平台强相关的轻量能力模块
  shared/    # 共享 UI / lib / types / config
src-tauri/
  src/commands/ # Rust 命令模块
```

## 当前脚手架包含

- Tauri 2 官方 `react-ts` 模板为基础
- React Hash Router
- Tailwind CSS v4
- shadcn/ui 基础配置与通用组件
- 内置主题系统（跟随系统 / 明亮 / 暗黑）与运行时切换
- 内置工作台设置中心，包含 `Account / General / Providers / Commands / Permissions / Workspace / About` 分类页与本地持久化
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
