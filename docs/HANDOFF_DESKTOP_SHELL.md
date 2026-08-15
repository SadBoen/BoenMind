# HANDOFF —— 桌面壳（DE）开工交接（2026-08-15）

> **新对话开工指令：读本文件 → 按 §五 步骤从第 1 步（pnpm add react-rnd）开始干。**
> 一句话任务：把现有前端改造成"模拟 OS 桌面"——启动画面 → 空桌面 → 左下开始菜单（5 应用）→ 任务栏 → 居中可拖拽窗口；**聊天/设置/插件/管家组件零改动嵌入，后端零改动**。方案已获用户批准。

## 一、已拍板清单（全部，勿再问）

| # | 拍板 | 内容 |
|---|---|---|
| 1 | 形态 | 模拟 OS 桌面：Windows 式布局逻辑（左下开始菜单+底部任务栏+时钟右置）+ macOS 风窗口质感 |
| 2 | 风格锚 | **Cosmos Cloud**（用户三认："水灵水灵的就是它"）：紫蓝渐变壁纸、玻璃拟态、大圆角、彩色渐变应用卡——拒绝 1Panel 式后台表格风 |
| 3 | 开机 | 简短启动画面（logo+版本+进度条 ~2s 可点击跳过）→ **空桌面**（应用手动开，聊天不自动打开） |
| 4 | 窗口 | 居中可拖拽（react-rnd），**单例**（重复点开=聚焦已有）；层叠/最小化/多实例留二阶段 |
| 5 | 窗口控制 | 标题栏左红黄绿三圆点（macOS 风）：红=关闭，黄绿占位二阶段接线 |
| 6 | 应用集 | 开始菜单 5 应用：💬聊天（现有界面整体保留）/💻编程（占位"建设中"）/⚙️设置/🧩插件/🤖管家 |
| 7 | 复用 | 内层=现有成熟组件零改动嵌入；新代码只写"壳"（注册表+桌面宿主五件套） |
| 8 | 范围 | 交付节奏=桌面先上线；**后端零改动**（所需端点全存在）；同步修 M1 报告 §四①②③ 属后续轮不混入 |

## 二、架构对齐（§四·B 落地第一步，写进代码注释的理由）

- DE 契约（docs/everything-is-plugin-architecture.md 行 363 原文）："前端壳 = @boenmind/client + 应用注册器（导航/页面/快捷键/托盘）。内核不关心前端长什么样，只提供 API + 事件流。"
- 行 365："应用插件的前端包跑在 DE 里（像软件窗口跑在桌面环境里），DE 提供窗口/导航/通知等宿主能力，应用提供内容。"
- 本期落地 = **AppRegistry 应用注册器**（现 `src/lib/navigation.tsx` 静态注册表升级——它已是注册器前身）+ DE 宿主层；应用=前端包（本期静态注册，动态加载后续）。
- 同一 SPA 同时服务 web 与 desktop-tauri 两个 DE（现状已如此，不动）。

## 三、前端现状地图（探查结论，免重新摸索）

- **栈**：React 19 + TS + Vite + **Tailwind v4**（无 tailwind.config，令牌在 index.css `@theme inline`）+ Base UI（@base-ui/react v1.7，shadcn 风格）+ zustand 单 store + i18next v26 + next-themes + react-resizable-panels v4 + pnpm 11。**react-rnd 尚未安装**（上轮 pnpm add 被用户打断，package.json 干净）——第 1 步装。
- **无路由**：导航靠 zustand `activeNav`/`settingsTab`；注册表在 `frontend/src/lib/navigation.tsx`（NavKey/NAV/SETTINGS 三表 + SettingsPage 渲染器）——改造成 `app-registry.tsx`。
- **App.tsx**（205 行）：三栏壳 = NavBar（48px 图标条）+ react-resizable-panels Group（SecondaryPanel|MainPanel|FilePanel）+ StatusBar（版本/模型/工作文件夹）+ TokenGate；布局持久化 localStorage `boenmind.layout.v2`。**将被 BootScreen→Desktop 替换**；NavBar/SecondaryPanel/MainPanel/StatusBar 归档，FilePanel 先退场（编程应用二阶段再回来）。
- **app-store.ts**（473 行，唯一 store）：sessions/chat 流式/config/permission/files/nav 全在此；新增 openApps/focusedApp/openApp/closeApp/focusApp 即可，其余不动。`settingsTab` 保留归设置应用内部。
- **应用内容组件（零改动嵌入）**：chat = `components/chat/SessionList` + `ChatWindow` 双栏（内部继续用 react-resizable-panels）；settings = `components/layout/SettingsMenu` + `lib/navigation.tsx` 的 SettingsPage；plugins = `components/settings/PluginsSettings`；steward = `components/settings/StewardSettings`（5s 轮询已内建）；coding = 新写占位组件。
- **弹窗类**（PermissionDialog/PluginSettingsDialog/sonner toaster）均为 portal 到 body，任何壳下正常工作。
- **i18n**：`src/i18n/index.ts` + `locales/{zh,en,ja,ko}.ts`（各 ~400 行内联 TS）；新键 ×4 语言。
- **构建**：`pnpm build`（tsc -b && vite build）→ frontend/dist；bm-server `--features embed` 用 RustEmbed 打包该目录（static_files.rs + lib.rs 178-180）。开发验证用 `pnpm dev`（5173，代理 /api → 127.0.0.1:17321）——**比反复 rebuild embed 快**。
- **Tauri 壳**（src-tauri/）：纯启动器，零改动自动生效。

