# 交接：背景特效（波纹）2D 定版 + 弹层层级修复（2026-08-16）

> **状态：波纹已重写为 2D canvas 流体并实测验证（真实屏幕呈现 + 流动）；弹层层级修复保持；
> bm-server embed 已重建（待办 0 完成）。** 剩余 = 用户桌面版观感确认（§四）。

## 一、波纹动画排查史（git 证据链 + 本轮独立实测，勿再重走弯路）

| 版本 | 提交 | 机制 | 结果 |
|---|---|---|---|
| FluidWave 动画版 | ba2d1df | WebGL2 不透明（alpha:false）+ rAF 30fps | 用户"唯一一次没说没效果"的版本 |
| EffectWave 初版 | 7dcf783 | WebGL2 透明（alpha:true）+ mix-blend-overlay + setTimeout | ❌ 用户真实浏览器确认不流动；静态纹理可见 |
| 2D 丝带版 | 79e609e（已回退） | 2D canvas 实色丝带 | IAB 内可见但细、淡，用户反馈"毛都没有"（观感不足） |
| WebGL 定版 | e002f10 | WebGL2 alpha:false + rAF 30fps | 动画引擎正常但从未通过用户验证 |
| **2D 流体定版（当前）** | 本轮 | 2D canvas 整屏流体（深蓝底 + 3 条粗壮正弦光带 + 高光） | ✅ **本机真实屏幕实测：画面呈现 + 流动** |

**本轮独立实测的关键证据**（不盲信上一份交接的"环境铁律"）：
1. headless/无 GPU Chromium 里 WebGL shader 在 GPU 内存正常产帧（readPixels 96% 像素变化），但合成输出黑屏；**同环境连 2D 对照和 CSS 背景也黑屏+彩色噪点**——黑屏是无 GPU 合成失败的普遍现象，不是 WebGL 特有。上一份交接的"WebGL drawing buffer 不呈现是环境限制"结论方向对，但证据不完整。
2. **CDP captureScreenshot 在本机（真 GPU，Radeon 780M）也返回黑帧**——截图管线本身不可信；真实验证手段 = **PowerShell CopyFromScreen 抓真实屏幕 + 帧心跳（canvas.dataset.frames）**。
3. 最终验证（带窗口 Edge 真 GPU + 抓屏）：2D 流体在真实屏幕上呈现为蓝白波浪画面（light 主题采样吻合 PALETTES.light），帧心跳 25fps，两帧抓屏在窗口区域像素差异 3-14% —— **画面呈现且流动，实锤**。
4. 窗口被遮挡/放屏幕外时 Chromium 合成器停止输出（抓屏差异 0%、rAF 被节流）——验证动画必须窗口在前台可见。

**为什么最终选 2D**：观感参照 deepseek 官网流体（参考项目 "Living fluid board" 同源观感），但用 2D 实现——2D canvas 的呈现路径在所有环境（IAB/桌面版/无 GPU/真 GPU）都比 WebGL 可靠，不依赖 GPU 合成；全屏 3 条正弦光带 30fps 开销极小。观感参数集中在 `FluidWave.tsx` 的 `PALETTES`/`BANDS`/`HAZE`，用户不满意时只调这几个常量。

## 二、本轮代码改动

- `frontend/src/components/skin/FluidWave.tsx`：WebGL 全部删除 → 2D 流体渲染（renderFluid 签名不变，壁纸静态帧/设置页缩略图/特效动画三处共用）
- `frontend/src/components/skin/effects.tsx`：EffectWave 改 2D 动画（rAF 30fps 节流 + frames/lastTime 心跳 + reduceMotion 静态帧 + 后台暂停，契约不变）
- `frontend/src/components/skin/SkinBackground.tsx`、`frontend/src/lib/skin.ts`：注释同步（特效自带底色盖过壁纸，非 mix-blend）

## 三、当前环境状态

- bm-server：**已重建 embed 并重启**（PID 变更为新进程，监听 127.0.0.1:17321；embed 的 js 已实测含新代码特征字符串）
- node server.mjs（8765 壳，便携版入口）仍在跑；磁盘 dist 已是新代码（pnpm build）
- Tauri 桌面壳（frontend/src-tauri）：frontendDist=../dist，下次构建自动带新代码

## 四、待办（用户验证）

1. **用户桌面版/8765 入口验证观感**：设置 → 外观 → 玻璃皮肤 → 背景特效"蓝色波纹" → 看整屏蓝白流体是否流动、浓淡是否合适。
2. 观感微调点（用户反馈后改 `FluidWave.tsx` 常量即可）：光带条数/振幅/带宽（BANDS）、配色（PALETTES）、流速（renderFluid 里 t*0.8）。
3. 用户验证菜单：Session 三点菜单应在最顶层、实色清晰（上轮修复保持）。

## 五、关联

- 原交接：HANDOFF_BG_EFFECT_ANIMATION.md；本文件上版（WebGL 定版叙事）已被本轮实测修正
- 参考项目：github.com/WYH66666666/DSH-Transparent-UI-Plugin（aqua 皮肤 Living fluid board；观感参照，机制自研）
- 前端审查修复轮：docs/REVIEW_FRONTEND_CROSS_2026-08-16.md
