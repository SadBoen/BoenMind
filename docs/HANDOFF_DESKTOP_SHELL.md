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

**四次迭代（用户看实物再反馈，同日）**：
- **窗口切换穿透修复**：AppWindow 外层测量容器（absolute inset-0）拦截鼠标，导致点击下层窗口露出的边缘无效——容器 `pointer-events-none` + Rnd `pointer-events-auto` 穿透（与 daedalOS RndWindow 的 pointerEvents 处理同机制）。实测：点最下层窗口边缘 → z 序置顶 + 菜单栏聚焦名同步。
- **底部白条融合**：壁纸原只铺桌面区（Dock/状态栏区域露 body 白底）→ 壁纸移到 Desktop 根容器全屏覆盖；Dock/状态栏改深色玻璃（bg-black/25 白字）与星空壁纸融合（M3 复核无白条）。
- **插件/管家窗口内容边距**：ScrollPage 加 p-6 + max-w-3xl 居中（与 SettingsPage 视觉一致；DOM 实测 25px 留白）。
- **Dock 磁吸（参考项目机制移植）**：参照评估表教科书级项目 PuruVJ/macos-web 的 DockItem（鼠标横向距离分段插值放大：最近 2x/相邻 1.414x/1.1x + spring 平滑）→ React 版：Dock 容器 onMouseMove 记录鼠标 X，各图标按"中心到鼠标距离"线性衰减放大（半径 88px，最大 1.4x），宽度参与 flex 布局 → 中心放大时两侧图标平滑推开（经典 macOS 磁吸）。注：dawidolko Sonoma 已删库（gh 搜不到，8⭐ playground 本就只作选型参照）；窗口机制与 daedalOS（useFocusable z 序栈 + RndWindow）对比确认同构。磁吸动效 IAB 无法自动化验证（无 hover/mousemove 合成），需真实浏览器确认。

**五次迭代（用户批准吸收 macos-web SystemUI 层，同日）**：
- **#system-ui 弹窗宿主层**（借鉴 macos-web SystemUI/SystemDialog 层级纪律）：Desktop 渲染 `#system-ui`（pointer-events-none fixed inset-0 z-[60] 最高层），ui/dialog.tsx 的 DialogPortal 统一挂载（`container = #system-ui ?? body`），overlay/popup 显式 pointer-events-auto 恢复接收指针。效果：PermissionDialog/PluginSettingsDialog/ProviderFormDialog/TokenGate 全部自动归位最高层，不再散落 body 与窗口/Dock 层级打架。DOM 实测 dialog-content 父链 = #system-ui → Desktop 根；视觉 M3 复核弹窗居中遮罩压暗层级正常。焦点陷阱 base-ui modal Dialog 内建（Tab 循环在弹窗内），无需自写（macos-web trap-focus 机制已内建等价物）。

实测结果（vite dev + 真后端 17321）：
- ✅ 启动画面（2s 自动进桌面 + 点击跳过）→ 空桌面 → 开始菜单 5 应用卡 + 脚注（版本/模型/工作目录，StatusBar 信息迁入）
- ✅ 聊天全链路：新建会话 → 发消息 → 流式回复（含思考过程折叠）
- ✅ 设置/插件/管家/编程应用打开；设置应用内菜单导航正常（外观/管家页切换实测）
- ✅ 任务栏：运行应用按钮 + 点击聚焦置顶（z 序按打开顺序）；红点关闭窗口
- ✅ 明暗主题切换（.dark class 实测）、英文切换实测（ja/ko 键同构由类型系统保证）
- ✅ 二次迭代布局实测：MenuBar 顶部（开始按钮 y=4）/Dock 底部居中（x=528=1280/2 正中）/开始菜单顶部下拉（y=57）/窗口钳制在 MenuBar 与 Dock 之间（y=44-648 不重叠）/壁纸明暗各 15 层
- ✅ 拖拽由用户真实鼠标实测确认可用（react-rnd 标题栏拖拽正常；IAB 合成事件无法自动化验证属环境限制，见记忆 iab-browser-testing-limitations）

