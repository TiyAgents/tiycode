# Tiy Agent 视觉规范

## 1. 文档目的

本规范用于沉淀 Tiy Agent 当前桌面工作台的视觉语言、设计约束和实现边界。它不是一份抽象品牌手册，而是一份直接指导界面实现的工程规范。

当前阶段的核心目标只有一个：

- 让桌面工作台在高信息密度下仍然保持清晰、克制、稳定和可扩展。

## 2. 适用范围

本规范覆盖以下内容：

- 主工作台布局
- 主题与设计 token
- 侧边栏、抽屉、终端、菜单、卡片、输入框等核心容器
- 线程状态、项目选择器等高频交互模块
- 动效、状态反馈和可访问性基线

本规范当前基于以下实现整理：

- `src/app/styles/globals.css`
- `src/app/providers/theme-provider.tsx`
- `src/widgets/dashboard-overview/ui/dashboard-overview.tsx`
- `src/shared/ui/button.tsx`
- `src/shared/ui/card.tsx`
- `src/shared/ui/input.tsx`

## 3. 视觉基调

### 3.1 产品气质

Tiy Agent 的界面不是营销页，也不是移动端卡片流，而是桌面型 AI 工作台。

视觉上应保持以下特征：

- 冷静而克制，不依赖高饱和品牌色制造存在感
- 高密度但不拥挤，信息层次靠结构和对比建立
- 更像专业工具，而不是娱乐型聊天应用
- 反馈明确，但不过度表演

### 3.2 风格关键词

- Desktop workbench
- Cool neutral
- Dense but breathable
- Quiet hierarchy
- Soft glass, not heavy skeuomorphism

## 4. 已收敛的设计问题

本次梳理重点修正了两类不合理设计：

### 4.1 线程状态缺少语义区分

之前 `running / completed / needs-reply / failed` 的图标色几乎一致，用户需要依赖图标形状才能识别状态，语义反馈偏弱。

现在的规范要求：

- 高显著场景中，`running / completed / needs-reply / failed` 可分别映射到 `app-info / app-success / app-warning / app-danger`
- 在 sidebar 这类高密度列表中，状态图标默认应降为低彩中性表达
- 只有当前项、关键反馈项或失败项才允许适度提权颜色

状态色只能用于语义提示，不能反向污染大面积布局层级，也不能让列表出现"彩色糖豆"堆积感。

### 4.2 新线程页项目选择器信息层级不足

之前项目切换只显示项目名，缺少路径与最近打开信息，项目来源感不足，难以快速判断当前线程将绑定哪个工作区。

现在的规范要求：

- 触发器必须同时展示项目名与路径摘要
- 最近项目列表必须展示：
  - 项目名
  - 路径摘要
  - 最近打开时间
  - 当前选中状态
- "选择新文件夹"属于和"最近项目"平级但语义不同的入口，需要独立分组

### 4.3 用户菜单层级表达过重或过平

用户菜单中的一级入口和二级选项不能长得几乎一样，也不能靠额外卡片把层级硬切出来。

之前出现过两类问题：

- 一级菜单与二级选项尺寸、图标、文字节奏太接近，展开后不容易快速分辨"入口"和"具体选项"
- 为了补层级又额外套了一层框、阴影和标签，导致菜单局部过重，破坏工作台应有的克制感

现在的规范要求：

- 一级菜单负责"进入某个设置域"，二级选项负责"选择该设置域下的具体值"
- 二级选项必须通过更轻的密度、轻微缩进和弱分隔来表达从属关系
- 不允许为了解决层级问题而默认引入额外卡片、重阴影或过多装饰标签
- 菜单层级应优先依靠字号、间距、图标尺寸和选中反馈区分，而不是新增容器层

## 5. 布局系统

### 5.1 工作台骨架

应用采用固定视口工作台，不允许页面级滚动。

- 顶部系统栏高度：`36px`
- 左侧 sidebar：`320px`
- 右侧 drawer：`360px`
- 底部 terminal 默认高度：`260px`
- terminal 最小高度：`180px`

实现原则：

