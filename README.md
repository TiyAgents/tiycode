# Tiy Agent

一个基于 \`Tauri 2 + TypeScript + React + shadcn/ui\` 的跨平台桌面应用脚手架。

## 启动

\`\`\`bash
npm install
npm run dev:app
\`\`\`

## 构建

\`\`\`bash
npm run build:app
\`\`\`

## 目录结构

\`\`\`text
src/
  app/       # 入口、Provider、路由、全局样式
  pages/     # 页面级组装
  features/  # 业务能力模块
  shared/    # 共享 UI / lib / types / config
src-tauri/
  src/commands/ # Rust 命令模块
\`\`\`

## 当前脚手架包含

- Tauri 2 官方 \`react-ts\` 模板为基础
- React Hash Router
- Tailwind CSS v4
- shadcn/ui 基础配置与通用组件
- Rust 端示例命令 \`get_system_metadata\`

## 常用脚本

```bash
npm run dev:web   # 仅启动前端 Vite
npm run dev:app   # 启动完整 Tauri 桌面应用
npm run build:web # 构建前端资源
npm run build:app # 构建桌面应用
```
