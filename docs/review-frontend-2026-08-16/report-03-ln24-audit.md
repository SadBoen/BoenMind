# Architecture Audit —— BoenMind 前端（工具C ln-24 独立审查）

> 工具C ln-24 独立审查，未看其他两份报告（TOOL_A / TOOL_B）。
> 按任务指示阅读了 `docs/REVIEW_TOOLS_CROSS_2026-08-16.md`（交叉结论，避免重复）与
> `docs/archive/HANDOFF_BG_EFFECT_ANIMATION.md`（已知 bug 背景）作为背景输入。
> 审查范围：仅 `frontend/src`（80 个源文件 16,259 行）+ `package.json` / `vite.config.ts` /
> `index.html` / `tsconfig*.json` / `components.json`。**未审** backend/、src-tauri/（仅核实依赖归属）、
> node_modules/、dist/。只读审计，未修改任何代码与文档。
> 日期：2026-08-16。

**Checklist: 35/35 complete**<br>**Incomplete: None**（编译/运行实证未执行——只读审计未跑 tsc/build，所有结论基于静态证据链，已在相关 Finding 中标注假设等级）

---

## Verdict: FAIL

一条已实证的正确性边界缺陷（P1）：会话分叉（fork）功能把客户端乐观生成的消息 id
（`Date.now()` 时间戳）当作服务器消息 id 提交给后端契约——答复刚完成时点"分叉"
在主流使用路径上必然失败。另有 5 条 P2（其中背景特效动画不生效为已确认未解 bug，
本报告补充所有权/生命周期角度的候选原因，标注假设）与 4 条 P3（死依赖/退役残留）。

---

## Actual architecture（实际执行的架构）

### 形态与入口
- **单页应用，无路由**：`main.tsx`（StrictMode + next-themes ThemeProvider + TooltipProvider + Toaster）→ `App.tsx`（TokenGate + SkinBackground + ClassicShell 恒渲染）。桌面形态已退役（2026-08-16 用户拍板"全删除"），`viewMode` 状态与持久化残留但**渲染恒为 ClassicShell**，`setViewMode` 无任何 UI 调用者。
- **双后端接入形态**：Vite 开发代理（/api → 127.0.0.1:17321，`BM_API_TARGET` 可覆盖）+ Tauri 桌面注入 `window.__BOENMIND_API__`（index.html 内联脚本）；`api/client.ts` 按优先级读取 `VITE_API_BASE` → 注入值 → 同源。
- **前端工程**：Vite 8 + rolldown codeSplitting（@lobehub/icons 与 markdown 链拆独立 chunk）；Tailwind v4 无 tailwind.config，令牌全在 `index.css` `@theme inline` + `:root`/`.dark` oklch 变量；`@base-ui/react` v1.7（shadcn style "base-nova"）；pnpm 11。