- `html`、`body`、`#root` 必须保持全高
- 全局 `overflow` 必须禁用
- 只允许局部容器滚动
- 面板收起/展开应通过局部过渡完成，不得引入页面抖动

### 5.2 内容组织

主工作台不采用营销网站常见的 12 栏栅格，而采用更实用的局部结构：

- 大框架用 `flex`
- 垂直内容组织用 `space-y-*`
- 信息卡片或 inspector 区块用局部 `grid`

主阅读区基线：

- 中心阅读列使用 `max-w-4xl`
- 单列为默认
- 在 `md` 扩展为两列
- 在 `xl` 扩展为三列

## 6. 主题与 Token 体系

### 6.1 主题模型

主题偏好支持：

- `system`
- `light`
- `dark`

运行时通过以下方式同步主题：

- `html.dark`
- `data-theme`
- `color-scheme`

### 6.2 Token 分层

当前 token 分为两层：

#### 通用语义层（shadcn/ui 基础）

用于通用组件库的基础样式：

- `background`
- `foreground`
- `card`
- `primary`
- `border`
- `muted`
- `destructive`

⚠️ **重要**：工作台 UI 组件**不应直接使用**这些 token，而应使用下方工作台语义层的 `app-*` token。

#### 工作台语义层（应用特定）

工作台专用 token，所有工作台界面必须使用这些 token：

| Token | 用途 |
|-------|------|
| `app-canvas` | 整体工作台背景 |
| `app-sidebar` | 侧边栏背景 |
| `app-drawer` | 右侧面板背景 |
| `app-chrome` | 系统栏/标题栏背景 |
| `app-terminal` | 终端面板背景 |
| `app-menu` | 浮层菜单背景 |
| `app-surface` | 主要内容块背景 |
| `app-surface-muted` | 次级承载面背景 |
| `app-surface-hover` | 悬停状态背景 |
| `app-surface-active` | 激活状态背景 |
| `app-border` | 默认边框 |
| `app-border-strong` | 强调边框 |
| `app-foreground` | 主要文字 |
| `app-muted` | 次要文字 |
| `app-subtle` | 微弱文字 |
| `app-code` | 代码块背景 |
| `app-overlay` | 遮罩层 |
| `app-success` | 成功状态 |
| `app-warning` | 警告状态 |
| `app-danger` | 危险/错误状态 |
| `app-info` | 信息/运行中状态 |

### 6.3 色彩原则

- 主层级由冷灰蓝中性色构成，不靠品牌色分层
- `primary` 只用于高权重操作，不应用于大面积装饰
- 功能状态色仅用于反馈，不参与主布局分区
- 所有新颜色优先进入 token，禁止在业务组件中继续扩散硬编码 OKLCH

### 6.4 层级关系

容器层次从外到内应保持以下关系：

1. `app-canvas`：整体工作台背景
2. `app-sidebar` / `app-drawer` / `app-terminal`：结构层
3. `app-surface`：主要内容块
4. `app-surface-muted`：次级承载面
5. `app-menu`：浮层和菜单
6. `app-overlay`：遮罩与底部渐隐

原则：

- 同一层级内不要同时使用过多边框、阴影和高亮
- 通过"底色差 + 边框强弱 + 选中态"来建立层级
- 弱化大面积品牌色，依靠中性色体系完成 90% 的视觉分区

## 7. 字体与排版

### 7.1 字体栈

基线字体栈：

```css
font-family: Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
```

等宽字体（代码、路径、哈希）：

```css
font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Monaco, Consolas, monospace;
```

### 7.2 字号层级

| 层级 | 字号 | 用途 |
|------|------|------|
| 大标题 | `19px` | 设置页面标题 |
| 中标题 | `15px` | 区块标题、工作区名 |
| 正文 | `13px` | 列表项、按钮、输入框 |
| 辅助 | `12px` | 描述、标签、时间戳 |
| 微小 | `11px` | 分类标签、元信息 |

### 7.3 密度原则

这是桌面工具，不是移动端内容卡片。

因此：

