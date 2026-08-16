# 前端专项质量审查报告（工具A code-review 独立审查）

> **审查日期**：2026-08-16
> **审查范围**：`frontend/src/`（67 tsx / 13 ts / 2 css + i18n/locales ×4，约 1.6 万行）+ `package.json` / `vite.config.ts` / `index.html` / `tsconfig*.json` / `components.json` / `.oxlintrc.json`
> **排除**：backend/、src-tauri/、node_modules/、dist/、锁文件
> **方法**：逐文件全量通读 + grep 实证（引用/使用点核对）+ `npx tsc -b` 编译实证 + `npx oxlint` 静态检查实证 + i18n key 引用核对
> **声明**：本报告由工具A（code-review 工作流）独立产出，**未查看**本轮其他两份工具报告；上轮交叉报告（docs/REVIEW_TOOLS_CROSS_2026-08-16.md 及 docs/review-tools-2026-08-16/ 三份）已浏览，凡上轮已报且本轮仍存在/已修复的项均显式标注"继承上轮"，不重复展开。
> **只读审查**：本轮未修改任何代码。

---

## 〇、结论摘要

前端整体质量高于同规模个人项目平均线：组件分层清晰（ChatPane/DockLayout 宿主化、注册表驱动导航与设置）、i18n 四语言结构约束（`Translation` 类型）、流式状态机（finalize/stop 兜底）都见工程自觉。但本轮发现 **1 条 Critical（构建当前断裂）** 与一批集中在"退役桌面壳残留 + i18n 维护性 + 事件流健壮性"三处的积压：

1. **`npm run build` 当前必失败**：`tsc -b` 退出码 2——ja.ts / ko.ts 缺 `settings.experts.namePrefixHint` key（zh/en 有）。这是本轮最高优先级。
2. **en.ts / ko.ts 第 571 行 `uninstall: "卸载"` 为中文残留**（复制粘贴漏译，四语言同构包中的两处）。
3. **markdown 三处渲染依赖 `prose` 类但未装 `@tailwindcss/typography`**——排版样式实际未生效。
4. **"蓝色波纹不动"给出新候选假设**（详见 §BUG-006），其中 **float32 u_time 精度** 与 **backdrop-filter 合成层交互** 是上轮交接文档未覆盖的新角度。
5. **精简专项收获大**：2 个死依赖（react-rnd / react-resizable-panels）、2 个死组件（ChatWindow / ExpertTeamDocs，继承上轮仍存在）、6 个死导出/死字段、~24 个死 i18n key（×4 语言 ≈ 96 条）、2 个不可达应用（plugins/steward）、1 处死 DOM 引用（#system-ui）。

**实证**：`npx tsc -b`（退出码 2，3 个错误）；`npx oxlint src`（22 warnings / 0 errors）；死依赖/死引用全部 grep 双端核对（package.json 声明 vs src 引用）。

---

## 一、BUG — 缺陷与潜在问题

### BUG-001 | 构建断裂：ja/ko 语言包缺 `namePrefixHint` key，`tsc -b` 失败（Critical）
**File(s):** `src/i18n/locales/ja.ts:597`、`src/i18n/locales/ko.ts:597`（缺键）；`src/i18n/locales/zh.ts:617`、`src/i18n/locales/en.ts:618`（有键）
**Severity:** Critical
**Observation:** `npx tsc -b` 实测退出码 2，报 `Property 'namePrefixHint' is missing ... but required`（ja/ko 各一条，另一条为 skin.ts:111 未用变量）。`package.json` 的 `build` 脚本是 `tsc -b && vite build`——**当前主分支构建直接失败，任何发布/打包都会被卡住**。zh.ts 在 2026-08-16 专家表单加前缀校验时新增了 `namePrefixHint`，en 同步了，ja/ko 漏了（结构同构由 `Translation` 类型强制，编译期立刻暴露——这正说明类型约束有效，只是没人跑过 build/CI）。上轮修复轮只跑了 backend 的 cargo 检查，前端构建断裂未被发现。
**建议：** 补 ja/ko 两行翻译（ja: "名前は coding- または chat- で始まる必要があります（統一 APP プレフィックス）、例: coding-coder"；ko 同理），顺带删除 skin.ts:111 的 `SKIN_AUTO_KEY` 死常量；此后建议把 `tsc -b` 纳入任何提交前检查。

### BUG-002 | en/ko 语言包第 571 行 `uninstall: "卸载"` 为中文残留（High）
**File(s):** `src/i18n/locales/en.ts:571`、`src/i18n/locales/ko.ts:571`（对应 `zh.ts:570`；ja.ts 同行正常）
**Severity:** High
**Observation:** 三个语言包同构，en 与 ko 的 `skills.uninstall` 值直接复制了中文"卸载"——英文界面与韩文界面显示中文"卸载"按钮。这是典型的"×4 语言同步"漏网（值级漏译，类型检查只能保证结构不能保证翻译）。用中文字符扫描 en/ko/ja 三个包仅此一处（ja 的命中均为日文汉字，正常）。
**建议：** en: "Uninstall"；ko: "제거"（或"삭제"）。建议加一条 CI/脚本扫描：非 zh 包值中出现 `[\u4e00-\u9fff]` 且为纯中文词组即报错（注意日文包需排除汉字）。

### BUG-003 | markdown 渲染三处使用 `prose` 类但未安装 `@tailwindcss/typography`（High）
**File(s):** `src/components/chat/MessageItem.tsx:88`、`src/components/files/FilePreview.tsx:70`、`src/components/team/ExpertTeamDocs.tsx:54`（`prose prose-sm dark:prose-invert`）；`package.json` 全文
**Severity:** High
**Observation:** Tailwind v4 的 `prose` 系列类由 `@tailwindcss/typography` 插件提供，而 `package.json` 依赖中无此包，`index.css` 也未 `@plugin` 引入——**三处 markdown 渲染的正文排版样式（列表/标题/引用/表格间距等）实际全部未生效**，只剩外层 `break-words` 与代码块自定义样式兜底。代码块（`pre` 覆盖）与链接（`a` 覆盖）有自定义样式，但 markdown 的标题/列表/表格/引用排版缺失，长文档（专家团队文档、README 预览）观感与预期不符。
**建议：** 装 `@tailwindcss/typography` 并在 index.css `@plugin "@tailwindcss/typography";`（v4 语法）；或明确放弃 prose 并自写 markdown 排版类。三者选一即可。