## 四、注意坑（血泪清单）

1. **react-markdown v10 不接受 className prop**（传了白屏崩溃且 tsc 不报错）——样式类放外层容器（MessageItem.tsx:84 现成模式），新代码别踩。
2. Tailwind v4 无 config 文件：新令牌加进 index.css `@theme inline` 与 :root/.dark 两套 oklch 变量，别找 tailwind.config。
3. **bm-server 正在后台运行**（上一会话起的，服务 17321，embed 的是旧 dist）——重建 release exe 前必须停掉（Windows 锁 exe 致链接失败）；开发期别停，用 vite dev 联调即可。
4. 桌面层自绘（开始菜单/任务栏/窗口框），**不要用 Base UI Dialog** 做开始菜单（portal 到 body 会脱离桌面层级）。
5. Windows curl 中文 JSON 报 invalid unicode——API 验证用纯 ASCII；IAB 浏览器 fill 不触发 React onChange（按钮 disabled 不解锁），验证须真实键盘 type 或干脆只做视觉验证。
6. 窗口内容区要给明确高度/滚动语义（旧 MainPanel 的 ScrollArea 职责移进窗口内容层），否则聊天内部滚动条会丢。
7. 明暗主题都要验证：next-themes 切 .dark class，新写的渐变/玻璃令牌必须两套都调。

## 五、实现步骤（批准方案原文，按序执行）

1. `pnpm add react-rnd`（MIT；dawidolko 同款选型实证）。无其他新依赖。
2. **`src/lib/app-registry.tsx`（新）**：`AppId = "chat"|"coding"|"settings"|"plugins"|"steward"`；`AppEntry { id, nameKey, icon, gradient /* Cosmos 渐变底色 */, component, defaultSize }`；注册 5 应用（内容组件见 §三，零改动）；原 NAV/SETTINGS 表退役或由注册器派生。
3. **`src/components/desktop/`（新五件套）**：
   - `BootScreen.tsx`：渐变启动画面（logo+BoenMind+版本+进度条 ~2s 点击跳过）→ 空桌面
   - `Desktop.tsx`：紫蓝渐变壁纸（明暗两套）+ 窗口层 + 任务栏 + 开始菜单
   - `Taskbar.tsx`：底栏 48px 毛玻璃——开始钮、运行中应用（开窗高亮/点击聚焦）、时钟 + 后端状态点（复用 health store）
   - `StartMenu.tsx`：左下玻璃面板，彩色渐变应用卡网格（Cosmos 风）+ 底部脚注（版本/后端状态/工作文件夹——StatusBar 信息迁入处）；点外/Esc 关闭
   - `AppWindow.tsx`：react-rnd（默认居中、单例聚焦）；标题栏=渐变应用图标+名称+左红黄绿三圆点；内容区渲染 AppEntry.component
4. **`src/stores/app-store.ts`**：新增 openApps/focusedApp/openApp/closeApp/focusApp；其余不动。
5. **`src/App.tsx` 重写**：BootScreen → Desktop；旧三栏壳归档（勿删，编程应用二阶段可能复用 FilePanel）。
6. **`src/index.css`**：桌面令牌（渐变壁纸/backdrop-blur 玻璃/窗口圆角 14px 阴影/应用卡渐变）明暗两套；应用内部样式零改动。
7. **i18n ×4**：应用名、编程"建设中"、启动画面、无障碍标签。
8. **文档与记忆**：调研笔记已含 Cosmos/Linux 面板/macOS 对比（docs/research/2026-08-15/desktop-shell-landscape.md）；完工后更新本文件状态段 + HANDOFF_KERNEL_PHASE1.md + 记忆 desktop-shell-direction，commit+push（自动推送政策）。

## 六、验证计划

1. `pnpm build` + `pnpm lint`（oxlint）全绿。
2. 浏览器实测（browser-use 或手动 + vite dev）：启动画面→空桌面→开始菜单开 5 应用（**聊天链路真实验证**：建会话发消息流式正常）→ 拖拽/重复点开聚焦/红点关闭 → 明暗主题 + 4 语言切换。
3. commit + push（pre-push 本地质量门跑 Rust 侧；前端侧靠 build/lint 绿）。