### 模块与边界
| 层 | 位置 | 职责 | 边界性质 |
|---|---|---|---|
| 壳层 | `components/classic/ClassicShell.tsx` | 导航（NAV_APPS 三应用 + 设置入口）、主面板渲染 `APPS[activeNav].component`、状态栏、dock 重置右键菜单 | 纯渲染，无业务逻辑 |
| 应用注册表 | `lib/app-registry.tsx` | `APPS: Record<AppId, AppEntry>`（chat/coding/wiki/settings/plugins/steward）+ `SETTINGS: Record<SettingsTab, SettingsEntry>`（13 页）——单一数据源，编译期保证完整性 | 注册表驱动，组件零改动嵌入 |
| 布局系统 | `components/layout/DockLayout.tsx` + `lib/dock-views.tsx` + `lib/dock-open.ts` | dockview-react 8.1 可停靠容器：`VIEWS` 视图注册表（7 视图）、`DEFAULT_LAYOUTS` 每应用默认布局、`boenmind.dock.v9.<appId>` 版本化快照持久化、`dockHandles` Map 注册表 + `apiAppMap` WeakMap（header action 反查应用） | 宿主能力；视图=公共组件零改动嵌入 |
| 全局状态 | `stores/app-store.ts`（898 行，唯一 zustand store） | 导航/设置分级/外观/皮肤/健康/配置/会话/聊天流式/权限/项目/文件/todos 全部集中；局部闭包持有 `streamController` | 所有组件经 hook 订阅，无 prop 穿透 |
| API 契约层 | `api/client.ts`（947 行） | 60+ REST 端点 + 3 条 SSE 流（chat/subscribeEvents/subscribeTerminal）；统一 `request()` 错误解析（401 → `notifyUnauthorized` 回调 → TokenGate）；模块级 `authToken`/`unauthorizedHandler` 单例 | 与后端契约**全凭注释对齐**（"对齐后端 xx"），无运行时校验 |
| 皮肤系统 | `lib/skin.ts` + `skins/index.ts` + `skins/glass/style.css` + `components/skin/*` | `SKINS` 注册表（classic/glass）、`PRESET_WALLPAPERS`（5 款）、`BACKGROUND_EFFECTS`（none/wave）扩展点；`data-skin` 属性 + `--skin-*` CSS 变量注入；背景层 z-0 / 内容 z-10 | 皮肤只覆盖 CSS 令牌，组件零改动；可逆（去属性即还原） |
| 外观 | `lib/appearance.ts` | 字体档位（html 根字号）/ 强调色（data-accent）/ 减少动画（html.reduce-motion 类） | 纯前端偏好，localStorage |
| i18n | `i18n/index.ts` + `locales/{zh,en,ja,ko}.ts` | i18next v26 内联四语言；`boenmind.lang` 本地即时 + 后端 config.lang 为权威（启动校正） | 语言有明确单源规则 |
| 视图组件 | `components/{chat,coding,files,terminal,team,settings,ui}` | 共享宿主组件（ChatPane/TerminalPane/FilePanel/Editor/TodoPanel/GitGraph）+ 设置页 + shadcn ui 原语 | 经 VIEWS/APPS/SETTINGS 注册表接入 |

### 关键流与运行时接线
1. **启动流**：App 挂载 → `applyFontScale/applyAccent/applyReduceMotion/applySkin`（从 localStorage 恢复）→ `refreshHealth`（5s 轮询，变更检测防无谓重渲染）+ `loadConfig`（**只校正 lang，不应用 config.theme**）+ `loadSessions`（校验恢复的 activeSessionId/appSessionIds）。
2. **聊天流**：ChatInput → `sendMessage`：本地乐观追加用户消息（`id: Date.now()`）→ `api.chat` SSE（7 事件 union 契约）→ textDelta/toolCallStart/toolCallEnd/taskProgress/permissionRequest/done/error 投影 → `finalizeStream` 固化助手消息（**同样 `id: Date.now()`**）→ 只 `loadSessions()`，**不重拉消息** → 停止：`stopChat` + 8s 本地兜底 finalize。
3. **事件投影流**：TodoPanel 与 FilePanel **各自** `api.subscribeEvents`（after=0 全量重放 + 实时）——同一活跃会话两条并发 SSE 长连接；todo/write → store.setTodosFromEvent；tool/result、turn/end → 防抖刷新文件树。
4. **布局流**：DockLayout onReady → 恢复快照（fromJSON，restoringRef 防回写）→ 无快照构建默认布局 → onDidLayoutChange 防抖 500ms 落盘 `boenmind.dock.v9.<appId>`；重置经 `dockHandles` Map（避免 ref 穿透）。
5. **皮肤流**：AppearanceSettings 写 store → store action 同步 localStorage + `applySkin`（data-skin 属性）→ SkinBackground 按 skin===glass 渲染壁纸层（自定义图 > 预设渐变/流体 > 默认色调渐变）→ 特效层（wave→EffectWave WebGL2 30fps）→ 明暗遮罩。
6. **更新流**：AboutSettings → /api/updates/check|apply → managed 模式动态 import `@tauri-apps/api/core` 调 `backend_restart`（src-tauri 自定义命令）→ 轮询 health 至新版本 → location.reload()。