### BUG-004 | `reduceMotion` 分支在 EffectWave 中是死分支：开启后动画照常运行（Medium）
**File(s):** `src/components/skin/effects.tsx:161-168`
**Severity:** Medium
**Observation:**
```ts
const loop = () => {
  timer = window.setTimeout(loop, 33);
  if (reduceMotion) { render(); return; }   // ← 与下行完全相同
  render();
};
```
两个分支体完全一致——注释与文件头宣称"reduce-motion 开启时渲染静态帧（与无障碍偏好兼容）"，但实现根本没做：reduceMotion 开启时仍每 33ms 全屏 WebGL 重绘（CPU/GPU 空耗 + 与无障碍意图相悖）。且 `loop` 里先排下一个 setTimeout 再渲染，reduceMotion 下渲染的帧内容随时间变化（`performance.now()` 驱动），不是静态帧。
**建议：** reduceMotion 时只 `render()` 一次（进 effect 时）并停止调度，或按 `performance.now()` 冻结在启动时刻的 time 值渲染单帧。

### BUG-005 | 波纹特效注释与实现漂移：`soft-light` vs `mix-blend-overlay`（Low）
**File(s):** `src/components/skin/effects.tsx:3`（文件头注释"mix-blend-mode: soft-light"）、`src/components/skin/effects.tsx:185`（实现 `mix-blend-overlay`）
**Severity:** Low
**Observation:** 文件头、`FRAGMENT` 注释（"混合交给 CSS mix-blend-mode"）与渲染层（`className="h-full w-full mix-blend-overlay"`）不一致。overlay 与 soft-light 在浅色壁纸上的视觉差异大（overlay 对 50% 灰以下区域变暗、以上变亮；soft-light 更柔和），直接关系到波纹"看起来动不动"的观感（见 BUG-006 候选 B'）。修注释或修实现需与产品预期对齐。

### BUG-006 | "蓝色波纹"动画静止——新视角候选假设（专项，待排查）
**File(s):** `src/components/skin/effects.tsx`（全文 187 行）、`src/components/skin/SkinBackground.tsx:47`、`src/components/skin/FluidWave.tsx:161-181`
**Severity:** High（功能缺陷，未决）
**Observation:** 已读 `docs/archive/HANDOFF_BG_EFFECT_ANIMATION.md`，其已实证：shader 编译 ✅、uniform ✅、渲染循环在跑（frames 计数增长）、时间源变化（lastTime 自增）、GL 无错误、特效层内容有渲染（开关截图 6.66% 差异），但画面帧间 0% 差异。交接文档给出假设 A（WebGL 合成怪癖，最可能）/B（setTimeout 未执行）/C（双 WebGL context 冲突）。以下为本轮**全新视角补充的候选假设**（标注为候选，未写死结论，按嫌疑排序）：

- **候选 A'（与交接 A 同族，补充具体机制）：glass 皮肤下 backdrop-filter 合成层交互。** `skins/glass/style.css` 对 `nav/footer/.dv-groupview/[data-slot=dialog-content] 等施加 `backdrop-filter: blur(...)`——backdrop-filter 会在 Chromium 合成器创建独立合成层；`EffectWave` canvas（`mix-blend-overlay`，紧邻这些层）的帧提交可能与 backdrop 层缓存路径冲突，首帧后不再重绘。**验证**：切到 classic 皮肤（无任何 backdrop-filter）看波纹是否流动；或在特效 canvas 外层容器临时加 `isolation: isolate` / `transform: translateZ(0)` 强制独立合成层。
- **候选 L（交接未覆盖，时间累积型）：`u_time` 的 float32 精度耗尽。** `gl.uniform1f` 是 32 位浮点；`performance.now()/1000` 从进程启动起单调累积。float32 在值 x 处的量化步长 ≈ x·2⁻²³；当步长超过每帧增量（约 0.033s）时画面冻结——算得阈值约 **x ≈ 27 万秒 ≈ 连续运行 77 小时**。应用挂机过夜/多日是桌面应用的常态，用户"确认不动"的场景可能恰是长时间运行态（上轮证据链的 lastTime 45.5→74 是短时测量，无法覆盖此场景）。**验证**：手动把 `u_time` 设为 300000 看波纹是否静止；或改传 `time % 3600`（周期截断）。
- **候选 B'（感知型）：速度常数与 overlay 对比度过低。** `t = u_time * 0.7`、域扭曲位移 `t * 0.3`，fbm 频率高（`uv*2`）；overlay 混合在浅色渐变壁纸（PRESET_WALLPAPERS 的 css 是低饱和浅色）上对比度极低，叠加 0.8 alpha 的平滑渐变后帧间视觉差异可能低于感知阈。交接的像素采样（1-3.5s 间隔 0% 差异，阈值 3/255）与"35px/s 应可见"矛盾，但采样区域/阈值方法学存疑，不能完全排除"确实在动只是看不出来"。
- **候选 C'（静默降级）：WebGL2 context 获取失败无痕返回。** `getWaveProgram` 在 `getContext("webgl2")` 返回 null 时缓存 null 并静默 return（`renderWave` 直接返回）——画面空白无任何提示（只有 shader 编译失败才 console.error）。Tauri WebView2/GPU 受限环境会命中。验证：console 打印一次 context 失败。
- 其余已排除/低嫌疑：`setTimeout` 链生命周期（StrictMode 双挂载下 cleanup 正确，生产无 StrictMode）；双 WebGL context（两个 canvas 各自独立 context，无冲突面）；canvas 尺寸为 0（用户可见静态纹理，证明已渲染）。

