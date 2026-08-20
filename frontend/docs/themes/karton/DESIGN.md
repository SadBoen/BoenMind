# 卡通档（Kraft Journal）设计规范

> 风格定位：暖米牛皮纸底 + sage 墨绿主色 + 大圆角（手账贴纸感）。
> 配色处方来源：Grok 前端设计专家（2026-08-21）。
> **核心原则：一档一颗 accent（唯一绿），antd 与 cssVars 必须同 hex；文字用暖墨不用冷黑；贴纸降 8-10%；危险色用陶土。**

## 一、颜色（Design Token）

| 角色 | 值 | 用途 |
|---|---|---|
| `--bm-bg-0` | `#E8DDC9` | 牛皮纸画布（保留识别） |
| `--bm-bg-1` | `#F7F1E6` | 面板（比画布亮一档） |
| `--bm-bg-2` | `#E4D8C0` | 次级面 |
| `--bm-bg-3` | `#D9C9A8` | 选中（和纸叠层） |
| `--bm-bg-glass` | `rgba(247,241,230,0.88)` | 半透明面板 |
| `--bm-panel-mid` | `rgba(62,107,94,0.06)` | 面板中段（一点 sage 吻色） |
| `--bm-border` | `#D4C4A8` | 常规边框 |
| `--bm-border-subtle` | `#E5D9C4` | 弱分割线（单元内） |
| `--bm-border-strong` | `#C4B090` | 强分割线（单元间/resizer） |
| `--bm-fg` | `#2C2416` | 主文字（**暖墨**，禁冷黑） |
| `--bm-fg-muted` | `#6B5D4D` | 次文字 |
| `--bm-fg-faint` | `#9A8B78` | 占位 |
| `--bm-accent` | `#3E6B5E` | **唯一绿**（cssVars = antd colorPrimary） |
| `--bm-accent-hover` | `#2F5448` | 同色相加深（H 158-162° 只降 L） |
| `--bm-accent-2` | `#3E6B5E` | pressed 同主绿 |
| `--bm-accent-soft` | `rgba(62,107,94,0.12)` | 选中浅底 |
| `--bm-danger` | `#C45C4A` | **陶土**（与牛皮纸同温度，非冷红） |
| `--bm-radius` | `20px` | 面板/输入 20、头像 12、按钮 999 |

antd：`colorPrimary:#3E6B5E`、`colorPrimaryHover:#2F5448`、`borderRadius:16`（LG 24/SM 12）。

**双绿历史事故**：旧值 `#2C4A47`（冷杉绿，过深像企业后台）已废弃，统一到 `#3E6B5E`（sage，植物手账）。白字铺在 `#3E6B5E` 约 6.2:1 AA 过关，不必回退死绿。

## 二、样式差异

| 维度 | 值 |
|---|---|
| 控件圆角 | 16-24 / Button 18 / Tag/Segmented 胶囊 9999 |
| 输入卡片圆角 | 20px |
| 头像圆角 | 12px |
| 边框 | 1px 暖咖（`#D4C4A8` 族） |
| 阴影 | 轻 `0 1px 4px` |
| 半透明 | 无（88% 近实心） |
| 背景模糊 | 无 |
| 特效 | **贴纸 SVG 满铺 opacity 0.08-0.10**（勿超 13%，会抢前层） |
| 专属 CSS | 10 条（**3 条死代码与全局重复，可删**：`.bm-sider`/`.chat-input-card` border/`.session-item:hover`） |

**贴纸约束**：贴纸层保持暖色（珊瑚/芥末/米），**不要再铺一层深绿贴纸**（会和 accent 振纹）；用户自定义 accent 换成蓝时贴纸粉绿不跟随——属已知取舍。

## 三、图标

- 发送按钮：**Lucide `Send`（纸飞机）** 18px stroke2.25 round cap **禁止 rotate**（用方向正确的那颗），色 `#3E6B5E`
- 按钮底：实心 `#3E6B5E` 图标 `#F7F1E6` radius 999（胶囊）
- 全档图标基线：Lucide 18px / stroke 2.2 / round cap
- 窗口交通灯：红黄绿（卡通特色保留）

## 四、配色依据

- 暖黄米 × 冷绿是标准文具配（kraft 纸 × sage），协调。
- `#3E6B5E` 色相 162° sage；`#2C4A47` 色相 174° 冷杉——两套绿不是深浅关系，是不同色相，必须统一。