### 当前/目标/过渡态与漂移
- **已接受的退役态**（有文档记录）：桌面壳全删（HANDOFF_DESKTOP_SHELL.md 交接、App.tsx/AppearanceSettings 注释）——但 **viewMode 状态、setViewMode action、`boenmind.viewMode` 持久化、APPS.plugins/steward 两个注册条目、AppEntry.defaultSize 字段全部残留且无消费者**（漂移：注释说"留切换开关占位"，实际开关点击只弹 toast，开关本体（setViewMode）已无调用者）。
- **契约漂移**：注释宣称"语言与主题以后端 config.toml 为准"，可执行代码只对 lang 执行一致性校正，theme 只写不读。
- **皮肤系统**：与用户拍板一致（2026-08-16 系列 commit 实证），注册表均有消费者（BACKGROUND_EFFECTS 被设置页与 SkinBackground 消费）。
- **Git 状态**：最近提交为背景特效系统（7dcf783/05b5052）；工作区唯一未跟踪文件 `frontend/check-i18n.cjs`（用户自写 i18n 键对账脚本，非既定架构，不构成审计对象）。

---

## Fitness summary

| Area | Status | Evidence |
|---|---|---|
| Pattern fitness and ownership | CONCERNS | 注册表三件套（APPS/VIEWS/SKINS）有真实消费者且编译期兜底，模式适配合格；但退役态残留（viewMode/APPS.plugins-steward/死组件/死依赖）是无消费者兼容路径的活样本；SSE 订阅所有权分散在两个组件（双连接）；EffectWave 的 reduce-motion 契约失效（宣称静态帧、实际持续动画） |
| Contracts and boundaries | FAIL | 分叉功能把客户端乐观 id 泄漏进服务器消息契约（P1）；API 契约零校验、纯注释对齐（已发生一次实机字段不齐故障 42eea24）；SSE 解析三处复制已开始行为分叉（401 处理在 terminal 流缺失）；ChatStreamEvent union 契约本身清晰 |
| Dependency topology | PASS | 无环；依赖方向全部单向（api ← store ← components）；dockview 确有消费者（DockLayout/dock-views/dock-open 三文件）；死依赖（react-rnd/react-resizable-panels）确认零引用，属清理项而非拓扑缺陷 |
| Physical structure and configuration | CONCERNS | components 按域分组内聚良好；`boenmind.*` 17+ 键无集中注册表（皮肤键在 skin.ts 集中、其余散在 store action 内）；theme 双轨（config.toml 写 / next-themes localStorage 读）无启动同步，与 lang 的单源规则不对称 |

---

## Findings

### P0

None。未发现数据丢失、安全或崩溃级缺陷。

### P1

| 优先级 | 问题 | 证据与理由 | 必需解决方向 |
|---|---|---|---|
| P1 | 分叉功能把客户端乐观消息 id 提交给服务器契约，答复刚完成时必然失败 | 见 F1 | 流结束后重拉消息或后端按会话内序号定位 |