**建议：** 按候选 A'（classic 皮肤对照 + isolation 实验）→ 候选 L（u_time 大值实验，一行代码可验证）→ 候选 B'（速度常数放大 5× 对照）的顺序排查，全部实验都在真实浏览器/桌面版做（交接已证 IAB 截图无法验证动画）。

### BUG-007 | 终端输入大块数据（粘贴 >64KB）静默失败（Medium）
**File(s):** `src/api/client.ts:520-524`（`btoa(String.fromCharCode(...data))`）
**Severity:** Medium
**Observation:** `String.fromCharCode(...data)` 对数组展开超过 ~65535 个元素抛 RangeError（"Maximum call stack size exceeded"）。终端粘贴大段文本/二进制时该异常在 async 函数内抛出 → 变成 rejected promise → 调用方 `api.terminalInput(...).catch(() => {})` 静默吞掉——**用户粘贴内容直接丢失且无任何提示**。当前 xterm 输入走 `term.onData` 逐次事件，单次通常较小，但浏览器粘贴大文本到终端时 `onData` 可能携带数万字符的单帧数据，触发概率真实存在。
**建议：** 改为分块 base64 编码（`for (let i = 0; i < data.length; i += 0x8000)` 逐块 `String.fromCharCode`），或 `chunkedArrayBufferToBase64` 标准实现。

### BUG-008 | ja/ko 的 `nameHint` 文案与校验正则矛盾（Medium）
**File(s):** `src/i18n/locales/ja.ts:617`、`src/i18n/locales/ko.ts:617`（"coding-xxx / wiki-xxx"）；`src/components/settings/ExpertsSettings.tsx:234`（`/^(coding|chat)-[a-z0-9_-]+$/`）
**Severity:** Medium
**Observation:** zh/en 的 `nameHint`/`namePrefixHint` 都写 "coding-xxx / chat-xxx"，而 ja/ko 的 `nameHint` 写 "coding-xxx / wiki-xxx"——与前端校验正则（仅接受 `coding-`/`chat-` 前缀）冲突：日韩用户按界面提示填 `wiki-xxx` 会被 toast 拒绝。翻译维护漂移的又一实例（与 BUG-001/002 同根：四语言包靠人工同步）。
**建议：** 修正 ja/ko 文案与 zh/en 对齐；长期建议给四语言包加"值级一致性"扫描（如占位符、品牌词、示例值的对照测试）。

### BUG-009 | TodoPanel 事件流断连后无任何恢复路径（Medium）
**File(s):** `src/components/coding/TodoPanel.tsx:37-52`、`src/api/client.ts:848-849`（"网络中断由 keep-alive 重连语义交给上层"）
**Severity:** Medium
**Observation:** `subscribeEvents` 流断开后不重连（`catch` 静默），注释把重连责任推给上层——但 TodoPanel 是唯一消费者：断连后面板只显示灰色"未连接"点，**没有重连逻辑也没有手动重连入口**（FilePanel 同用 subscribeEvents 但有手动刷新按钮兜底）。后端 250ms 轮询的替代通道（上轮 B 报告的 PERF-004）不存在于前端。用户长会话期间后端重启/网络闪断 → 任务清单永久停更，直到切换会话或重挂面板。
**建议：** 抽 `useSessionEventStream(sessionId, handler)` 共享 hook，内置指数退避重连（onClose/网络错误后 1s→2s→4s…重连），TodoPanel/FilePanel 复用；或在 TodoPanel 加手动重连按钮。

### BUG-010 | 错误消息硬编码 `⚠️` emoji 前缀且未走 i18n（Low）
**File(s):** `src/stores/app-store.ts:762`（`content: `⚠️ ${ev.message}``）
**Severity:** Low
**Observation:** 流错误以 `⚠️` emoji 前缀硬编码进消息内容——消息会被后端持久化，跨语言界面下 emoji 无语言问题但格式不统一；且 `error` 事件消息本身未 i18n（后端错误文案语言不定，可接受）。次要。

### BUG-011 | 布局重置无二次确认：一键清空用户自定义布局（Low，继承上轮同主题）
**File(s):** `src/components/classic/ClassicShell.tsx:154-165`（右键菜单直接 `resetDockLayout`）
**Severity:** Low
**Observation:** 导航右键「重置布局」直接执行且无确认；dockview 布局快照含用户全部自定义（面板开关/尺寸/位置），误点即丢（localStorage 单键无备份）。与上轮 A-27（布局快照 key 版本化重置用户布局）同主题，两处叠加使"用户布局丢失"成为高频风险。建议：重置前 confirm，或重置时把旧快照备份到 `boenmind.dock.v9.<app>.bak`。

### BUG-012 | `DialogPortal` 引用不存在的 `#system-ui` 宿主容器（Medium，死引用）
**File(s):** `src/components/ui/dialog.tsx:18-30,44`
**Severity:** Medium
**Observation:** `container={document.getElementById("system-ui") ?? document.body}` 及其注释（"系统弹窗统一挂载到桌面壳的 #system-ui 宿主层（Desktop 渲染）"）——**全库 grep 无任何代码创建 `#system-ui` 元素**（桌面壳退役时宿主层随删，dialog.tsx 是漏网残留）。当前恒回退 `document.body`，行为正常但：① 注释误导后续维护者；② 若未来有人"按注释"重建一个 `pointer-events-none` 的 #system-ui 容器，弹窗会打进穿透容器导致不可交互（注释自己都写了容器是 pointer-events-none，而 DialogOverlay 注释声称"已处理"，实际 `pointer-events-auto` 在 overlay 上——两层语义极易踩坑）。建议删除宿主层注释与回退分支，直接挂 body。

