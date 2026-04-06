<div align="center">
  <img src="./public/app-icon.png" alt="TiyCode 标志" width="120" />
  <h1>TiyCode</h1>
  <p><strong>一款开源、灵活、便利的跨平台 vibe-coding agent。</strong></p>
  <p>TiyCode 是一个基于 Tauri、React、TypeScript 与 Rust 构建的桌面 AI 工作台。它把线程式 Agent 运行、工作区感知工具、终端与 Git 集成、设置管理以及可扩展运行时放进同一个本地优先的桌面应用里。</p>
  <p>
    <a href="./README.md">English</a>
  </p>
</div>

## 为什么是 TiyCode

TiyCode 面向的是希望获得“AI 编码工作台”而不是“单一聊天框”的用户。这个项目强调本地桌面体验，让 Agent 对话、工具执行、终端流程、Git 操作和扩展能力在同一个界面中协同工作。

当前仓库最适合以**源码优先的桌面应用**来理解和使用。也就是说，现阶段的主要使用方式是从源码运行、阅读架构设计，并在现有工作台、运行时和扩展宿主的基础上继续开发。

## 核心亮点

- **桌面优先的 AI 工作台。** 应用基于 Tauri 2、React + TypeScript 前端和 Rust 核心构建，因此界面能够自然接入终端会话、仓库状态读取和工作区作用域工具等本地能力。
- **内置 Agent Runtime。** 主执行链路已经收敛为 `Frontend -> Rust Core -> BuiltInAgentRuntime -> tiycore -> LLM`，不再依赖单独的 sidecar 进程。
- **运行中结构化澄清。** 运行时支持 `clarify` 步骤，Agent 在缺少关键信息时可以中途暂停、向用户发问，并提供推荐选项后继续执行。
- **内建 Git 与终端工作流。** 工作台已经整合真实仓库状态、Diff、History 视图和终端能力；当本地 Git CLI 不可用时，部分写操作会自动降级。
- **统一扩展中心。** 应用内置 `Extensions Center`，把 Plugins、MCP、Skills、Marketplace 和 Activity 放到同一入口中统一管理。
- **面向 Agent 线程的 UI 基础。** 前端已经接入基于 AI Elements 风格的线程组件，包括 plan、queue、reasoning、tool call、confirmation、sources、suggestion 和 prompt input 等界面单元。

## 技术栈

- **桌面壳层：** Tauri 2
- **前端：** React 19、TypeScript、Vite
- **后端 / 原生核心：** Rust
- **AI Runtime：** `tiycore`
- **UI 基础：** Tailwind CSS v4、shadcn/ui、AI Elements 风格线程组件
- **持久化：** SQLite

## 快速开始

> [!IMPORTANT]
> 当前仓库明确适用于源码运行场景，仓库内尚未提供已验证的打包分发安装路径说明。

### 环境准备

在启动项目前，请先准备好一个可以运行 Tauri 2 工程的开发环境：

- Node.js 和 npm
- Rust toolchain
- Tauri 所需的平台依赖

### 开发模式启动

```bash
npm install
npm run dev
```

### 仅启动 Web 前端

```bash
npm install
npm run dev:web
```

### 构建桌面应用

```bash
npm run build
```

### 前端类型检查

```bash
npm run typecheck
```

### 运行 Rust 测试

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

## 架构速览

TiyCode 将界面渲染、桌面编排和 Agent 执行拆分为清晰的几层：

```mermaid
flowchart LR
  UI[React + TypeScript UI] --> TAURI[Tauri Rust Core]
  TAURI --> RUNTIME[BuiltInAgentRuntime]
  RUNTIME --> CORE[tiycore]
  TAURI --> TOOLS[Workspace / Git / Terminal / Extensions]
  CORE --> LLM[LLM Providers]
```

可以按下面的方式理解：

1. **React UI** 负责工作台渲染、线程交互和流式事件展示。
2. **Rust Core** 是系统访问、策略裁决、持久化以及本地高性能任务的真源。
3. **Built-in Runtime** 负责 agent session、helper 编排、tool profile 和事件折叠。
4. **Extension Host** 负责把 plugin、MCP 和 skill 能力接入桌面产品模型。

## 仓库结构

```text
src/
  app/         应用启动、路由、Provider 与全局样式
  modules/     工作台、设置、市场、扩展中心等领域模块
  features/    终端、系统元数据等平台侧能力模块
  shared/      可复用 UI、工具函数、配置与共享类型
  services/    bridge 与流式服务集成
src-tauri/
  src/commands/    Rust 命令模块
  src/extensions/  扩展宿主、注册表与运行时接缝
  migrations/      数据库迁移
  tests/           Rust 集成测试
public/            静态资源
```

## 开发命令

```bash
npm run dev        # 启动完整 Tauri 桌面应用
npm run dev:web    # 仅启动 Vite 前端
npm run build      # 构建桌面应用
npm run build:web  # 类型检查并打包 Web 资源
npm run typecheck  # 执行 TypeScript 校验
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml
```

## 扩展模型

TiyCode 将可扩展性作为桌面工作台的一等能力来设计。

- **Plugins** 提供本地安装的扩展包，可携带 hooks、tools、commands 和 skill packs。
- **MCP** 在产品层被视为独立扩展类型，并由 Rust 侧宿主管理。
- **Skills** 作为可复用的 Agent 能力资产，可以来自 builtin、workspace 或 plugin。

这些能力会统一呈现在 `Extensions Center` 中，但运行时访问仍然会经过宿主侧的 tool gateway、policy check、approval 和 audit 边界治理。

## 当前项目状态

这个仓库已经具备较完整的桌面壳层、工作台 UI、设置中心、内置运行时主链路、Git Drawer 和扩展体系设计。但与此同时，它更适合被理解为一个持续演进中的开源项目，而不是一个已经完成终端用户打包分发说明的成熟发布版产品。

因此，当前最适合的使用方式是：

1. 评估项目的产品方向与技术架构。
2. 从源码本地运行桌面应用。
3. 作为贡献者继续扩展工作台、运行时或扩展系统。

## License

本项目采用 Apache License 2.0 开源协议。详细信息请见 `LICENSE`。

## 致敬

本项目的诞生受到了以下项目和产品的启发，在此一并致谢：

- [pi-mono](https://github.com/badlogic/pi-mono)
- [nanobot](https://github.com/HKUDS/nanobot)
- Codex
- ClaudeCode