- 常规控件高度以 `h-8` 到 `h-9` 为主
- 图标按钮以 `size-6`、`size-7`、`size-8` 为主
- 默认优先选择"紧凑但可点"的方案
- 空白要服务分组，不要为了"轻盈感"无节制放大

### 7.4 圆角

圆角基线：

- 根 token：`--radius: 0.75rem`
- 常用圆角：`rounded-lg`、`rounded-xl`、`rounded-2xl`

规则：

- 列表项通常使用 `rounded-lg`
- 承载性卡片、菜单和模块容器通常使用 `rounded-xl` 或 `rounded-2xl`
- 避免在同一区域同时出现过多不相关的大圆角，防止界面"糯化"

## 8. 组件规范

### 8.1 Token 使用规则

#### ✅ 正确用法

工作台组件必须使用 `app-*` token：

```tsx
// 正确 - 工作台卡片
<div className="bg-app-surface border-app-border rounded-xl ..." />

// 正确 - 工作台输入框
<input className="bg-app-surface-muted border-app-border ..." />

// 正确 - 工作台按钮
<button className="hover:bg-app-surface-hover text-app-foreground ..." />
```

#### ❌ 错误用法

工作台组件不应直接使用 shadcn 基础 token：

```tsx
// 错误 - 使用了通用 token 而非工作台 token
<div className="bg-card text-card-foreground ..." />

// 错误 - 使用了非工作台 border token
<input className="border-input bg-transparent ..." />
```

### 8.2 Button

共享 Button 保持 shadcn 风格基础能力，但工作台使用时需偏向工具界面语气。

要求：

- 默认保留 hover、focus-visible、disabled 三类状态
- 在工作台中优先使用 `ghost`、`outline` 或轻量定制样式
- 强主按钮只出现在真正的提交、发送、确认动作上

工作台按钮样式指南：

```tsx
// 图标按钮
<Button 
  size="icon" 
  variant="ghost" 
  className="size-7 text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground"
>
  <Icon className="size-4" />
</Button>

// 工具栏按钮
<Button
  variant="ghost"
  className="h-8 gap-2 px-2 text-[13px] text-app-muted hover:bg-app-surface-hover hover:text-app-foreground"
>
  <Icon className="size-4" />
  <span>标签</span>
</Button>
```

不推荐：

- 对普通工具按钮使用高饱和实底
- 在次级操作上加入过强阴影

### 8.3 Card

Card 是主要信息承载容器，但不应该产生"营销卡片感"。

要求：

- 默认组合：`bg-app-surface + border-app-border`
- 阴影可无，或只保留极弱阴影
- 内边距需要服务信息密度，不得盲目放大

工作台卡片规范：

```tsx
// 标准卡片
<div className="rounded-xl border border-app-border bg-app-surface ...">
  <div className="p-4">...</div>
</div>

// 次级卡片
<div className="rounded-xl border border-app-border bg-app-surface-muted ...">
  <div className="p-3">...</div>
</div>
```

### 8.4 Input / Textarea / Composer

输入类组件应与工作台背景自然融合。

要求：

- 使用透明或半透明背景
- 保留明确但不过度夸张的 focus 状态
- placeholder 必须弱于正文
- 输入区应优先呈现"嵌入式工具感"，而不是"浮在页面上的表单"

工作台输入框规范：

```tsx
// 输入框
<input 
  className="h-9 rounded-lg border border-app-border bg-app-surface-muted px-3 text-[13px] text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0 ..." 
/>

// 文本域
textarea 
  className="min-h-16 rounded-lg border border-app-border bg-app-surface-muted px-3 py-2 text-[13px] text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0 ..."
/>
```

### 8.5 Menu / Popover

浮层菜单必须是当前工作台视觉体系的一部分，而不是浏览器原生感的漂浮块。

要求：

- 使用 `app-menu`
- 允许使用轻微 `backdrop-blur`
- 菜单内部必须有清晰的区块分隔
- 每个列表项应具备主信息和必要的次信息层级

针对设置类用户菜单，必须额外满足：

- 一级菜单项使用统一的 trigger 基线：
  - 左侧 `size-4` 图标
  - 主文本 `14px`
  - 纵向内边距维持在 `py-2.5`