### BUG-013 | 波纹特效无后台暂停：标签页隐藏后仍每 1s 全屏 WebGL 重绘（Low）
**File(s):** `src/components/skin/effects.tsx:150-183`
**Severity:** Low
**Observation:** setTimeout 驱动的优势（后台节流到 1s 仍流动）也是成本：后台标签页每 1s 一次全屏 shader 计算 + 合成。桌面应用窗口最小化时同理。建议监听 `visibilitychange` 暂停循环（还原时补一帧），省电且延长设备电池。

---

## 二、ARCH — 架构

### ARCH-001 | 应用注册表 6 项，导航只暴露 4 项：plugins/steward 应用不可达（High）
**File(s):** `src/lib/app-registry.tsx:71-122`（APPS 含 chat/coding/wiki/settings/plugins/steward）、`src/components/classic/ClassicShell.tsx:30-32`（NAV_APPS 仅 chat/coding/wiki + 底部 settings 按钮）
**Severity:** High
**Observation:** 桌面壳退役后开始菜单/窗口层随之删除，但应用注册表仍是桌面形态的"6 应用"模型：`APPS.plugins` 与 `APPS.steward` 没有任何 UI 入口（`setActiveNav` 仅被 `switchTo` 调用，而 switchTo 只处理导航条上的 4 项；`activeNav` 初始值也只会落在 chat）——**这两个"应用"当前不可达**，其承载的 `PluginsAppView`/`StewardAppView` 包装、`defaultSize`、`gradient`、`desktop.app.plugins/steward` nameKey 全是死重量（注意：PluginsSettings/StewardSettings 组件本体仍被 SETTINGS 注册表使用，活）。同时 APPS 表仍在渲染时无条件被 `APPS[activeNav]` 索引——若 localStorage 残留 `activeNav=plugins`（桌面形态时代写入），会渲染出一个软件形态下无入口的页面，属状态兼容残留。
**建议：** 拍板二选一：① 删除 plugins/steward 两个 AppId（组件保留在 SETTINGS 里，AppId 收敛为 4 项，`APP_IDS` 与 `APPS` 一并收口）；② 在导航条给 plugins/steward 加入口。当前"注册表承诺 6 项、导航只给 4 项"是架构口径不一致，迟早绊倒人。

### ARCH-002 | 单 store 三域状态持续膨胀（Medium，继承上轮 A-26）
**File(s):** `src/stores/app-store.ts`（全文 898 行；上轮审查时 836 行，40 天增长 ~62 行）
**Severity:** Medium
**Observation:** 桌面壳窗口状态（viewMode）+ 聊天流状态（streamingText/streamingToolCalls/taskProgress）+ 会话/项目/皮肤/外观偏好同存一个 zustand store。上轮已登记"多会话并行时按 slice 拆分"，本轮补充两个可立即执行的轻量切分：① 皮肤/外观偏好块（accent/reduceMotion/skin/skinParams/skinBackground/skinWallpaper/backgroundEffect/skinAuto/viewMode，13 项）与聊天流状态互相无订阅关系，可整体抽 `useAppearanceStore`；② `streaming*` 系列是单会话语义（finalizeStream 直接固化为 messages），未来专家团队多会话并行（已拍板属模型层）必然重写，届时 `streamingText` 无法表达多会话——建议在 HANDOFF 登记"streaming 状态单会话假设"的失效点。
**建议：** 本轮先抽外观切片（低风险、纯搬移）；多会话语义登记不动作。

### ARCH-003 | 权限模式映射逻辑两处独立实现且 UI 不同步（Medium）
**File(s):** `src/stores/app-store.ts:394-422`（loadPermissionMode/setPermissionMode）；`src/components/settings/PluginsSettings.tsx:51-88`（loadPermissionMode/changePermissionMode）；`src/components/chat/ChatInput.tsx:102-105`（挂载时 load 一次）
**Severity:** Medium
**Observation:** "yolo = permissive + allowDangerous、default = 不设置、其余透传"的映射逻辑在 store 与 PluginsSettings 各写一遍（改一处漏一处的风险点）。且 ChatInput 的权限模式选择器只在挂载时 `loadPermissionMode()` 一次（注释自认"设置页修改后刷新页面同步"）——聊天工具条与插件设置页两处选择器**不做跨页同步**：在设置页切到 YOLO 后回聊天，工具条仍显示旧模式直到刷新/重挂。这是"单一数据源"缺失的直接后果。
**建议：** 删除 PluginsSettings 的本地实现，统一走 store 的 loadPermissionMode/setPermissionMode；ChatInput 改为在 `permissionMode` 变化/窗口 focus 时刷新，或把设置页的修改动作经 store 广播。

### ARCH-004 | DockLayout 的 ref 通道是死 API（Low）
**File(s):** `src/components/layout/DockLayout.tsx:35-38,82,133`（forwardRef + useImperativeHandle 暴露 resetLayout）
**Severity:** Low
**Observation:** 布局重置实际经模块级注册表 `dockHandles`（resetDockLayout）完成——注释说明原因（壳层无 ref 通道），但 `forwardRef`/`useImperativeHandle`/`DockLayoutHandle` 接口仍保留且**全库无任何 ref 使用者**（ChatAppView/CodingApp 均无 ref）。双重通道（ref + 注册表）只剩其一，建议删 ref 通道，避免两个"官方入口"的语义分裂。

### ARCH-005 | 外观偏好状态双写：zustand + 手动 localStorage（Low）
**File(s):** `src/stores/app-store.ts`（skin 系列 294-379 行各处 `localStorage.setItem` + `set`）；`src/lib/skin.ts:97-194`（load/save 函数族）
**Severity:** Low
**Observation:** 皮肤/外观偏好既不进 zustand persist（注释明示"手动读写"），也没有统一存取器——同一 localStorage 键被 store action 与 lib 函数两处写（如 `saveSkinParams` 与 `setSkinParam` 各写一次同键）。键值约定散落（"boenmind.skin"/"boenmind.skin.params"/"boenmind.skin.background"/"boenmind.skin.wallpaper"/"boenmind.skin.effect"/"boenmind.skin.auto"/"boenmind.appearance.*"…10+ 键），无单一清单。建议：抽一个 `skin-storage.ts` 集中键定义与读写（迁移成本极低），或在 ARCH-002 的外观切片里顺带收口。

