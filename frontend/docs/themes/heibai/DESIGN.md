# 黑白档（Graphite Editorial）设计规范

> 风格定位：黑白灰、浅底、克制、现代桌面工具感（对标 VS Code / Cursor 浅色）。
> 配色处方来源：Grok 前端设计专家（2026-08-21），经 ant.design 官方色板核对。
> **核心原则：95% 石墨分层 + 5% 克制蓝做焦点（主按钮/焦点环/活动指示条），accent 禁止等于文字色。**

## 一、颜色（Design Token）

| 角色 | 值 | 用途 |
|---|---|---|
| `--bm-bg-0` | `#F5F6FA` | 画布/页面底 |
| `--bm-bg-1` | `#FFFFFF` | 面板/白卡 |
| `--bm-bg-2` | `#ECEEF3` | 悬停/次级面 |
| `--bm-bg-3` | `#E2E5ED` | 选中实底 |
| `--bm-bg-glass` | `#FFFFFF` | 本档不用玻璃 = bg1 |
| `--bm-panel-mid` | `#F5F6FA` | 面板中段 |
| `--bm-border` | `#E2E5EB` | 常规边框 |
| `--bm-border-subtle` | `#EEEFF3` | 弱分割线（单元内） |
| `--bm-border-strong` | `#C5CAD6` | 强分割线（单元间/resizer） |
| `--bm-fg` | `#1A1D23` | 主文字（≠ accent） |
| `--bm-fg-muted` | `#5C6370` | 次文字 |
| `--bm-fg-faint` | `#8B919C` | 占位/时间戳 |
| `--bm-accent` | `#2563EB` | 焦点/主按钮/活动条 |
| `--bm-accent-hover` | `#1D4ED8` | hover 深一档 |
| `--bm-accent-2` | `#1E40AF` | pressed |
| `--bm-accent-soft` | `rgba(37,99,235,0.08)` | 选中浅底 |
| `--bm-danger` | `#DC2626` | 危险 |
| `--bm-radius` | `6px` | 按钮/面板/输入 6 |

antd：`colorPrimary:#2563EB`、`borderRadius:6`、defaultAlgorithm（浅色）。无 blur、无贴纸。

**选中态配方**：底 = `accent-soft`（文字仍 fg，不要蓝字）+ 左/下 2px 实线 accent + 焦点环 `0 0 0 2px #fff, 0 0 0 4px #2563EB`。

## 二、样式差异（相对其它档）

| 维度 | 值 |
|---|---|
| 控件圆角 | 6px（最小档） |
| 输入卡片圆角 | 6px |
| 头像圆角 | 8px |
| 边框 | 1px 灰（三档明细见上） |
| 阴影 | 轻 `0 1px 4px` |
| 半透明/玻璃 | **无**（bg-glass=白） |
| 背景模糊 | **无** |
| 特效 | 无（纯明度分层 + 蓝焦点） |
| 专属 CSS | 0 条（默认档） |

## 三、图标

- 发送按钮：**Lucide `ArrowUp`**（16px stroke2 方钮 32×32 radius6 不旋转）——2024 工作台标准（Cursor/ChatGPT 同款）
- 全档图标基线：Lucide 16px / stroke 1.75 / round cap
- 窗口交通灯：自定义 SVG 白描边 1.5（石墨圆点）

## 四、配色依据（为什么是 #2563EB）

- 色相 221° 冷蓝，与画布 `#F5F6FA` 冷灰同温度（不是暖蓝/靛/黑），与 ant 官方蓝主色同族。
- 纯无彩只适合写作应用；dockview 多面板需要「可扫描的选中色」，明度差无法替代色相差（WCAG 2.4.7 焦点可见）。
- 危险色 `#DC2626`：正红，克制不刺眼。