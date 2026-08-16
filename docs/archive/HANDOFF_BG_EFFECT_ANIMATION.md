# 交接：背景特效"蓝色波纹"动画不生效（2026-08-16）

> 状态：**未解决，待排查**。现象已在真实浏览器由用户确认（"确定是没动静"）。
> 交接给下一轮会话/开发者，按本文档证据链与建议方案继续。

## 现象

设置中心 → 外观 → 皮肤 → 玻璃 → 背景特效 → 选择"蓝色波纹"（或默认即开启）：
背景壁纸之上应叠加缓缓流动的蓝色波纹动画（30fps，全局时钟驱动）。
**实际：波纹静态纹理可见，但完全不流动**（真实浏览器确认）。

## 已完成（本轮交付）

特效系统架构本身已完成并推送（commit `7dcf783`）：

- `frontend/src/components/skin/effects.tsx` — EffectWave 组件：WebGL2 透明 canvas，
  `mix-blend-overlay` 叠加在壁纸之上；setTimeout 33ms 驱动（30fps）；
  `performance.now()/1000` 全局时钟（多界面速度统一）；reduce-motion 时静态帧；
  shader program 已 WeakMap 缓存（只编译一次，修复了初版每帧重编译卡死整页的 bug）
- `frontend/src/lib/skin.ts` — `BACKGROUND_EFFECTS` 注册表（none/wave），
  将来新增特效（礼花/微风）在此登记
- `frontend/src/stores/app-store.ts` — `backgroundEffect` 状态（localStorage:
  `boenmind.skin.effect`，默认 wave）
- `frontend/src/components/skin/SkinBackground.tsx` — 层序：壁纸 → 特效层 → 遮罩
- `frontend/src/components/settings/AppearanceSettings.tsx` — 背景特效选择 UI
- `frontend/src/components/skin/FluidWave.tsx` — 蓝色波浪壁纸改为静态纹理
  （动画归特效层管）

## 已排查的证据链（IAB 内嵌浏览器实测）

| 检查项 | 结果 | 方法 |
|---|---|---|
| shader 编译 | ✅ 成功 | canvas.dataset.shaderOk=1（getShaderParameter COMPILE_STATUS） |
| uniform location | ✅ u_time/u_resolution 均有效 | getUniformLocation 非 null |
| 渲染循环在跑 | ✅ frames 计数 5→63→74 持续增长 | render 内 dataset.frames++ |
| 时间源变化 | ✅ lastTime 45.5→74（自增） | dataset.lastTime |
| GL 错误 | ✅ 无（0） | gl.getError() |
| 特效层内容渲染 | ✅ 同页开/关截图差异 6.66% | PowerShell 像素采样 |
| 壁纸静态波纹 | ✅ 视觉模型确认（8.5/10） | MiniMax 视觉 API |
| **动画帧更新** | ❌ 间隔 1-3.5s 截图 0% 差异 | 像素对比（阈值 3/255） |
| CSS 动画（animate-pulse） | ❌ 截图 0% 差异 | 同方法 |

**关键矛盾**：所有 WebGL 环节正确 + 渲染循环运行 + 时间变化，但画面从不更新。
且 CSS 动画在截图中也不更新 → **IAB 的 webview 截图返回冻结帧（合成器不更新）是
测试环境限制，无法用 IAB 截图验证任何动画**；但**用户真实浏览器确认也不动**，
说明产品侧确实存在动画不生效的 bug（不只是环境问题）。

## 待排查假设（按可能性排序）

### A. WebGL canvas 合成/混合怪癖（最可能）
`getContext("webgl2", { alpha: true, premultipliedAlpha: false })` +
CSS `mix-blend-overlay` 的组合，在 WebView2/Chromium 上可能导致
**canvas 帧不被合成器提交**（首帧渲染后静止）。
- **验证**：把 EffectWave 改为 **2D canvas 实现**（正弦波叠加 + globalAlpha，
  同样透明 canvas + mix-blend），若 2D 动画可见即坐实 WebGL 合成问题。
- **建议方案**（2D 实现要点）：
  ```ts
  // 每帧：clearRect → 多层正弦波（不同频率/相位/速度）填充路径 → fill
  // 时间统一 performance.now()/1000；颜色 rgba(60,110,220,0.25~0.5)
  ```
  2D canvas 的合成路径与 WebGL 不同，且实现更简单可靠；观感损失可控。

### B. setTimeout 循环未真正执行
frames 计数增长证明 render 被调用（dataset 计数器），但计数器写入与
drawArrays 之间无异常……此假设证据不足，仅当 A 方案无效时回头复查：
- 检查 `reduceMotion` 是否意外为 true（`boenmind.appearance.reduceMotion`）
- 检查 EffectWave 是否被 React 卸载/重挂导致 cleanup 反复清 timer
  （SkinBackground 随皮肤状态切换时）

### C. 双 WebGL canvas 上下文冲突
FluidWave（壁纸，alpha:false）+ EffectWave（特效，alpha:true）同屏两个
WebGL context——Chromium 对多 context 有限制（同时活跃 context 数）。
- 验证：临时把壁纸换成渐变（无 canvas），只留特效 canvas 看是否动。

## 建议的接手路径

1. 先做假设 A 的 2D canvas 重写（改动局限在 effects.tsx 的 render 实现，
   EffectWave 组件签名不变）
2. 用**真实浏览器/桌面版**验证（IAB 截图无法验证动画——记住这个限制，
   动画类 UI 只能靠用户前台确认或 dataset 计数器证据）
3. 若 2D 也不动 → 检查 B/C，并在 SkinBackground 挂载处加日志
   （或把 frames 计数显示到可见 UI 上让用户直接报数）

## 相关文件

- `frontend/src/components/skin/effects.tsx`（核心，改动点）
- `frontend/src/components/skin/SkinBackground.tsx`（层序）
- `frontend/src/lib/skin.ts`（BACKGROUND_EFFECTS）
- 视觉验证工具样板：`/tmp/vision/analyze.mjs`（MiniMax 视觉 API 直调）
- 像素采样：PowerShell System.Drawing（见记忆 skin-system-2026-08-16）