---

## 三、SEC — 安全

前端为薄客户端（数据面全在后端，上轮 P0-3 Origin/CSRF 已修），本轮安全面发现较少，全部 Low：

### SEC-001 | 访问令牌明文存 localStorage（Low）
**File(s):** `src/api/client.ts:383-401`
**Severity:** Low
**Observation:** `boenmind.token` 明文 localStorage。桌面（Tauri WebView2）无第三方脚本注入面，风险低；网页部署（Linux standalone 模式）下若后端被 XSS 波及，令牌可被读出。替代：内存 + sessionStorage 混合，或仅对部署模式启用。登记即可。

### SEC-002 | 文件预览 iframe 无 sandbox（Low）
**File(s):** `src/components/files/FilePreview.tsx:96-101`（`<iframe src=data:application/pdf...>`）
**Severity:** Low
**Observation:** PDF 预览 iframe 无 `sandbox` 属性（无 `allow-scripts` 等）。PDF 在 Chromium 内置查看器渲染，脚本执行面很小，但本地文件预览属"不可信内容渲染"场景，纵深防御上加 `sandbox=""` 零成本（内置 PDF 查看器在 sandbox 下正常工作）。

### SEC-003 | 外部背景图 URL 经 img 加载，无协议白名单（Low）
**File(s):** `src/components/skin/SkinBackground.tsx:35-40`（`<img src={bg.value}>`）、`src/lib/skin.ts:197-219`（compressImageFile/sampleImage）
**Severity:** Low
**Observation:** 用户可填任意 URL 作为背景（含 `file://`、内网地址）——仅造成用户自身环境的资源加载/隐私泄露（DNS 探测），且 sampleImage 对跨域图会失败降级（已处理）。本地应用可接受，登记即可。

---

## 四、PERF — 性能

### PERF-001 | 多个独立 5s 轮询且无错误退避（Medium）
**File(s):** `src/App.tsx:45`（refreshHealth 5s）、`src/components/settings/McpSettings.tsx:55`（load 5s）、`src/components/settings/StewardSettings.tsx:34`（load 5s）
**Severity:** Medium
**Observation:** 三处轮询各自独立、全量 GET；后端未就绪/断网时仍每 5s 发一次失败请求（无退避/暂停）。App 的 refreshHealth 有 `online` 状态但轮询不因 offline 降频；MCP/Steward 页面轮询无任何失败处理差异。与上轮 C-F14（后端轮询订阅无退避）同主题的前端侧。
**建议：** 统一一个 `usePolling(fn, interval, {pauseOnOffline})` 帮助函数：offline 时暂停或降频到 30s；恢复在线立即补一次。

### PERF-002 | 波纹特效无可见性暂停（Low）
**File(s):** `src/components/skin/effects.tsx:150-183`（同 BUG-013）
**Severity:** Low
**Observation:** 同 BUG-013，归入性能维度再记一笔：后台节流至 1s 仍持续全屏 WebGL 重绘，长挂机场景持续耗电。建议 visibilitychange 暂停。

### PERF-003 | ChatPane 预览计算与流式重渲染的解耦正确，无需改（提示项）
**File(s):** `src/components/chat/ChatPane.tsx:69-75`
**Severity:** Low（信息项）
**Observation:** `previews` 以 `[messages, t]` 为依赖——流式期间 messages 引用不变（增量走 streamingText），memo 不重算，设计正确。仅提示：`t`（翻译函数）在语言切换时会使整表重算，量级可接受。无需动作。

---

## 五、QUAL — 代码质量

### QUAL-001 | 死代码全景（详见"精简代码专项"章节，此处不重复）

### QUAL-002 | SSE 流解析三处复制（Medium，继承上轮 B-QUAL-005，仍存在）
**File(s):** `src/api/client.ts:759-790`（chat）、`826-851`（subscribeEvents）、`870-895`（subscribeTerminal）
**Severity:** Medium
**Observation:** 上轮已报（行号当时 635-766，本轮文件已长到 947 行，复制体漂移到 759/826/870），仍存在且三处行为已有分叉：chat 与 subscribeEvents 有 401 处理、subscribeTerminal 没有；错误处理注释措辞各异。三处共同的缺陷（无重连、无背压）见 BUG-009。建议抽 `readSSEStream(res, onData)` 单一实现（~20 行），三处改为调用，顺带统一 401 行为。

### QUAL-003 | 插件/Skill 设置对话框大量重复（Medium）
**File(s):** `src/components/settings/PluginSettingsDialog.tsx`（453 行）vs `src/components/settings/SkillSettingsDialog.tsx`（277 行）
**Severity:** Medium
**Observation:** 两对话框共享同一 schema 模型（SettingField），但各自实现了一套 `groupOf/groupTitle/groupInstances/addGroupInstance/removeGroupInstance/toggleCollapsed/toggleClear/setField/load`（约 100+ 行重复逻辑；`removeGroupInstance` 甚至行为分叉：插件版删除后做实例编号前移压缩、skill 版直接删）。差异点（quota/测试/恢复默认）以外应抽共享 `useSettingsForm(schema, load, save)` hook + `SettingsGroupList` 组件。

### QUAL-004 | 权限映射三处（已归 ARCH-003）与 git 状态获取两处
**File(s):** `src/components/coding/GitGraph.tsx:27-35` vs `src/components/files/FilePanel.tsx:47-53`（各自 `api.gitInfo(projectRoot)`）
**Severity:** Low
**Observation:** 同一 git 数据被两个面板独立拉取（分支图与文件树徽标各自请求）。面板多开时同数据 N 份请求。可接受（面板生命周期独立），但若未来加 git 状态轮询应做共享订阅。登记。