- 一级菜单可以带右侧摘要信息，但摘要只能是弱化的次级信息，不能和主标签争抢层级
- 二级菜单项必须明显轻于一级菜单：
  - 图标建议降到 `size-3.5`
  - 文本建议降到 `13px`
  - 行高与内边距应比一级菜单更紧
- 二级菜单的从属关系优先通过以下方式建立：
  - `ml-*` 级别的轻微缩进
  - 一条弱对比度的竖向分隔线
  - 更克制的 hover / selected 背景
- 二级菜单禁止默认包进额外"卡片框"中；除非信息量显著增加，否则不应叠加边框、阴影、标题条和胶囊标签
- 当前选中项优先使用简洁 check 标记或轻量背景，不使用高饱和品牌色，不把选中态做成新的视觉主角

项目选择器属于标准范式：

- 触发器：主标题 + 次级路径摘要 + 当前状态
- 列表项：图标容器 + 项目名 + 路径 + 最近打开时间
- 当前项：使用轻量背景高亮，不使用强品牌色填充

### 8.6 状态指示器

线程状态指示器必须同时满足：

- 语义明确，但不抢夺列表层级
- 面积小，不喧宾夺主
- 在亮色和暗色主题下都清晰

推荐样式：

```tsx
// 状态图标容器
<div className={cn(
  "flex size-5 items-center justify-center rounded",
  status === "running" && "bg-app-info/15 text-app-info",
  status === "completed" && "bg-app-success/15 text-app-success",
  status === "needs-reply" && "bg-app-warning/15 text-app-warning", 
  status === "failed" && "bg-app-danger/15 text-app-danger",
)}>
  <StatusIcon className="size-3" />
</div>
```

高密度列表默认使用低彩中性色：

```tsx
// 列表中的状态（低彩）
<div className="text-app-subtle">
  <StatusIcon className="size-3.5" />
</div>
```

### 8.7 Switch / Toggle

开关组件在工作台中的使用：

```tsx
// Switch 应使用 app-* token 保持一致性
<Switch 
  size="sm"
  aria-label="描述文字"
  checked={value}
  onCheckedChange={onChange}
/>
```

**注意**：不要覆盖 Switch 的 className，组件内部已使用正确的 `app-*` token。

### 8.8 SegmentedControl

分段控制器规范：

```tsx
// 使用 app-* token
<div className="flex items-center gap-1 rounded-xl border border-app-border bg-app-surface-muted p-0.5">
  {options.map(option => (
    <button
      className={cn(
        "flex h-8 flex-1 items-center justify-center rounded-lg px-3.5 text-[12px] transition-colors",
        "hover:bg-app-surface-hover hover:text-app-foreground",
        isSelected && "bg-app-surface text-app-foreground shadow-[0_1px_2px_rgba(15,23,42,0.08)]"
      )}
    >
      {label}
    </button>
  ))}
</div>
```

## 9. 交互与动效

### 9.1 动效基线

标准过渡参数：

- duration：`300ms`
- easing：`cubic-bezier(0.22, 1, 0.36, 1)`

适用对象：

- sidebar / drawer 宽度切换
- terminal 高度变化
- 菜单展开
- hover 与选中态颜色切换

### 9.2 动效原则

- 更强调稳定和可读，而不是弹跳和表演
- hover 比 click 动画更重要
- 不允许在高频列表中引入抢注意力的位移动画

### 9.3 状态层级

交互优先级应按以下顺序表达：

1. hover：轻微提亮、边框增强、文字变清晰
2. active/current：更实的背景或边框
3. focus-visible：清晰、连续、不被遮挡
4. disabled：降低对比，但仍可辨识

## 10. 可访问性基线

必须满足：

- 键盘可见焦点
- 列表项和图标按钮具有足够点击面积
- 文字与背景保持稳定对比
- 仅靠颜色无法完成状态识别时，必须辅以图标或文案

对于菜单、线程状态、项目选择器这类组件，颜色只能强化语义，不能独立承担语义。

## 11. 已修复的设计缺陷