**F1（P1）：fork 使用客户端伪造的消息 id —— 乐观 UI 与服务器契约边界未收口**
- **边界**：`stores/app-store.ts`（乐观消息）→ `api/client.ts` `forkSession` 契约 → `components/chat/MessageItem.tsx`（分叉按钮）。
- **证据**：`finalizeStream`（app-store.ts:253-282）固化助手消息 `id: Date.now()`；`sendMessage`（:701-707）用户消息同样 `id: Date.now()`；流结束只调 `loadSessions()`（:785），**不重拉消息**——当前会话内存列表的消息 id 全部是客户端时间戳。`MessageItem.tsx:93` 把 `message.id` 传给 `forkFromMessage` → `api.forkSession(srcId, atMessage)`（app-store.ts:632-639，请求体 `{ at_message: <client id> }`）。客户端 13 位毫秒时间戳与服务器自增行 id 永不相交：答复末尾刚完成的那条消息（分叉按钮最常见的触发对象）按 id 定位必然失败。历史消息（`getSession` 加载，服务器 id）能正常工作——即同一功能两条路径行为不一致。
- **材料性后果**：2026-08-16 用户定调上线的"答复末尾分叉"在主流路径（刚答完立即分叉）必然报错；无数据损坏但功能名存实亡，且用户无法从 UI 判断哪条消息可分叉。
- **为何当前折衷不可接受**：前端把本地临时身份（乐观 id）泄漏进持久化 API 契约，是"谁拥有消息 id"的所有权没有划清——前端持有展示态，服务器持有权威态，二者从未对账。
- **迁移风险**：小（不涉及后端契约变更，见下）。
- **最小安全下一步**（二选一，单步可完成）：(a) `sendMessage` 的 `done` 事件后补一次 `getSession(activeSessionId)` 重拉消息列表，使内存 id 与服务器一致（顺带修复错误消息 id 残留）；(b) 若后端支持，fork 契约改为按"会话内消息序号/seq"定位，前端不再传 id。回滚：各自 revert 即可。
- **实践参考**：[React useOptimistic（官方文档：乐观状态必须与服务器回执调和，本地值不作权威）](https://react.dev/reference/react/useOptimistic)

### P2

| 优先级 | 问题 | 证据与理由 | 必需解决方向 |
|---|---|---|---|
| P2 | reduce-motion 对背景特效无效，且仍保持 30fps 帧生产 | 见 F2 | 冻结时间源或跳过渲染循环 |
| P2 | 主题双轨（config.theme 写 / next-themes localStorage 读）无启动同步，与 lang 单源规则不对称 | 见 F3 | loadConfig 校正 setTheme |
| P2 | 背景特效"蓝色波纹"动画不生效（已确认未解 bug）——补充所有权/生命周期候选原因 | 见 F5 | 按交接路径 2D canvas 重写 + 真实浏览器验证 |
| P2 | API 契约零校验、纯注释对齐，已发生一次实机字段不齐故障 | 见 F4 | 后端出 schema 或关键端点加校验 |
| P2 | SSE 解析三处复制（chat/subscribeEvents/subscribeTerminal），行为已开始分叉 | 见 F6 | 抽统一 parseSSE helper |

**F2（P2）：reduce-motion 契约对 EffectWave 失效——声明"静态帧"、实现持续动画**
- **边界**：`components/skin/effects.tsx`（特效帧循环）与 `lib/appearance.ts` / `index.css .reduce-motion`（无障碍契约）。
- **证据**：effects.tsx:161-168 循环体两分支（`if (reduceMotion)` 与正常路径）**都调用 `render()`**，而 render 恒传 `performance.now()/1000`（实时时钟）——reduce-motion 开启时波纹照常流动，与文件头注释"reduce-motion 时渲染静态帧（与无障碍偏好兼容）"（:7）和设置项语义直接矛盾；同时该用户仍承受 30fps WebGL 帧生产（GPU 空转，`.reduce-motion` 的 CSS 规则管不到 JS 驱动的 canvas）。
- **材料性后果**：用户显式开启的"减少动画"对默认开启的背景特效不生效（辅助功能偏好失效）；与 HANDOFF_BG_EFFECT_ANIMATION.md 假设 B（"检查 reduceMotion 是否意外为 true"）方向相反——即使为 true 也不会静止，给排查动画 bug 制造了第二个干扰面。
- **最小安全下一步**：`loop` 中 reduce-motion 分支改为**不调度下一次**（或冻结时钟，render 一次后停表），单文件 3 行改动；顺带在特效层响应 `prefers-reduced-motion` 媒体查询可进一步对齐系统级偏好（可选）。
- **实践参考**：[MDN prefers-reduced-motion（减少动画偏好的标准语义）](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion)

**F3（P2）：主题双轨——config.toml 写透、启动从不读回；与 lang 的单源规则同类别不对称**
- **边界**：`AppConfig.theme` / `HealthInfo.theme`（后端权威候选）vs next-themes localStorage（键 `theme`，未命名空间）vs `loadConfig`（app-store.ts:457-478）。
- **证据**：`loadConfig` 明确"语言以后端 config.toml 为准"并 `applyLang` 校正（:462-464）；**theme 无任何读回路径**——App 挂载 effect（App.tsx:35-47）只 applyFontScale/applyAccent/applyReduceMotion/applySkin；AppearanceSettings.applyTheme 只写两端（setTheme + saveConfig，:146-156）；health.theme 仅在 refreshHealth 变更检测里被比较（app-store.ts:438），比较结果不消费。next-themes 默认持久化键 `theme` 与全项目 `boenmind.*` 命名约定也不一致。
- **材料性后果**：桌面（Tauri webview origin）与网页（localhost/部署域 origin）localStorage 互相独立、后端 config.toml 共享——用户在网页设深色、桌面启动仍回浅色；外部改 config.toml 的 theme 被前端忽略。同一"外观"配置，lang 有校正、theme 没有，规则不统一。
- **为何当前折衷不可接受**：既有约定（"共享外观以后端 config.toml 为准"）已写进代码注释与 lang 实现，theme 是同一规则的漏网；继续留下去双端主题漂移会随用户增多常态化。
- **最小安全下一步**：`loadConfig` 中 `if (config.theme && config.theme !== resolvedTheme)` 时 `setTheme(config.theme)`（注意挂载后执行，避免 hydration 抖动）；next-themes 可配置 `storageKey: "boenmind.theme"` 一并收编命名空间。单文件改动，可回滚。
- **实践参考**：[next-themes 官方文档（storageKey 配置与主题持久化语义）](https://github.com/pacocoursey/next-themes)

**F4（P2）：API 契约无校验、全凭注释对齐——已有实机故障先例**
- **边界**：`api/client.ts`（60+ 端点）↔ 后端 REST/SSE 面。
- **证据**：client.ts 全部契约以"对齐后端 xx"注释声明（如 :10-12 ProviderKind、:123 McpServerConfig、:356 ChatStreamEvent），无任何运行时/构建时校验层（package.json 无 zod/io-ts；无 OpenAPI 消费）；git log 实证 `42eea24 "fix(settings): 前端字段名对齐后端 snake_case——作用域徽标与 SKILL 设置按钮实机修复"`——字段名不齐已造成一次实机功能损坏，靠人工实机发现。
- **材料性后果**：后端字段改名/新增必填字段的检测完全依赖人力冒烟；本仓库两周内已发生一次同族故障（42eea24、以及 fork 类 id 语义错配 F1 同属"注释契约无机器防线"）。随后端 9 crate 继续演进，漂移概率线性上升。
- **最小安全下一步**：不做全量重写——先让后端暴露机器可读契约（`/api/openapi.json` 或最小 `types` 端点），前端加一个启动时契约校验（关键端点 sample 校验，失败降级 + 显式警告）；或先为高频端点（config/sessions/chat）手写轻量运行时校验。有界、可回滚。
- **实践参考**：[OpenAPI 规范 v3.1（机器可读 API 契约的现行标准）](https://spec.openapis.org/oas/v3.1.0)

**F5（P2）：背景特效动画不生效（已确认未解）——从所有权/生命周期角度的候选原因补充**
- **边界**：`components/skin/effects.tsx` EffectWave（JS 帧生产）↔ CSS 合成（canvas + `mix-blend-overlay`）↔ 浏览器合成器；背景：`docs/archive/HANDOFF_BG_EFFECT_ANIMATION.md`（用户实机确认"没动静"；帧循环在跑、时间在变、GL 无错、截图 0% 差异）。
- **证据（本审计补充，全部标注假设，未实机验证）**：
  - **H1（与交接假设 A 同向，中等置信）**：混合模式的合成生命周期独立于 JS 帧生产——`mix-blend-mode` 使 canvas 成为混合根（backdrop root），Chromium 对 blend-mode 元素存在已记录的不重绘问题（见参考），本组件组合（透明 WebGL canvas + overlay 混合 + 无 will-change）与之吻合；交接文档的 2D canvas 重写方案即绕开此路径。
  - **H2（所有权盲区，高置信——这本身是可修的架构缺陷）**：渲染循环**只拥有"写入 drawing buffer"的所有权，不拥有"帧是否被呈现"的任何可观测回路**——没有帧计数输出、没有合成器反馈、没有降级路径；唯一验证手段是肉眼/截图，这正是交接文档排查周期的根因。任何动画类 UI 都应有一个可观测的帧心跳（dataset 计数器已证明有效）或至少 console.debug 门控输出。
  - **H3（低置信）**：挂载瞬间 `resize()` 在布局稳定前执行，`canvas.clientWidth` 可能为 0 → 首帧 1×1；若 ResizeObserver 未触发（尺寸确为 0 时确实不触发），drawing buffer 停留 1×1 且后续 resize 不清除旧帧。用户能看到静态纹理说明存在有效尺寸帧，此假设大概率不成立，但可作为排查清单项。
- **材料性后果**：特效**默认开启**（`loadSkinEffect` 默认 "wave"，lib/skin.ts:101）且只对 glass 皮肤生效路径可见——产品默认观感路径上的已确认缺陷；纯装饰性（无数据/安全影响），故 P2。
- **最小安全下一步**（按交接文档路径）：先做假设 A 的 2D canvas 重写（组件签名不变，改动局限 effects.tsx）；同步补 H2 的帧心跳观测面；用真实浏览器验证（IAB 截图无法验证动画，交接文档已证实该环境限制）；若 2D 也不动，再查 H3 与双 WebGL context 冲突（交接假设 C）。
- **实践参考**：[Chromium issue 503638（mix-blend-mode 元素不重绘的经典记录，will-change/translateZ 为已验证 workaround）](https://bugs.chromium.org/p/chromium/issues/detail?id=503638) 与 [WebKit commit：WebGL drawing buffer 内容未变化时不重复呈现（presentation 层缓存语义）](https://github.com/WebKit/WebKit/commit/3b3f025ed56606c9245cfd626b3d5d708b394a72)

**F6（P2）：SSE 解析三处复制，行为已开始分叉**
- **边界**：`api/client.ts` 内 `chat`（:728-793）、`subscribeEvents`（:810-853）、`subscribeTerminal`（:858-897）。
- **证据**：三处各 ~28 行结构相同的 fetch + `\n\n` 分隔 + `data:` 前缀宽容解析 + JSON.parse 容错循环；行为已分叉：401/非 2xx 处理在 chat 与 subscribeEvents 有（:754-758、:821-825），subscribeTerminal 只有静默 return（:869）；解析修正（如注释自称的"宽容解析"）需三处同步。上轮全库审查已点过同族（B QUAL-005"SSE 解析三处复制"），本审计确认前端三处现状。
- **材料性后果**：改动放大 ×3 + 错误语义漂移已经发生（terminal 流对 401 无反应）；下一次解析层修复（如按行解析替代 split 盲切）若只改一处即产生不一致行为。
- **最小安全下一步**：抽 `async function parseSSEStream(body: ReadableStream, onData: (line: string) => void)` 单 helper，三处消费；行为以现状最强者（chat 的 401 处理）为准统一。单文件重构，纯行为不变，可随时 revert。
- **实践参考**：[MDN Response.body（fetch 流式读取的现行标准接口，helper 即封装此机制）](https://developer.mozilla.org/en-US/docs/Web/API/Response/body)

### P3

| 优先级 | 问题 | 证据与理由 | 必需解决方向 |
|---|---|---|---|
| P3 | 死依赖 react-rnd / react-resizable-panels（+ 两个 tauri 插件 npm 包无前端引用） | 见 F7 | pnpm remove |
| P3 | viewMode 退役残留（setViewMode 无调用者）+ APPS.plugins/steward 无导航消费者 + AppEntry.defaultSize 无消费者 | 见 F8 | 删除死状态/死注册条目，收窄 APP_IDS |
| P3 | 死组件 ChatWindow.tsx / ExpertTeamDocs.tsx | 见 F9 | 删除两个文件 |
| P3 | 同会话两条 SSE 订阅 + 无重连语义 | 见 F11 | 单订阅上移 / 重连退避 |

**F7（P3）：死依赖 react-rnd 与 react-resizable-panels**
- **证据**：package.json 声明两者（^10.5.3 / ^4.12.2）；全 src 零 import（grep 实证）。两者均为退役形态的专用依赖：react-rnd 是桌面壳窗口拖拽拍板项（HANDOFF_DESKTOP_SHELL.md §一-4），react-resizable-panels 是三栏壳布局依赖（同文档 §三，键 `boenmind.layout.v2` 已不存在）；现布局由 dockview 承担（确有消费者：DockLayout.tsx/dock-views.tsx/dock-open.ts）。附带：`@tauri-apps/plugin-updater` / `@tauri-apps/plugin-process` 两个 npm 包在前端 src 零 import（Rust 侧 tauri-plugin-updater/process 是独立依赖，JS 侧从未调用其 API——更新链路全走后端 REST + 自定义 `backend_restart` 命令）。
- **材料性后果**：依赖面/安装体积冗余 + 每轮安全审计面误含；误导后来者以为仍在使用。无运行时风险。
- **最小安全下一步**：`pnpm remove react-rnd react-resizable-panels @tauri-apps/plugin-updater @tauri-apps/plugin-process`（后两个移除前先确认 src-tauri 无 JS 侧调用），一条命令可回滚。
- **实践参考**：[pnpm remove（官方移除命令）](https://pnpm.io/cli/remove)

**F8（P3）：退役残留——setViewMode 无调用者、APPS.plugins/steward 无导航消费者、AppEntry.defaultSize 无消费者**
- **证据**：`setViewMode` 仅在 store 定义（app-store.ts:295-298）与初始化读取，全组件无调用（AppearanceSettings:195-205"桌面形态"卡片点击只 toast 提示）；`APP_IDS` 含 plugins/steward（app-store.ts:66）且 `APPS` 有对应注册条目（app-registry.tsx:106-121），但 ClassicShell 导航只暴露 chat/coding/wiki + settings（NAV_APPS/PLACEHOLDER_APPS）——`activeNav` 初始化从 `boenmind.activeNav` 恢复（app-store.ts:299），旧 localStorage 残留可把用户带到 plugins/steward 视图（该视图内无返回入口之外的导航语义）；`AppEntry.defaultSize`（app-registry.tsx:64）与 `nameKey: "desktop.app.*"` 为桌面壳时代契约，现无消费方。
- **材料性后果**：死状态 + 死注册条目维持 4 处虚假契约面（viewMode 类型、defaultSize、plugins/steward 条目、`boenmind.viewMode` 键）；恢复路径可能把用户带到 UI 无法表达的视图（低概率、可手动导航回）。
- **最小安全下一步**：删 `viewMode`/`setViewMode`/`boenmind.viewMode`（AppearanceSettings 形态卡片保留为纯静态提示）；`APP_IDS` 收窄为 NAV 可达集合（或显式保留注释）；`AppEntry` 移除 `defaultSize` 与 `nameKey` 的 desktop 前缀约定。逐个单步、可回滚；与已拍板"等软件形态稳定再议"不冲突——删除的是死代码而非形态能力。
- **实践参考**：[仓库决策记录 docs/archive/HANDOFF_DESKTOP_SHELL.md（退役拍板的原始依据）](../archive/HANDOFF_DESKTOP_SHELL.md)

**F9（P3）：死组件 ChatWindow.tsx / ExpertTeamDocs.tsx**
- **证据**：全 src 无 import（grep 实证）；ChatWindow 为对话窗口旧形态（chat 重构为 DockLayout 面板后遗留），ExpertTeamDocs 为"专家团队"规划遗留；上轮交叉审查已点名（A-14 死代码三件含这两件，此处前端侧复核确认）。约 230 行。
- **材料性后果**：搜索噪音 + 维护面（i18n 键若随组件删除可再减 1 处来源）；无运行时影响。
- **最小安全下一步**：删除两文件 + 清理其独占 i18n 键（可用工作区已有 `check-i18n.cjs` 脚本核对残留键）。
- **实践参考**：[仓库交叉审查记录 docs/REVIEW_TOOLS_CROSS_2026-08-16.md（死代码项的先验确认）](../REVIEW_TOOLS_CROSS_2026-08-16.md)

**F11（P3）：同会话双 SSE 订阅且无重连语义**
- **证据**：FilePanel.tsx:90 与 TodoPanel.tsx:40 各自 `api.subscribeEvents(activeSessionId, ...)`（coding 默认布局同时含两面板，dock-views.tsx:108-139 → 同会话两条并发长连接 + 双份 after=0 全量重放）；client.ts:849 注释"网络中断由 keep-alive 重连语义交给上层"，但 subscribeEvents 返回的只有 `close()`，**无任何上层重连逻辑**——断线后投影静默停止，TodoPanel 的"live"绿灯是挂载态标志而非心跳（TodoPanel.tsx:47），显示 stale 数据且无提示。热升级流程以整页 reload 兜底（AboutSettings.waitForNewBackend），掩盖了断线场景。
- **材料性后果**：本地进程下断线罕见（故 P3）；但 web 部署（BOENMIND_TOKEN 模式已支持）下网络波动即静默陈旧；连接与重放 ×2 是持续的开销与行为面。
- **最小安全下一步**：订阅上移为 store 内单一 `subscribeEvents` 所有权（多面板共享一条连接，事件分发到投影），或先补最小重连退避（指数退避 + 心跳超时置灰 connected 状态）；后者单文件可改、可回滚。
- **实践参考**：[MDN EventSource（标准 SSE 客户端自带自动重连语义——对照 fetch 流不重连，支撑"补重连"的理由）](https://developer.mozilla.org/en-US/docs/Web/API/EventSource)

---

## Evolution order and residual risks

### 演进顺序（按先决条件与风险排序）
1. **F1（P1，立即）**：done 事件后重拉 `getSession` 使乐观 id 与服务器一致——单点改动，先于一切重构（当前唯一的功能级损坏）。
2. **F2（P2，顺手）**：reduce-motion 冻结特效时钟——3 行，与 F1 同轮。
3. **F3（P2）**：theme 启动校正 + `storageKey: "boenmind.theme"`——与 lang 校正对称，一文件。
4. **F5（P2，需真实浏览器）**：按交接路径重写 2D canvas + 补帧心跳观测面；此条依赖真实环境验证，独立排期。
5. **F6（P2）**：抽 parseSSE helper——纯重构，行为统一，宜在 4 前做（特效排查会再动 effects 之外的文件，先收拢解析层降低交叉面）。
6. **F4（P2，可后置）**：契约校验防线——工作量最大，与后端协商 schema 形态。
7. **F7/F8/F9/F11（P3 清理轮）**：一次清理定调（死依赖删除 + 退役残留收窄 + 死组件删除），随后再做订阅单源化。

### 残余风险与盲区
- **后端契约未审（范围外）**：F1 的 fork 端点行为、subscribeEvents 的 keep-alive/断连语义、event_log 事件 schema（todo/write、step/start 的字段名）均为后端权威，本报告只证明前端侧不匹配；建议后端审查轮核对 fork `at_message` 的定位语义与 SSE keep-alive。
- **动画冻结未实机验证**：本环境（无真实浏览器动画验证能力，IAB 截图已证无效）下 F5 全部候选原因保持假设等级；H2（无可观测帧心跳）是其中唯一可由代码直接修复的架构项。
- **未跑编译实证**：只读审计未执行 tsc/build/oxlint；静态结论均基于调用路径与 import 图（grep 全量核实），不依赖编译器输出。
- **上轮基线对照**：REVIEW_TOOLS_CROSS 中前端相关的"SSE 复制 / 死组件"两项在本报告保留为 P2/P3（本轮独立复核确认现状未变）；后端项（CSRF、权限门、双写）不在本报告范围。
- **已接受风险**：dock 布局快照按版本号重置用户布局（v8→v9 已发生一次，DockLayout.tsx:44-52 注释为已接受权衡）；5s health 轮询无退避（用户已拍板"别一直弹"）。