### QUAL-005 | 局部 formatTime 重复实现（Low）
**File(s):** `src/lib/utils.ts:16-24` vs `src/components/settings/LogsSettings.tsx:26-30`
**Severity:** Low
**Observation:** LogsSettings 自带一个 `formatTime(tsMs)`（HH:MM:SS）与 utils 的实现无关（一个格式化会话时间、一个格式化日志时间）。utils 的 `formatTime` 默认语言写死 `"zh"`（`intlLocale(lang ?? "zh")`）——当前唯一调用方 SessionList 传了 `i18n.language` 所以无表现问题，但函数签名默认值是个隐患（新调用方忘传即固定中文格式）。建议默认值改取 `i18n.language`。

### QUAL-006 | oxlint 22 条警告全量清单（Low）
**File(s):** `npx oxlint src`（22 warnings / 0 errors）
**Severity:** Low
**Observation:** 分布：`react(only-export-components)` 15 条（Fast refresh：注册表/皮肤工具文件混合导出组件与常量——多为设计权衡，可整文件 `// oxlint-disable` 并集中到文件头）；`react-hooks(exhaustive-deps)` 5 条（StewardSettings/McpSettings/SkillsSettings/RefinementSettings/PluginsSettings 的 `useEffect(..., [])` 缺 `load` 依赖——`load` 是组件内 `async` 函数每次渲染新建，放进依赖会死循环，当前写法是"明知故犯"，建议改为 `useCallback` 或模块级函数）；`no-unused-vars` 1 条（skin.ts:111 `SKIN_AUTO_KEY`）；`exhaustive-deps` 1 条（ScrollIndicators:60 的 tick 依赖——tick 是重算信号，属合理用法可 disable 注释）。

### QUAL-007 | 玄学写法残留：LogsSettings 死三元分支（Low）
**File(s):** `src/components/settings/LogsSettings.tsx:160`（`${paused ? "" : ""}`）
**Severity:** Low
**Observation:** 模板串里三目两分支相同，是"滚动暂停高亮"实现到一半的残留（下方已有 scrolledHint/following 文案提示）。删掉死分支。

### QUAL-008 | zh.ts `terminal` 段缩进错乱（Low）
**File(s):** `src/i18n/locales/zh.ts:128-130`（`terminal: {` 缩进 4 空格、其余段 2 空格；`createFailed` 行尾 `},` 后跟多余空行）
**Severity:** Low
**Observation:** 格式化瑕疵（en.ts 同段同样错乱，128-130 行）。prettier 一轮即可。

---

## 六、IMP — 改进建议

### IMP-001 | 前端无任何测试（Medium）
**File(s):** `src/` 全部（无 `*.test.*`/`*.spec.*`）；`package.json`（无 vitest/jest）
**Severity:** Medium
**Observation:** 前端 1.6 万行零测试；`lib/git-lanes.ts` 注释明言"供组件引用与测试"（computeLanes 是纯函数，正是测试友好面）但无测试文件。上轮 B 报告认可"前端 i18n/类型纪律"，但构建断裂（BUG-001）能被静默放过正说明缺少哪怕一条 `tsc -b` 的 CI 门槛。建议：① CI 先加 `pnpm build`（立刻拦下 BUG-001 这类问题）；② 为纯函数层（git-lanes、skin.ts 的 load/save、utils）补 vitest 冒烟测试。

### IMP-002 | markdown 渲染组件三处重复配置（Low）
**File(s):** `src/components/chat/MessageItem.tsx:159-178`、`src/components/files/FilePreview.tsx:70-84`、`src/components/team/ExpertTeamDocs.tsx:54-66`
**Severity:** Low
**Observation:** 三处 `ReactMarkdown` 的 remark/rehype/`pre` 覆盖完全同构（ExpertTeamDocs 还少 `a` 覆盖）。抽 `components/shared/Markdown.tsx`（含 typography 类，见 BUG-003）一处维护。

### IMP-003 | 无障碍缺口（Low）
**File(s):** 多处：`ChatInput.tsx:172-199`（占位按钮仅 title 无 aria-label，disabled 状态下 title 也不可达）、`ProjectSwitcher.tsx:97-107`（删除按钮无 aria-label）、`GitGraph` 节点纯图形无文本替代、色彩语义依赖（工具调用绿/红、git 徽标）
**Severity:** Low
**Observation:** 桌面应用可接受度较高，但 message actions 复制/分叉按钮 hover 才显示（opacity-0 group-hover:opacity-100）对键盘/触屏用户不可发现——至少加 focus 可见态。登记。

### IMP-004 | dockview 布局重置加确认/备份（Low，见 BUG-011）

### IMP-005 | vite 拆包：vendor-icons 组仅 2 个图标受益（Low）
**File(s):** `vite.config.ts:30-37`
**Severity:** Low
**Observation:** `@lobehub/icons` 组与 `vendor-markdown` 组是 rolldown codeSplitting 配置。实际使用面：icons 只 import 了 DeepSeek/MiniMax 两个 Color 图标（provider-presets.tsx:17-18，es 子路径导入）——打包图中只有这两个模块，独立 chunk 收益有限但不有害；markdown 链（react-markdown/remark-gfm/rehype-highlight/unified/hast/micromark…）拆独立 chunk 对首屏有真实收益（设置页/聊天共用）。结论：配置保留，无动作；仅提示 `@lobehub/icons` 若未来换用 lucide 品牌图标可整体移除该依赖（约 200KB 包）。

### IMP-006 | `.oxlintrc.json` 规则面过窄（Low）
**File(s):** `.oxlintrc.json`
**Severity:** Low
**Observation:** 插件声明了 `react/typescript/oxc` 但 rules 只配了 2 条 react 规则，typescript/oxc 的默认规则集基本未启用（oxlint 默认开启 correctness 类，22 warnings 已含部分）。建议显式启用 `"typescript": "error"`、`"oxc": "error"` 规则集并修到 0 warning——lint 是前端唯一静态防线（无测试），值得加码。

---

## 七、精简代码专项（独立章节）