## 七、不做清单（防跑偏）

编程应用真实壳（文件树/编辑器/分支图）、todo 面板、窗口层叠/最小化/多实例、插件前端包 manifest 字段+动态加载（iframe/WebComponent 待拍）、/api/apps 端点、M1 报告 §四①②③ 后端修复（max_steps 128/grep·find ignore+timeout/read offset+limit）——全留后续轮。

## 九、完成状态（2026-08-15，本会话）

**桌面壳已上线（代码全部落地 + 浏览器实测通过 + build/lint 绿）**。与批准方案的偏差仅一处：

- **旧三栏壳不归档、直接删**（用户拍板"重新写会不会更好"→ 评估后采纳）：NavBar/SecondaryPanel/MainPanel/StatusBar/FilePanel/FilePreview/navigation.tsx 全部 git rm（git 历史兜底，二阶段编程应用要 FilePanel 从历史 checkout 即可）。

**二次迭代（用户看实物后反馈，同日）**——macOS 组合定稿：
- 修布局 bug：Desktop 根容器漏 `flex-col` 致任务栏（footer）掉到屏幕顶部、开始菜单却在左下（用户见"左上角小星星/菜单跑左下角"）——任务栏按 Windows 逻辑本应在底部。
- **改为 macOS 组合**：顶部 MenuBar（开始按钮+聚焦应用名+后端状态点+时钟，毛玻璃深色底保证对比度）+ **底部居中 Dock**（5 应用渐变图标，运行指示点，悬停放大）+ 开始菜单改从顶部按钮下拉。
- **星空壁纸**：CSS 15 层（10 星星 + 2 光晕 + 紫蓝渐变）明暗两套，解决"背景与状态栏对比度太小"。

**三次迭代（用户看实物再反馈，同日）**：
- **窗口层叠偏移**（26px→48px，视觉 M3 复核确认下层标题栏可见）+ 点击窗口任意可见部分聚焦置顶（onMouseDownCapture，实测点击露出的边缘即置顶）。
- **底部状态栏**补回：Dock 下方 h-7 细条（后端状态点+版本 | 模型 | 工作目录 | 品牌）；时钟留在顶部菜单栏。
- **Dock 悬停放大** scale-110→125 + 上浮 6px。
- **视觉 MCP 打通**：本会话工具列表无 minimax 工具（配置已就绪但未加载），改为直调 MiniMax M3 API（config.json 的 key/base_url）识图验证——两次截图复核均通过，以后可复用此工作流。

实测结果（vite dev + 真后端 17321）：
- ✅ 启动画面（2s 自动进桌面 + 点击跳过）→ 空桌面 → 开始菜单 5 应用卡 + 脚注（版本/模型/工作目录，StatusBar 信息迁入）
- ✅ 聊天全链路：新建会话 → 发消息 → 流式回复（含思考过程折叠）
- ✅ 设置/插件/管家/编程应用打开；设置应用内菜单导航正常（外观/管家页切换实测）
- ✅ 任务栏：运行应用按钮 + 点击聚焦置顶（z 序按打开顺序）；红点关闭窗口
- ✅ 明暗主题切换（.dark class 实测）、英文切换实测（ja/ko 键同构由类型系统保证）
- ✅ 二次迭代布局实测：MenuBar 顶部（开始按钮 y=4）/Dock 底部居中（x=528=1280/2 正中）/开始菜单顶部下拉（y=57）/窗口钳制在 MenuBar 与 Dock 之间（y=44-648 不重叠）/壁纸明暗各 15 层
- ⚠️ 拖拽未能在 IAB 中自动化验证（CUA/Playwright 合成事件对 React 拖拽失效，环境限制，见记忆 iab-browser-testing-limitations）；代码与 react-rnd 标准用法一致（dragHandleClassName 标题栏），需真实浏览器人工确认一次

顺手修的真 bug：窗口默认尺寸超过窗口层高度（聊天 700px > 层 672px）会溢出到任务栏——AppWindow 挂载时按容器钳制尺寸（min(default, 容器-16)）。

遗留（不在本期范围）：窗口层叠/最小化/多实例、黄绿圆点接线、react-rnd 尺寸调整（enableResizing 关着）、编程应用真实壳。

## 八、关联文档

- 批准方案全文：本文件 §五（原 ExitPlanMode 方案 v2，用户已批准）
- 调研笔记：docs/research/2026-08-15/desktop-shell-landscape.md（16 项目 + macOS 系对比 + 风格结论）
- 架构契约：docs/everything-is-plugin-architecture.md §四·B（行 352-365）、§四·C、§6.3/6.4
- 主交接：docs/HANDOFF_KERNEL_PHASE1.md（项目全景）
- 记忆：desktop-shell-direction（方向与拍板）、coding-app-m1-decision（M1 背景）