### 11.1 Token 使用不一致 ✅ 已修复

以下组件已更新为使用 `app-*` token：

| 组件 | 修复内容 | 状态 |
|------|----------|------|
| `Card` | `bg-card` → `bg-app-surface` | ✅ 已修复 |
| `Button` | 使用 shadcn 默认 token → `app-*` token | ✅ 已修复 |
| `Input` | `border-input bg-transparent` → `border-app-border bg-app-surface-muted` | ✅ 已修复 |
| `Textarea` | `border-input` → `border-app-border bg-app-surface-muted` | ✅ 已修复 |
| `Switch` | `primary`/`input` → `app-info`/`app-border` | ✅ 已修复 |
| `Separator` | `bg-border` → `bg-app-border` | ✅ 已修复 |
| `Toggle` | 通用 token → `app-*` token | ✅ 已修复 |

### 11.2 Settings 页面问题 ✅ 已修复

| 问题 | 位置 | 修复内容 |
|------|------|----------|
| Switch 错误的 className 覆盖 | PromptSettingsPanel | 移除了 `border-app-border shadow-none data-[state=checked]:bg-app-surface-active data-[state=unchecked]:bg-app-surface-muted` |
| Textarea 冗余样式覆盖 | TextAreaSection | 简化为仅保留 `mt-3` 和 `minHeightClassName` |
| Separator 多余覆盖 | SectionDivider | 移除 `className="bg-app-border"`，组件已内置 |

## 12. 禁止项

后续 UI 不应再引入以下问题：

- 在组件内直接写新的硬编码色值而不进入 token
- 用高饱和渐变承担主布局视觉
- 列表项只有一层信息，导致用户需要 hover 后才理解对象含义
- 过强阴影与大圆角同时泛滥，破坏工具界面的克制感
- 页面级滚动回流破坏工作台固定骨架
- 把营销页式留白和桌面工具密度混在一起
- **工作台组件直接使用 shadcn 通用 token 而非 app-* token**
- **覆盖组件内部已使用的 app-* token**

## 13. 设计评审检查表

每次新增或调整工作台 UI，至少检查以下项目：

- [ ] 亮色、暗色、跟随系统三种主题是否都成立
- [ ] `canvas / panel / surface / menu` 四层关系是否仍然清晰
- [ ] 是否复用了既有 `app-*` token，而不是新增临时颜色
- [ ] hover、active、focus-visible、disabled 是否完整
- [ ] 列表是否具备主次信息层级
- [ ] 状态色是否只用于语义反馈
- [ ] 局部滚动是否正常，是否出现 body 滚动
- [ ] 字体回退到系统字体时，布局是否仍然稳定
- [ ] **组件是否使用了正确的 `app-*` token，而非通用 shadcn token**
- [ ] **是否避免覆盖组件内部已定义的 app-* token**

## 14. 后续扩展原则

如果后续新增设置页、 onboarding、营销页等新表面，不应直接复用工作台的全部约束，而应：

1. 保留 token 体系和主题模型
2. 单独定义页面类型的布局与密度规则
3. 明确哪些模式属于"工作台语法"，哪些属于"页面语法"

本文件在当前阶段应优先保护工作台语法的连续性。

## 15. 组件 Token 映射参考

当需要将 shadcn 组件适配到工作台场景时，使用以下映射：

| shadcn Token | 工作台等效 Token | 备注 |
|--------------|-----------------|------|
| `bg-card` | `bg-app-surface` | 卡片背景 |
| `bg-background` | `bg-app-canvas` | 页面背景 |
| `text-foreground` | `text-app-foreground` | 主要文字 |
| `text-muted-foreground` | `text-app-muted` | 次要文字 |
| `border-border` | `border-app-border` | 默认边框 |
| `bg-accent` | `bg-app-surface-hover` | 悬停背景 |
| `bg-input` | `bg-app-surface-muted` | 输入框背景 |
| `primary` | `app-info` | 强调色（状态） |
| `destructive` | `app-danger` | 错误/危险 |
| `ring` | `app-info/50` | 焦点环 |