### S-1 | 死依赖：react-rnd、react-resizable-panels（确认无引用）
**File(s):** `package.json:33-34`
**证据:** 全 src grep `react-rnd|react-resizable-panels` 零命中（dockview 已完全接管可停靠布局；react-resizable-panels 疑似桌面壳窗口缩放时代残留、react-rnd 疑似桌面壳悬浮窗时代残留）。
**建议:** 两个依赖直接删（src 无引用，删除零风险；`pnpm-lock.yaml` 一并清理）。这同时消除两棵依赖子树（react-rnd → re-resizable 等）的安装/审计面。

### S-2 | 死组件：ChatWindow、ExpertTeamDocs（继承上轮 A-14，确认仍存在）
**File(s):** `src/components/chat/ChatWindow.tsx`（11 行，ChatPane 薄包装）、`src/components/team/ExpertTeamDocs.tsx`（70 行）
**证据:** 全库零引用（grep 双侧确认）。上轮已报（A-14 死代码三件中的两件，另一件 clip_tool_output 在后端已随修复轮处理），本轮仍未删。
**建议:** 删除或加 `// kept as reference` 标注（ChatWindow 可留作形态示例，ExpertTeamDocs 连 i18n key `team.docLoadFailed` 一起删）。

### S-3 | 死导出：APP_LIST、AppEntry.gradient、AppEntry.defaultSize
**File(s):** `src/lib/app-registry.tsx:64,124`（`APP_LIST` 仅定义处；`gradient`/`defaultSize` 6 处定义）
**证据:** `APP_LIST` 全库零引用；`gradient` 字段零消费（桌面壳窗口层/开始菜单随退役删除，字段残留）；`defaultSize` 零消费。
**建议:** 删除 `APP_LIST` 导出与 `gradient`/`defaultSize` 字段（AppEntry 收敛为 `{id, nameKey, icon, component}`）。

### S-4 | 死组件文件：ui/separator.tsx（整体无引用）
**File(s):** `src/components/ui/separator.tsx`（43 行）
**证据:** `<Separator` 全库零引用（ChatInput 用的是 `SelectSeparator`，属 select.tsx）。
**建议:** 整文件删除（shadcn 生成物，随时可 regen）。

### S-5 | 死导出：DialogTrigger、DialogClose
**File(s):** `src/components/ui/dialog.tsx:171-182`
**证据:** 两导出全库零引用（DialogContent 用内部 `DialogCloseButton`；DialogOverlay 为内部使用，导出可留）。
**建议:** 删 `DialogTrigger`/`DialogClose` 导出（或留但接受——shadcn 常规导出面，优先删）。

### S-6 | 死状态：store.viewMode / setViewMode（桌面形态开关占位的另一半）
**File(s):** `src/stores/app-store.ts:100-101,294-298`；`src/App.tsx:2-4`（注释自认"渲染恒为 ClassicShell"）；`src/components/settings/AppearanceSettings.tsx:78,187-192`
**证据:** `setViewMode` 全库零调用；`viewMode` 仅 AppearanceSettings 用于"经典"卡片选中态回显（一个恒真的比较）。localStorage `boenmind.viewMode` 写入逻辑死。桌面形态切换入口（设置页"桌面形态"按钮）只 toast `desktopRemoved` 提示。
**建议:** 删除 `viewMode`/`setViewMode`（选中态改常量 `"classic"`）；保留 AppearanceSettings 的占位卡片与 `desktopRemoved` 文案（用户拍板留占位）。顺带删除 index.html 或注释里对桌面形态的引用（若有）。

### S-7 | 死 DOM 引用：#system-ui（见 BUG-012）

### S-8 | 不可达应用：APPS.plugins / APPS.steward（见 ARCH-001；`desktop.app.plugins/steward` i18n key 一并列入 S-9）

### S-9 | 死 i18n key 清单（约 24 个 × 4 语言 ≈ 96 条死翻译）
**File(s):** `src/i18n/locales/zh.ts`（及 en/ja/ko 同构）
**证据:** 静态引用核对（473 个静态 `t("...")` 引用 + 14 个动态前缀）：
- `nav.*` 全段 7 个（chat/team/gallery/knowledge/settings/gallerySoon/knowledgeSoon）——导航表退役后零引用（软件导航用 `desktop.app.*`）；
- `team.docLoadFailed`（随 S-2 删除）；
- `desktop.*` 段 13 个中 11 个死：bootSkip/startMenu/dock/emptyHint/windowClose/windowMinimize/windowMaximize/switchClassic/switchDesktop/codingComingSoon/toolDesc（classicNav 与 app.wikiDesc 仍活；app.chat/app.coding/app.settings 活——导航按钮用；**app.plugins/app.steward/app.chatDesc/app.codingDesc 死**）；
**建议:** 一次清理轮删除上述 key（四语言同步），并给 i18n 加"key 引用完整性"的 CI 脚本（t(`...`) 动态前缀白名单 + 静态引用对比），防止死 key 再度积累。

### S-10 | 死分支：effects.tsx reduceMotion（BUG-004）、LogsSettings paused（QUAL-007）

### S-11 | 重复代码合并清单（跨维度汇总）
| 重复体 | 位置 | 合并建议 |
|---|---|---|
| SSE 流解析 ×3 | client.ts:759/826/870 | `readSSEStream` 单函数（QUAL-002） |
| 权限模式映射 ×2 | app-store.ts:394-422 / PluginsSettings.tsx:51-88 | 收口到 store（ARCH-003） |
| 设置对话框表单逻辑 ×2 | PluginSettingsDialog / SkillSettingsDialog | `useSettingsForm` hook（QUAL-003） |
| Markdown 渲染配置 ×3 | MessageItem/FilePreview/ExpertTeamDocs | `shared/Markdown.tsx`（IMP-002） |
| git 状态拉取 ×2 | GitGraph / FilePanel | 共享订阅（QUAL-004） |
| formatTime ×2 | utils / LogsSettings | 各自保留或统一（QUAL-005） |
| 皮肤 localStorage 存取 ×12 函数 | lib/skin.ts:97-194 | 键定义收口到单文件（ARCH-005） |

