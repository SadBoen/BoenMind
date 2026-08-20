# 玻璃档（Glacier）设计规范

> 风格定位：深海军蓝底 + 白玻璃面板 + 青（Sky Cyan）强调，玻璃拟态（glassmorphism）。
> 配色处方来源：Grok 前端设计专家（2026-08-21）。
> **核心原则：边框用白不用 accent；accent 只许 5% 以下「吻色」；玻璃档唯一特效=光晕。**

## 一、颜色（Design Token）

| 角色 | 值 | 用途 |
|---|---|---|
| `--bm-bg-0` | `#121826` | 画布（去紫海军蓝） |
| `--bm-bg-1` | `#1A2234` | 实心面板 fallback |
| `--bm-bg-2` | `#222C42` | 抬升/卡片 |
| `--bm-bg-3` | `#2A3650` | 悬停/选中实心 |
| `--bm-bg-glass` | `rgba(255,255,255,0.055)` | **白玻璃**（非染色紫！透明度可调） |
| `--bm-panel-mid` | `rgba(56,189,248,0.055)` | 中段吻色（仅此一层青） |
| `--bm-border` | `rgba(255,255,255,0.11)` | 常规（深底可见） |
| `--bm-border-subtle` | `rgba(255,255,255,0.06)` | 弱分割 |
| `--bm-border-strong` | `rgba(255,255,255,0.20)` | 强分割/焦点环外圈 |
| `--bm-fg` | `#E6EDF7` | 主文字 >15:1 |
| `--bm-fg-muted` | `#93A0B5` | 次文字 ≥4.8:1 |
| `--bm-fg-faint` | `#6B7A90` | 占位 |
| `--bm-accent` | `#38BDF8` | 焦点/链接/活动条/光晕 |
| `--bm-accent-hover` | `#7DD3FC` | hover 高光 |
| `--bm-accent-2` | `#0EA5E9` | pressed/过渡 |
| `--bm-accent-soft` | `rgba(56,189,248,0.15)` | 选中底 |
| `--bm-accent-solid` | `#0284C7` | 若 antd 强制白字时 colorPrimary 用这颗 |
| `--bm-danger` | `#FB7185` | 暖玫瑰（与青拉开温度） |
| `--bm-radius` | `12px` | 面板 12/按钮 8/胶囊 999 |

antd：`darkAlgorithm`、`colorPrimary:#38BDF8`、`colorTextLightSolid:#121826`（青底海军字，白字铺青会挂 2:1 对比）。**不要让 darkAlgorithm 自己生成一堆紫。**

**配套（玻璃档必要）**：
```
--bm-blur:              16px          /* blur 10px 不够，像脏雾 */
--bm-saturate:          1.4
--bm-glass-highlight:   rgba(255,255,255,0.22)   /* 上沿 1px 假折射 */
--bm-accent-glow:       drop-shadow(0 0 6px rgba(56,189,248,0.55))
```

**body 渐变（替换旧紫蓝）**：
```css
background:
  radial-gradient(1200px 560px at 50% -12%, #2A3F66 0%, transparent 62%),
  radial-gradient(720px 380px at 92% 108%, rgba(56,189,248,.10) 0%, transparent 50%),
  #121826;
```

**选中态**：`accent-soft` 底 + `box-shadow: inset 0 0 0 1px rgba(56,189,248,.35)`，文字保持 fg，不要把整行染青。

## 二、样式差异

| 维度 | 值 |
|---|---|
| 控件圆角 | 14px |
| 输入卡片圆角 | 14px |
| 头像圆角 | 8px |
| 边框 | 1px **白**（三档 alpha 见上），非 accent alpha |
| 阴影 | 靠渐变+模糊，少阴影 |
| 半透明 | **有**：bg-glass 白 5.5%，透明度 20-95% 可调（localStorage bm_glass_opacity） |
| 背景模糊 | **blur(16px) + saturate(1.4)**：sider/输入卡/会话栏 |
| 特效 | 紫蓝渐变 body（青版）、三处毛玻璃、accent 光晕（唯一允许特效） |
| 专属 CSS | 6 条 |

**玻璃拟态 best practice**（Grok）：
1. 边框用白 `rgba(255,255,255,.10-.20)`，**不用 accent 描边**（除焦点环/活动条）
2. 玻璃填充用白 5-7%，accent 只许 5% 以下吻色
3. 上沿高光 `border-top: 1px solid rgba(255,255,255,.22)` 假折射
4. body 必须有色差（径向渐变），否则 blur 采样纯黑=玻璃不存在
5. 实心主按钮：**青底+海军蓝字**，禁止白字铺 #38BDF8

**透明度一致性漏洞（待修）**：`.bf-sider`/输入卡/会话栏的 rgba 原是硬编码 `.62/.58`，**不跟随透明度滑块**（滑块只影响 --bm-bg-glass/panel-mid）——应改为引用 `--bm-bg-glass`。

## 三、图标

- 发送按钮：**Lucide `CircleArrowUp`** 16px stroke1.5 青 `#38BDF8` + `filter: drop-shadow(0 0 6px rgba(56,189,248,.55))`（圆环=透镜/拟态；光晕=玻璃档唯一特效）
- 按钮底：`bgGlass` + `border rgba(56,189,248,.45)`，图标 accent，hover 底切 accent-soft
- 全档图标基线：Lucide 16px / stroke 1.5 / round cap
- 窗口交通灯：`rgba(255,255,255,.45)` 空心圆（glass 特色）

## 四、配色依据（为什么是 Sky Cyan 不是紫）

- 旧病：`#232b45`（227° 蓝紫）+ `#a78bfa`（258° 紫）同色系互吞，border rgba(紫,.32) 对比 <1.5:1 等于没边。
- `#38BDF8` 色相 ≈199°，和海军蓝（≈222°）拉开 23°，是**分裂互补里最干净的一刀**（不是 180° 互补撞色）。
- 青是蓝玻璃被照亮后的高光色（短波散射）——玻璃拟态的正确语义是「冰/折射」，不是「霓虹紫」。
- 不选 Teal（≈170° 偏绿，像 Grafana 监控台）；Linear/Arc/Raycast 深色里能活的高饱和色全是**高明度、低面积**。