# 交接：背景特效（波纹）定版 + 弹层层级修复（2026-08-16）

> **新对话开工指令：读本文件 → 按 §四 待办顺序执行。**
> 状态：波纹已定版（参考项目同款机制）并提交；弹层层级已修复并提交；**待办 0 = 重建
> bm-server embed（桌面版验证入口）**；两项修复均待用户真实环境验证。

## 一、波纹动画完整排查史（git 证据链，勿再重走弯路）

| 版本 | 提交 | 机制 | 结果 |
|---|---|---|---|
| FluidWave 动画版 | ba2d1df | WebGL2 不透明（alpha:false）+ rAF 30fps + u_time + 无 blend | ✅ **真实环境像素级验证有效**（提交信息原文："实测 1.2s 内 73.6% 像素变化"）——用户"唯一一次没说没效果"的版本 |
| EffectWave 初版 | 7dcf783 | WebGL2 透明（alpha:true）+ mix-blend-overlay + setTimeout | ❌ 用户真实浏览器确认不流动（HANDOFF_BG_EFFECT_ANIMATION 原交接）；静态纹理可见 |
| 2D 丝带版 | 79e609e（已回退） | 2D canvas 实色丝带（去 blend） | IAB 内像素可见但细、淡，用户反馈"毛都没有"（观感不足）；此版已被 WebGL 定版替换 |
| **定版（当前）** | 最新提交 | **参考项目同款**：WebGL2 不透明（alpha:false）+ rAF 30fps + u_time + 无混合模式 | 待用户真实环境验证 |

**为什么是 WebGL 而非 2D**：参考项目 @deepseek-ai/dsh-client-ui-aqua（[DSH-Transparent-UI-Plugin](https://github.com/WYH66666666/DSH-Transparent-UI-Plugin)）的 "Living fluid board" 就是整屏 WebGL 流体 shader + rAF——用户点名"看看参考项目的做法"。当初 FluidWave shader 即由其移植。

**⚠️ 环境铁律（本会话用教训换来的，务必记住）**：
1. **ZCode IAB（内嵌 webview）里 WebGL drawing buffer 完全不呈现**（纯红 shader 测试 0%，CSS 与 2D canvas 正常）——**在 IAB 里测不到 WebGL 画面是环境限制，不是产品 bug**；当初差点因此误判"环境不支持 WebGL"而放弃 WebGL 方案
2. **IAB 截图返回冻结帧**——动画无法用截图/像素对比验证（交接原文档已记载）
3. 动画类 UI 验证 = 真实浏览器/桌面版 + dataset 帧计数（frames/lastTime/shaderOk 心跳已埋）

## 二、弹层层级修复（用户实测反馈）

- **现象**：Session 列表三点菜单弹出后"被盖在毛玻璃下方"
- **修复**（已提交）：
  1. dropdown-menu / select / tooltip 的 z-50 → **z-[80]**（统一最高层；ClassicShell 右键菜单 z-[70] 之下→之上）
  2. glass 皮肤**弹层（菜单/选择器/提示）移除 backdrop-filter**——操作浮层实色清晰、恒在最顶层（参考项目浮层纪律；原 glass/style.css 把弹层也毛玻璃化）
- 保留毛玻璃：nav/footer/.dv-groupview/dialog-content（皮肤卖点，用户未抱怨）

## 三、当前环境状态

- bm-server：**已在跑（旧 exe！embed 的是 2D 丝带版 dist）**——待办 0 重建
- vite dev：5176 在跑（最新代码，IAB/浏览器可看）
- 前端 dist（磁盘）：最新（WebGL 定版 + 菜单修复，已 build）
- 浏览器 IAB：6 个标签（5176），glass 皮肤 + light 主题（localStorage 测试残留）
- 壁纸清单：青蓝/日落/极光/星云（四款两字，bluewave 已删）

## 四、待办（按顺序）

1. **重建 bm-server embed**（桌面版验证入口，2.5 分钟）：停服务 → `cd backend && cargo build --release --features embed` → Start-Process 启动（勿用 run_in_background，会被清理）
2. **用户验证（桌面版！）**：设置 → 外观 → 玻璃皮肤 → 背景特效"蓝色波纹" → 看整屏蓝色流体是否流动。IAB 里测不到 WebGL，务必桌面版/真实浏览器
3. **用户验证菜单**：Session 三点菜单应在最顶层、实色清晰
4. 若真实环境波纹仍不动 → 查 mix-blend/合成器（Chromium issue 503638）或参考项目降级路径（"流体着色器失败时不拖垮主题"，DSH-Transparent-UI-Plugin 最新 commit 同款）

## 五、关联

- 原交接：docs/archive/HANDOFF_BG_EFFECT_ANIMATION.md（假设 A/B/C 排查史，已被本文件结论覆盖）
- 参考项目：github.com/WYH66666666/DSH-Transparent-UI-Plugin（aqua 皮肤：Living fluid board / 粒子鲸 / Mica 模式 / 着色器失败降级）
- 前端审查修复轮：docs/REVIEW_FRONTEND_CROSS_2026-08-16.md（P0/P1/清理轮已完成，P2 遗留 API 契约防线等）
- 记忆：frontend-review-2026-08-16（本会话全部结论已同步）