顺手修的真 bug：窗口默认尺寸超过窗口层高度（聊天 700px > 层 672px）会溢出到任务栏——AppWindow 挂载时按容器钳制尺寸（min(default, 容器-16)）。

**六次迭代（自主轮，同日）——窗口控制接线（黄绿圆点 + 尺寸调整）**：
- **黄点最小化**（macOS 语义）：窗口收进 Dock——渲染跳过但保留 openApps z 序；Dock 图标变暗（opacity-50 saturate-50）+ 空心指示点（border 空心）；点击 Dock 恢复 + 置顶聚焦；聚焦转移给 z 序上一个可见窗口（无则空桌面）；开始菜单再开已最小化应用 = 恢复（openApp 兼容分支）。
- **绿点最大化/还原**：铺满桌面容器（-16px 边距，实测 1264×576 @ 8,8）；还原回最大化前位置尺寸（prevState ref）；**双击标题栏 = 最大化切换**（替代原双击关闭——对齐 macOS zoom 语义）；最大化时禁拖禁 resize + ResizeObserver 跟随容器尺寸。
- **react-rnd 尺寸调整**：enableResizing 开启（minWidth 320/minHeight 240）+ 右下角斜线角标；onResizeStop 落回受控 size。注意 react-rnd v10 底层是 re-resizable，其 Resizer 手柄 div 默认无 className（查 DOM 找不到手柄属正常，拖拽有效）。
- i18n ×4：windowMinimize/windowMaximize。
- 浏览器实测全链路过：开始菜单开聊天 → 黄点最小化（窗口消失）→ Dock 图标 opacity-50 saturate-50 + 空心指示点 → Dock 点击恢复（聚焦 ring）→ 绿点最大化（1040→1264×576@8,8）→ 绿点还原（回 1040×576@(120,8)）→ 右下角拖拽 resize（1040→1155）。
- 坑：**Dock 磁吸按钮在 IAB 合成鼠标下点不中**——鼠标移动到按钮中心即触发 onMouseMove 磁吸放大、按钮漂移，mousedown 落点偏出 → click 无效（MenuBar/StartMenu/窗口内按钮无此问题）。真实用户鼠标无碍（用户看着放大后的按钮点击）。IAB 自动化恢复窗口的 workaround：先 cua.move 移开鼠标（清磁吸）再 dom_cua 点击。此限制已记入验证结论。