### S-12 | 超长文件拆分建议
**File(s):** `src/api/client.ts`（947 行）、`src/stores/app-store.ts`（898 行）
**证据/建议:** 两者都是"薄 API 声明 + 薄状态定义"为主，但已越过易读线：
- client.ts：API 面按域分文件（`api/sessions.ts`/`api/plugins.ts`/`api/workspace.ts`…）合并导出，或至少把类型定义（约 40 个 interface/type，前 370 行）抽 `api/types.ts`；SSE 解析抽公共函数（QUAL-002）后可减 ~100 行。
- app-store.ts：外观/皮肤块（~90 行）+ 项目块（~80 行）+ 会话块（~200 行）+ 聊天流块（~150 行）可先按 ARCH-002 抽外观切片，再按需分片；`finalizeStream`/`refreshLastTask` 与 `handleEvent` 是内部闭包互相引用的耦合点，拆 slice 时注意（zustand 的 `get()` 可跨 slice 引用）。

### S-13 | 过度抽象/冗余 API
- DockLayout forwardRef 通道（ARCH-004）；
- `skinParamValue`/`skinById` 之外 `loadSkinId` 与 store 初始化重复读取 localStorage（store.ts:330-340 初始化时 `loadSkinId()` 两次调用 + applySkin）；
- `lib/dock-open.ts:80` 的 `export { VIEWS }` 再导出（dock-views 已导出，多一层转发）；
- `AppSettingsChat`/`AppSettingsCoding` 两个 3 行包装组件（app-registry.tsx:47-52）——可改为 `AppSettings` 直接注册 + `appId` prop，注册表需要 `ComponentType` 无参签名是唯一理由，可接受；登记。

### S-14 | 包体积与配置精简评估
- 运行时依赖 21 项中 2 项死（S-1）；devDependencies 中 `shadcn`（CLI 用，保留）、`@types/node`（vite.config 用，保留）；
- `@tauri-apps/plugin-process`、`@tauri-apps/plugin-updater` 在 src 零引用（`grep @tauri-apps` 仅 AboutSettings 动态 import `@tauri-apps/api/core`）——**两个 Tauri 插件依赖疑似死依赖**（桌面壳侧可能由 src-tauri 使用，**前端侧确认无引用**，建议核对 src-tauri 的 Cargo 配置后从 package.json 删除——标注待核）。
- `components.json`（shadcn CLI 配置）保留；`.oxlintrc.json` 建议扩规则面（IMP-006）；
- index.css 中 `@import "shadcn/tailwind.css"`（shadcn v4 的基础样式包）与 `@fontsource-variable/geist`、`dockview.css` 均有实际作用，保留；
- 全局滚动条美化（index.css:224-244）与 `.scrollbar-none`（211-219）并存——前者对全部元素生效，后者仅消息区；两处 `::-webkit-scrollbar` 规则有重复面，可合并（Low）。

---

## 八、发现统计

| 类目 | 数量 | 其中 Critical | High | Medium | Low |
|---|---|---|---|---|---|
| BUG | 13 | 1 | 2 | 6 | 4 |
| ARCH | 5 | 0 | 1 | 3 | 1 |
| SEC | 3 | 0 | 0 | 0 | 3 |
| PERF | 3 | 0 | 0 | 2 | 1 |
| QUAL | 8 | 0 | 0 | 3 | 5 |
| IMP | 6 | 0 | 0 | 2 | 4 |
| 精简专项 S-1~S-14 | 14 | 0 | 1 | 2 | 11 |
| **合计** | **52** | **1** | **4** | **18** | **29** |

（注：精简专项与六类维度有部分重叠引用，如 S-2 对应 QUAL-001/上轮 A-14、S-10 对应 BUG-004；统计按条目去重后约 48 个独立发现。）

---

## 九、Top 10 优先处理清单

| 序 | ID | 严重度 | 问题 | 修复量 |
|---|---|---|---|---|
| 1 | BUG-001 | Critical | ja/ko 缺 `namePrefixHint` → `tsc -b` 失败，**构建当前断裂** | 2 行翻译 + 删 1 死常量 |
| 2 | BUG-002 | High | en/ko 第 571 行 `uninstall: "卸载"` 中文残留（值级漏译） | 2 行 |
| 3 | BUG-003 | High | 三处 markdown 用 `prose` 但未装 @tailwindcss/typography，排版未生效 | 装包 + index.css 一行 |
| 4 | BUG-006 | High | "蓝色波纹"静止：先做候选 A'（classic 皮肤对照 / isolation 实验）与候选 L（u_time 精度，一行实验） | 排查 + 小修 |
| 5 | S-1/S-2/S-3/S-4/S-5 | High | 精简首轮：删 2 死依赖 + 2 死组件 + 死导出（APP_LIST/gradient/defaultSize/separator/DialogTrigger） | 纯删除，半小时 |
| 6 | S-9 | Medium | 死 i18n key ~24 个 ×4 语言清理 + 加 key 完整性 CI 检查 | 一次清理轮 |
| 7 | ARCH-001 | High | plugins/steward 应用不可达：注册表收口为 4 项或补导航入口 | 定调 + 小改 |
| 8 | ARCH-003/QUAL-003 | Medium | 权限模式映射收口到 store（跨页同步）+ 设置对话框抽共享 hook | 中 |
| 9 | BUG-009/QUAL-002 | Medium | SSE 解析三处统一 + `useSessionEventStream` 重连（TodoPanel 断连无恢复） | 中 |
| 10 | BUG-004/BUG-013 | Medium/Low | reduceMotion 静态帧落实 + visibilitychange 暂停特效循环 | 小 |

---

*工具A code-review 独立审查完毕。所有结论基于源码通读与实证（tsc/oxlint/grep 双端核对）；波纹动画部分为候选假设，待真实浏览器验证。*
