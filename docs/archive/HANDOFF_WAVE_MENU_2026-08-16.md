# 交接：背景特效（波纹）2D 定版 + 弹层层级修复（2026-08-16）

> **状态（2026-08-16 终）：用户定调"算了不纠结——完全没效果"，放弃波纹特效投入。**
> 特效开关保留，"蓝色波纹"留存现状（2D 流体实现，常量可调）；此条已记入架构文档
> §四·B 补充 3（v0.25）。本文档转为证据链存档，不再有执行性待办。

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

## 四、终态

**用户定调（2026-08-16）："算了，不纠结了——完全没效果"，放弃。** 即使引擎层面逐级实测正常（帧心跳、GPU 像素、真实屏幕抓屏流动），用户真实环境始终看不到效果——呈现受环境合成器行为差异支配，"引擎产帧" ≠ "用户可见"。特效开关保留，wave 留存现状（2D 流体，`FluidWave.tsx` 常量可调），不再投入。已记入架构文档 §四·B 补充 3（v0.25）与 §十 迭代清单。

## 五、关联

- 原交接：HANDOFF_BG_EFFECT_ANIMATION.md；本文件上版（WebGL 定版叙事）已被本轮实测修正
- 参考项目：github.com/WYH66666666/DSH-Transparent-UI-Plugin（aqua 皮肤 Living fluid board；观感参照，机制自研）
- 前端审查修复轮：docs/REVIEW_FRONTEND_CROSS_2026-08-16.md