**七次迭代（用户拍板，同日）——双 DE 并存：经典软件界面回归（默认）+ 桌面模式**：
- 用户拍板四点：① 默认软件形式界面（经典三栏壳回归）② 界面层插件化要做，学 DeepSeek Harness ③ 插件分类不拆目录，用标签（category）+ UI 分 tab ④ 编程应用仍是软件实现第一优先（导航图标占位点不了）
- **ClassicShell（新组件，components/classic/）**：原三栏壳形态回归——左侧 48px 导航条（应用图标 = APPS 注册表驱动，聊天/设置/插件/管家 + 底部「桌面模式」入口）+ 主面板（渲染 APPS[activeNav].component，内容组件零改动）+ 底部状态栏（StatusBar 加 variant="classic" 浅色变体）。**编程图标 disabled 置灰**（用户拍板"点不了"，M2 接线）
- **默认界面 = 经典**（viewMode: "classic" | "desktop"，localStorage boenmind.viewMode 持久化）；桌面模式从导航条底部入口进；桌面 MenuBar 开始按钮旁加「切换经典软件界面」（PanelLeft 图标）。两壳共享同一 store（activeNav 与 openApps 并存），后端零改动——**架构 §四·B「前端壳多套并存」实作验证**
- **插件分类标签**：PluginInfo 加 category（manifest category 声明，单文件/未声明默认 system）；现有插件打标 ctx-compactor/web-search/refine-suggest=system、pdf-omni=app；插件页加 tab（全部/系统增强/功能插件，计数+过滤）；旧后端无字段时缺省 system 兼容。不拆目录（用户拍板）
- **DeepSeek Harness 调研结论**（用户"deepseek 已经做到了，可以学习"实锤）：dsh（deepseek-ai/deepseek-harness，2026-08-13 发布 MIT）前端界面插件化 = **ui-slots 槽位组合（Slot & Injection Kernel：ctx.slots/renderSlot/inject）+ 同域 bundle 动态注册（/plugins/<id>/client.js CJS factory → window.__ModuleLoader__.load）+ 客户端插件不得自带 React（从壳 platform module table 取依赖）+ CSS 内联 + RPC 桥**——不是 iframe/WebComponent；为 §四·C 待拍板的"iframe/WebComponent/动态加载"提供了第三种答案（同域 bundle：无隔离但依赖共享简单）
- 浏览器实测：默认进经典（导航条+聊天全界面+状态栏）→ 插件页 tab 过滤 → 桌面模式 → 切回经典 → 刷新持久化（两种模式都验）
- **IAB 新坑：tab.reload() 后合成输入通道失效**（dom_cua/cua/playwright 点击全部无效，新标签页打开正常）——验证流程避开 reload，用新标签页

**八次迭代（用户复查导航语义，同日）——导航条修正 + 外观形态设置**：
- 用户澄清：左侧导航条是**软件导航**（如 wiki、编程），不是给设置项导航的；设置入口放导航条**最底部**（原 NavBar bottom 分区语义），插件/管家/模型提供商等收进设置应用二级菜单（原 SettingsMenu 已有）
- ClassicShell 导航改为：顶部 NAV_APPS = [chat, coding(占位 disabled)]；底部独立区 = 设置（齿轮）+ 桌面模式。主面板仍渲染 APPS[activeNav]
- **外观设置页改造**（用户：形态切换开关放设置→外观里；各形态显示各自专属设置）：顶部界面形态卡片（软件形态/桌面形态，点击即切换 viewMode）；**软件形态区 = 字体大小**（小/标准/大 → html 根字号 14/16/18px 全局缩放，localStorage boenmind.fontScale）；**桌面形态区 = 桌面模板壁纸**（星空/极光渐变，.desktop-wallpaper-aurora CSS 变体 + 外观卡预览缩略图，localStorage boenmind.wallpaper）；主题/语言为通用区（两形态共用）
- 壁纸状态入 zustand store（响应式即时生效）；工具函数独立 lib/appearance.ts（Fast refresh 纪律）
- 坑：壁纸若只写 localStorage 不重渲染（Desktop 不订阅）——必须走 store
- 实测：导航条 4 项（对话/编程disabled/设置/桌面模式）→ 设置二级菜单完整 → 外观页形态切换 → 桌面形态显示壁纸（字体隐藏）→ 极光切换即时生效 → 切回软件形态显示字体大小 → 字体"大"→ html 18px

遗留（不在本期范围）：界面层插件化落地（dsh 机制吸收，§四·C 拍板）、多实例（单例为拍板项）、编程应用真实壳（M2）、WIKI 插件（未立项）。

## 八、关联文档

- 批准方案全文：本文件 §五（原 ExitPlanMode 方案 v2，用户已批准）
- 调研笔记：docs/research/2026-08-15/desktop-shell-landscape.md（16 项目 + macOS 系对比 + 风格结论）
- 架构契约：docs/everything-is-plugin-architecture.md §四·B（行 352-365）、§四·C、§6.3/6.4
- 主交接：docs/HANDOFF_KERNEL_PHASE1.md（项目全景）
- 记忆：desktop-shell-direction（方向与拍板）、coding-app-m1-decision（M1 背景）
