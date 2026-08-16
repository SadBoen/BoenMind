# HANDOFF —— 迭代指针（原阶段 1 交接）

> 权威顺序：架构宪法 `docs/everything-is-plugin-architecture.md`（v0.26）> `docs/EXTENSION_POINTS_REGISTRY.md` > 本文件 > `docs/design/*`。
> 历史全量 → docs/archive/HANDOFF_KERNEL_PHASE1_ARCHIVE.md。下文「轮次脉络」是日记，**开工只读本节当前状态 + 下一步 + 待拍板**。
> 开工前先 `git pull`。

## 当前状态（2026-08-17 校准）

**不要按旧句开工**：服务面不是 13；LoopHooks 不是 10；桌面壳不是双 DE 完成态；M2 下一件不是分支图。

| 项 | 当前态 |
|---|---|
| 里程碑 | 阶段 1 完成。M1 ✅。M2 活 todo ✅（分支图/项目切换/终端已做）。剩余：**skill 系统提示按场景注入**（对话宿主化③，不是 M2 原验收句） |
| 服务面 | **以登记表为准**：协议 14 + 运行期 mcp。`credentials`/`scheduler` 已消费；tools/notify 已注册、插件 lookup 待接线 |
| 配置 | 一把 `Arc<RwLock<AppConfig>>`（HTTP/Port/门闩）。禁止再 clone |
| 设置中心 | 阶段 1–5 已落地（桌面代码退役、scopes、`[apps]`、专家同池、分级）。文：`docs/design/SETTINGS_ARCHITECTURE_2026-08-16.md` |
| MCP | `bm-mcp` 出厂；4c OAuth 不做。组合根始终持有空管理器（C P1 已修）；connect/disconnect 后 invalidate 会话 agent |
| DE | 经典三栏默认；桌面壳**代码已删**，开关 toast。皮肤/特效见架构 §四·B 补充 3 |
| 聊天 | 插入排队 = DE 输入策略（≠ `enqueue_turn`）；TokenRing = 128K 参考窗 |
| 冻结 | 双写对账至 M3；挂点不删；不换 dsh |

**LLM 厂商插件化（2026-08-16，已落地）**：`ProviderKind` 3 家 + `ProviderPort` + `stable_id`；交接 docs/archive/HANDOFF_LLM_PROVIDER_PLUGIN.md。

**内核接线**：KernelBuilder 预装面；消费态见登记表。http 面 YAGNI。图纸 archive/SERVICE_FACES_2026-08-15.md **止于当时 13 面**。

**编程应用 M1**（8254bd7）：报告 docs/archive/ACCEPTANCE_M1_2026-08-15.md。问题 ①–⑤ 已修。

**界面插件化**：已拍学 dsh 槽位；落地设计文未出（§四·C）。脉络见 docs/archive/HANDOFF_DESKTOP_SHELL.md（历史，非当前 DE）。

以下段落保留 08-16 日记（布局/对话宿主化等），**与上表冲突以上表为准**。

**阶段 1 完成态**：主线 A（执行级事件日志 + 自研 bm-loop 引擎）+ 主线 B（pi-compat 插件兼容层）全部落地；默认引擎已反转 bm；Steward 三件套（调度器/inject 通道/前端状态页）真实验收通过；legacy 删空（§十三终点）。BoenMind = 可聊天/调工具/派子代理/有管家/有记忆的完整运行时。

**LLM 厂商插件化轮（2026-08-16，方案 A 拍板并落地）**：`ProviderKind` 24→3（minimax/deepseek/custom；`#[serde(other)] Unknown` 兼容旧配置）+ **`ProviderPort` 服务面（第 14 面）**——LlmPort 经它取官方端点/协议形状（不再直读硬编码表）+ `stable_id` 取代 pi_name（bm-core `ProviderConfig::descriptor`）+ `ProviderShape`（openai-compatible/anthropic/gemini；custom 表单可选方言，Anthropic/Gemini 厂商经 custom 接入）+ thinking 白名单按形状收敛（SSRF 校验回环放行，ollama 类本地服务经 custom）+ 前端预设 3 家/custom 形状下拉/四语言同步。验收：61+129 测试全绿、旧 config 含 groq/mistral 实测启动不炸（kind=unknown）、custom 端到端实测（真实 key 回复成功）、pre-push 放行。提交 93d542f/6eed10a/165adcb；交接文档 docs/archive/HANDOFF_LLM_PROVIDER_PLUGIN.md。**顺带**：50 个测试遗留会话全部删除（备份 ~/.boenmind/backup-session-clean-20260816/）。

**内核接线完成（2026-08-16 凌晨，服务面铺开轮，当时态）**：当时登记 13 面（漏 provider）+ LoopHooks 写 10（实为 12）。**当前以文首表 + 登记表为准。** http 面 YAGNI。图纸 archive/SERVICE_FACES_2026-08-15.md 止于当时 13 面。

**编程应用 M1 已验收通过（8254bd7，2026-08-15）**：BoenMind 用自身运行时（bm-loop + MiniMax-M3 + 内置工具，纯 API 驱动）在自身仓库完成真实 bug 修复全链路（prompt_hash 注入面缺口：定位→修复→回归测试→git 提交），独立复核 33 测试全绿 + clippy 零告警。**验收暴露 6 个真实问题**（报告 docs/archive/ACCEPTANCE_M1_2026-08-15.md §四）——**①-⑤ 已全部修复**（1f064db：max_steps 64→128 + grep/find 接入 ignore crate 带 timeout + read/工具提示段；3d3bf2b：ctx-compactor 索引落 $BOENMIND_HOME + bm-compat dev-deps 补齐；⑥ 压缩水线持续收敛）。

**M2 编程应用独立壳已起步（0fb2acb，本日）**：独立壳 = 文件树/编辑器/分支图 + 活任务清单（用户痛点"任务清单生成后不会实时插入/删除"闭环：模型调 todo 工具 → todo/write 全量快照落事件日志 → 前端事件流投影实时刷新）。后端：todo 工具（bm-server todo_tool.rs）+ GET/POST /api/sessions/{id}/todos（共用 apply_todo_op）+ POST /api/workspace/file + GET /api/workspace/git-info；bm-loop 删 step_queue（A6 修订）+ 步数预算提示（剩 6 步注入收敛指令）；is_text 补 application/json。前端：CodingApp 三栏（FilePanel 从 2cb65fa^ 恢复 + Editor 可编辑保存 + TodoPanel 事件流投影）+ GitBar（分支/提交时间线）+ subscribeEvents（fetch 流式带 Bearer）；ClassicShell 编程导航接线；桌面壳经注册表同步生效。**浏览器实测全链路通过**（模型真实建 3 任务 high 优先级实时投影 + 回合步数显示；编辑器保存/还原；桌面壳同链路）。**M2 下一件 = 分支图深化（DAG）+ 编辑器增强（diff/未保存守卫）+ 长会话性能（EventQuery 类型过滤）**。

**前端壳（DE）完成态（2026-08-15 八次迭代收官 + 当日导航收口）**：**双 DE 并存**——经典软件界面（默认，左侧导航=软件导航 chat/coding/wiki 占位置灰、底部=仅设置入口——a8397f8 移除桌面模式按钮，外观页形态切换接管；外观页形态切换+形态专属设置：软件=字体大小/桌面=壁纸模板）+ 桌面壳（OS 形态入口，窗口控制接线全落地：最小化/最大化还原/resize）；插件分类标签（manifest category + 插件页 tab）；**界面层插件化已拍板要做，学 DeepSeek Harness 思路**（机制调研完成：ui-slots 槽位 + 同域 bundle 动态注册——§四·C 落地设计待拍板，实现按本架构落不抄代码）。全脉络见 docs/archive/HANDOFF_DESKTOP_SHELL.md §九/§十（历史）。

**架构讨论轮（本日，用户开题两场）**：
- **内核主权评估（三轮讨论定调）**：用户开题"换上 dsh 内核的 Rust 版会不会更好？以后它的生态是我们羡慕的"→ 定调**不换不跟随 dsh 内核**（Rust 移植=双倍维护/权威分界原则/挂载点三类：故意不放 loop+权限审批、架构简化掉 provider、该学前端槽位+事件域扩展机制/演进路径：策略插件化→存储 port→协议 version+迁移/生态=平台协议非内核，兼容层+商店+贡献面）——已入架构 §15.4
- **对话宿主化 + 场景作用域拍板**（用户"每个带 LLM 的软件都有对话框，不该每软件重写"）：对话界面=宿主能力（ChatPane 共享组件/形态变体/编程壳右栏 Tab）；会话 `app` 场景字段（一软件一会话）；工具面按 `session.app` 组装（内置手脚+系统增强全局、场景工具按场景）；skill 场景注入随 M2 深化；manifest `scopes`+槽位随 §四·C——已入架构 §四·B 补充 + 待拍板 9
- 架构文档 v0.22（状态行同步）

**对话宿主化 + 场景作用域已落地（拍板 9 执行，2026-08-15）**：
- 后端：`sessions` 表 + `Session` struct 加 `app` 字段（默认 chat；旧库启动自动 ALTER 迁移，旧会话归 chat）；创建 API 接受 `app`；引擎 `build_loop_agent` 透传 `session.app` 并留场景工具登记点（当前无场景工具；todo/subagent 属内置手脚全局）——`chat_bm`/`run_steward_turn` 两入口贯通
- 前端：ChatWindow 主体抽为宿主共享 `ChatPane`（形态变体 full/panel；panel = 紧凑标题栏 + ChatInput compact 隐藏提示/占位按钮）；编程壳右栏 Tab（任务/对话），对话 Tab 懒创建 coding 会话（ensureAppSession）；`activateApp`（场景最近会话 localStorage 持久化，无则清聚焦——聚焦会话永远属于当前应用）；聊天会话列表按 app 过滤；ClassicShell 切应用自动恢复各场景会话
- 实测（隔离 home）：旧库迁移/创建 app 会话/引擎工具面构建/聊天列表无 coding 会话/编程壳对话 Tab 显示 coding 会话消息/切回聊天恢复 chat 会话全通过；用户 36 个真实会话升级无损（app=chat）
- 已知限制：桌面壳多窗口共存时编程对话与聊天窗口共享聚焦会话（多实例拍板项，CodingApp 已注明）

**编程壳功能规划轮（2026-08-15 用户开题 + 定调两件）**：
- **吸收原则升级（用户定调）**："能插件化的、网上已有的 skill/MCP/库就用网上的——找到最优解转成自己的官方插件，做好上游跟踪台账"→ 台账 `backend/vendor/UPSTREAM_TRACKING.md`（区别于补丁台账 UPSTREAM_PATCHES.md；T1 终端 xterm.js / T2 portable-pty 跟踪 xpty async fork / T3 code-graph 0.116 转官方插件 / T4 浏览器 B 方案）
- **TerminalPane 一期已落地（07618ce，上游吸收 T1/T2）**：后端 /api/terminal（portable-pty 会话，创建/输入/resize/SSE/关闭；broadcast 多订阅 + 读线程自清理；**ConPTY ESC[6n 光标查询应答**——不答则 cmd 输出阻塞，实测坑）；前端 TerminalPane 宿主组件（xterm 6 + fit；编程壳右栏第三个 Tab，终端 Tab 加宽 30rem）；dir 命令全链路实测通过
- **剩余**（执行顺序已拍）：项目切换（currentProject 上下文 + 文件树/终端联动）→ codegraph 转官方插件（scopes=["coding"]）→ 浏览器仿真 B 方案（可视化 web 工具链）

**聊天单元微调轮（2026-08-15 用户四点）**：
- 三横按钮加宽 h-6w-6 → h-7w-9（与 token/X 协调）；**独立 session 列表确认删除**（v6 默认布局已无，旧标签页刷新即见）
- **窄面板自适应**：ChatPane ResizeObserver 检测单元宽度——<560px（如编程壳右列 320px）时会话列表 = 内部悬浮窗（absolute overlay 不挤占聊天宽度）；≥560px（聊天应用）内嵌侧栏；两种形态实测通过
- **多 tab 叠放时三横位置**：dockview prefix 槽位 = pre-actions 容器在 tabs 容器之前（组级最左）——原生已满足"三横在最左边"，无需改动
- 坑记录：**dev server 长跑 + 多轮 HMR 会模块混乱**（面板渲染空白，疑似 HMR 状态损坏）——测试前重启 vite（杀旧实例，注意端口被占时新实例起 5174）；用户提议"自编内容区状态栏"备选未采用（dockview 槽位零破坏，官方扩展点）

**聊天单元一体化轮（2026-08-15 用户纠正设计理解）**：
- **用户澄清**："SESSION 和聊天不是一个单元的两个界面，而是在一个单元里面"——聊天单元 = 列表 + 聊天框 + 上下文显示，列表显隐由顶部状态栏控制
- **落地**：ChatPane 内嵌可折叠会话列表（flex 侧栏 256px，SessionList 复用按场景过滤）；状态栏 **prefix 槽位** = 三横开关（SessionsToggle，与 token/X 同一行——dockview 三槽位：prefix=列表开关 / right=token 状态+关闭）；store `chatSessionsOpen` 按场景记忆显隐（chat 默认展开、coding 默认折叠）；布局 v6：chat 应用去掉独立 session-list 面板（列表收进聊天单元；session-list 视图保留注册表可加回）；ChatPane 内容区旧三横/悬浮窗移除
- 浏览器实测：聊天单元=列表+聊天一体 ✓ / 三横-token-X 同行 ✓ / toggle 收起恢复 ✓ / 编程对话默认折叠+展开见 coding 会话 ✓

**布局 v5 + 状态栏轮（2026-08-15 用户三点）**：
- **编程壳去编辑器（用户"一行代码都不用看"）**：布局 v5——中=任务清单主区 / 左=文件树 / 右=对话独立列 / 底部=终端|分支图叠放；编辑器视图仍在注册表（功能单元模式，随时可加回）；key v4→v5
- **状态栏升级（用户拍板开搞）**：dockview 8.1 `rightHeaderActionsComponent`（三个 header action 槽位之一）——面板标题栏右侧统一状态区 StatusBarActions（按组内活跃面板渲染状态项；当前=对话面板 TokenUsage）；token 数据源=后端新 API `GET /api/sessions/{id}/usage`（事件日志 assistant/message 事件 usage 聚合：input/output/messages）；前端图标+总量（Coins + 缩写），点击悬浮窗看明细（**宽度不足转悬浮窗**，用户拍板）；流结束（finalizeStream）usageVersion+1 → 自动重拉；浏览器实测真实消息 3.6K（输入 3512/输出 44）全链路通过
- **GitBar 精简（用户"github 横条没懂"）**：移除顶部横排提交时间线（commit 标题对不看代码的用户无意义），保留项目切换器+当前分支+变更数+刷新；提交历史看分支图 DAG 面板
- 坑记录：dockview header 区域元素 IAB 合成点击失效（须 dom_cua，同 Dock 磁吸坑）；TokenUsage 悬浮窗 backdrop 与 ChatPane 会话入口同款（z-40）

**布局与聊天单元轮（2026-08-15 用户开题三点，两项落地）**：
- **默认布局重定义（用户"你好好定义一下"）**：chat = 左会话列表/中对话/**右文件树**（工作目录随手翻文件）；coding = 左文件树/中编辑器+底部任务|终端|分支图叠放/**右对话独立列**（AI 干活主入口不再和终端挤 Tab）；布局 key v3→v4（用户自定义布局随版本重置一次，已知代价）
- **会话入口（用户"聊天单元一定要带 SESSION"）**：界面充足 = dock 会话列表面板（chat 应用已有）；界面不充足 = 聊天单元顶部三横按钮（panel 形态）→ portal 悬浮窗（SessionList 复用，`scene` prop 化——编程壳按 coding 场景过滤 + 新建对话）；token 用量/缓存命中状态栏 = 待接数据源，并入状态栏专项
- **状态栏统一（用户问题 2）评估**：dockview 8.1 原生支持 `headerComponent` 自定义面板标题栏——把 X 关闭行统一成状态栏可行，改动集中在 DockLayout 视图注册层（统一 header 组件），视图组件零改动；**待拍板**（影响全部面板观感，建议单独专项）
- 验证：tsc/lint/build 全绿；浏览器实测（经典界面）聊天三栏布局/编程四区布局/三横悬浮窗（coding 场景过滤 + 新建对话）/full 形态无三横全通过；**坑：IAB reload 后输入通道全失效（已知环境限制）——测试须新开 tab；导航右键重置布局在应用未挂载时静默忽略（先在应用内右键）**

**项目切换轮（2026-08-15，编程壳功能规划②执行）**：
- **项目模型**：前端项目集合（localStorage `boenmind.projects`/`boenmind.currentProject` 持久化，模式同 appSessionIds）+ 后端 workspace 路径参数化——`/api/workspace` 四端点（list/read/write/git-info）加 `root` 参数（缺省 = 配置工作目录兜底，设置页 working_dir 语义不变；`resolve_root` 空串视为缺省）
- **UI**：CodingApp 头部 ProjectSwitcher 下拉（切换/新建/删除；新建 = 名称+绝对路径，提交前 `listWorkspace` 探测可访问性，失败 toast 不落库；首个项目自动设为当前，后续新建不抢焦点；删除当前项目自动回退列表首个）
- **联动**：文件树/编辑器（previewFile 切换即清空）/GitBar/分支图全部带 `currentProject.root`；终端视图（dock-views 注册）以项目根为 cwd；**修复 FilePanel 不跟随切换 bug**（只在挂载时 navigateDir——补订阅 currentProjectId）
- **用户拍板：编辑器不再深化**（"我又不会写代码"）——M2 深化剩余的编辑器增强（diff/未保存守卫/目录新建）取消，编辑器的读取/保存保留现状
- 验证：后端 113 测试全绿 + clippy；前端 tsc/lint/build 全绿；浏览器实测 T1-T6/P1/P2 全通过（无项目空态/新建+自动当前+全联动/多项目/切换联动含非 git 降级/终端 cwd=项目根提示符/刷新持久化/删除回退/无效路径 toast）；截图证据在会话 artifacts（模型无图像输入，视觉验证受限说明）
- 已知行为：项目变化（删除回退）时 dockview 终端面板可能重建，终端跟随新项目根——符合预期

**M2 深化轮（2026-08-15，两项落地）**：- **分支图 DAG 可视化**：后端 /api/workspace/git-info 输出拓扑数据（commits 含 parents 边——`git log --branches --pretty=format:%h|%s|%p`，本地分支指针——`for-each-ref refs/heads`）；前端新视图 git-graph（lib/git-lanes.ts 纯函数泳道分配——旧→新遍历、主链同列、merge/分叉跨列曲线、lane 释放复用；SVG 渲染：节点圆点/merge 空心/分支标签锚 tip/当前分支高亮）+ VIEWS/DEFAULT_LAYOUTS 注册（编程壳右下第 4 个叠放 Tab）；**布局快照 key 版本化 v3**（DEFAULT_LAYOUTS 演进时 bump——默认布局新增视图能浮现；代价=用户自定义布局随版本重置，§四·C 插件默认布局落地时做快照指纹精细迁移）；浏览器实测（临时切工作目录到仓库）：6 tab 布局 + fetch 15 提交 + DAG SVG 渲染（15 节点/分支标签）；lane 算法 merge 拓扑 node 自检通过
- **EventQuery 类型过滤（长会话性能）**：EventQuery 加 `event_type` 字段（+ `EventQuery::of_type` 便捷构造）；turso read 重构为动态 SQL（params_from_iter，type 列过滤替代全量 replay）+ kernel 内存实现过滤 + EventLog 新方法 `read_where`（migrate 不 verify——过滤后 seq 不连续）；todo_tool.load_todos 改用 of_type("todo/write")；三处测试（turso SQL 层/kernel 内存层/todo 混合事件语义）全绿
- 验证：后端 4 crate 测试全绿 + clippy 零新增警告；前端 tsc/lint/build 全绿

**前端布局架构拍板轮（2026-08-15 用户开题"典型的软件界面"）**：
- **应用布局系统已拍板（架构 §四·B 补充 2，v0.23）**：软件界面 = VS Code workbench 模型——应用内容区 = 可停靠视图容器（dock layout）：停靠左/右/中上/中下 + 悬浮叠加、Tab 叠放切换、关闭/最大化、分界线拖拽、导航右键重置布局、每应用默认布局、设置界面不动
- **布局库定案（上游吸收 T5）**：dockview 8.1（@dockview/core + @dockview/react）——封装宿主组件 DockLayout + 视图注册表 VIEWS，不散用上游 API
- **视图实例语义（用户拍板）**：对话视图**单实例且绑定应用场景**（编程里的对话是编程专家，焦点在编程不会跑到 WIKI——复用 session.app 机制）；终端/文件树/任务列表/编辑器**可多开**；专家团队模式（多模型并行）属模型层语义另行拍板
- 实施 = 编程壳迁移（左文件树/中编辑器/右下任务|对话|终端叠放）→ 聊天应用 → 新应用默认布局声明

**应用布局系统已实施落地（布局拍板轮执行，2026-08-15）**：
- **依赖**：dockview-react 8.1.0（**注意 8.x 包名变了**：单包 `dockview-react`（React 绑定）+ 内部 `dockview`/`dockview-core`，7.x 的 @dockview/core+@dockview/react 已废弃）
- **宿主组件 `components/layout/DockLayout.tsx`**：实例化/每应用布局快照持久化（localStorage `boenmind.dock.<app>`，onDidLayoutChange 防抖落盘 + 卸载前兜底）/重置注册表（resetDockLayout 供壳层调用）/主题（resolvedTheme → dockview-theme-{light,dark} class，CSS 变量桥接 Tailwind 令牌明暗自动跟随）
- **视图注册表 `lib/dock-views.tsx`**：VIEWS（session-list/chat-pane/file-panel/editor/todo-panel/terminal = 宿主共享公共组件零改动嵌入，面板 params 表达形态与场景）+ DEFAULT_LAYOUTS（每应用默认布局声明——**新应用有可停靠视图在此声明一行即可**）；chat-pane 视图挂载即 ensureAppSession(params.app)（一软件一会话）
- **迁移**：编程壳（GitBar + DockLayout：左文件/中编辑器/右下任务|对话|终端叠放，对话=panel 形态 coding 场景）；聊天应用（左会话列表/中对话）；设置界面保持现状不动
- **导航右键「重置布局」**（只对有布局声明的应用 chat/coding 显示）；顺手修复：导航按钮 active 态不再标 aria-disabled（浏览器对 disabled 元素不派发 contextmenu，active 时右键会失效）
- 浏览器实测全通过：两应用默认布局 / 面板关闭 / 布局持久化（新标签页恢复关闭状态）/ 右键菜单重置（任务面板恢复）/ 明暗主题联动；拖拽停靠/分界线 = dockview 原生能力（IAB 合成拖拽不可行，真实鼠标验证）

**最近四轮回溯**：
- 修复轮（同日，用户定调"回头看查出问题先修"）：三真缺口修两件（declare_event! 宏 / branch/fork 事件）、压缩参数双轨打通（bm-core effective()）、memory/write 生产者接线、**内核第一根接线**（bm-compactor 经 KernelBuilder 装配进生产，bm 引擎从 kernel 取事件日志+压缩服务）——测试全绿 + clippy 零 lint
- 回头看+对标轮（本轮）：架构回头看（内核未接线三轨实锤/文档漂移修正）+ 全网对标三调研（底座前 10/记忆/插件同类，笔记 docs/research/2026-08-15/ 约 100KB 全部标注核实口径）→ 报告 docs/archive/REVIEW_ARCHITECTURE_2026-08-15.md + docs/REVIEW_LANDSCAPE_2026-08-15.md；架构文档当时 v0.21
- 代码回看轮（5c6451b）：三子代理并行审查全代码 → P0×3 修复（会话串行锁/压缩后 usage 重置/投影守卫）+ P1 一批（死代码/失败风暴/flusher 泄漏等）；报告 docs/REVIEW_CODE_2026-08-15.md；未修项（inbox 未接线/prompt_hash 契约/env 集中化）挂编程应用 M2
- 拍板轮（fa5019b）：pre-push 本地质量门（hooks/pre-push，GitHub 私有仓库无 Actions 免费额度）；pi 目录改名 `~/.boenmind/pi` → `~/.boenmind/agents`（启动自动迁移，真实验收过）；回头看立项材料 docs/REVIEW_BEFORE_CODING_APP.md（7 拍板点待拍）
- Steward 验收轮（059c9e6）：采集器全链路真实验收 + inject 锚点缺陷修复（note_round_done 推进 last_wake_at）

**⚠️ 当前唯一外部依赖**：GitHub 账户 Billing 未处理 → workflow 全瘫（含自托管 job，账户层拒绝调度）。质量门已由 pre-push 钩子本地接管，日常 push 不受影响；macOS 构建链（仅打 tag 触发）受影响，发版时再议。

## 轮次脉络（完整 commit 明细见归档）

| 轮次 | 要点 |
|---|---|
| LLM 厂商插件化轮 | 方案 A 落地（93d542f/6eed10a/165adcb）：ProviderKind 3 家 + ProviderPort（14 面）+ stable_id 取代 pi_name + custom 协议形状；旧配置 Unknown 兼容；交接文档归档 HANDOFF_LLM_PROVIDER_PLUGIN.md |
| 服务面铺开轮 | 当时 13 面注册（六批）；其后 +provider + 运行期 mcp。LoopHooks 12（当时写 10）。图纸 archive/SERVICE_FACES 止于 13 面 |
| 聊天单元一体化轮 | 会话列表收进聊天单元（ChatPane 内嵌可折叠列表）；状态栏三槽位（prefix=列表开关/right=token+X）同行；chat 布局 v6 去独立列表面板 |
| 布局 v5 + 状态栏轮 | 编程壳去编辑器（任务清单主区）；状态栏升级（dockview rightHeaderActions + token 用量 API/悬浮明细/流后刷新）；GitBar 精简去提交时间线 |
| 布局与聊天单元轮 | 默认布局 v4 重定义（chat 三栏含文件树 / coding 对话独立右列）+ 聊天单元会话入口（三横悬浮窗，SessionList scene 化）；状态栏统一评估待拍板 |
| 项目切换轮 | 前端项目集合 + workspace root 参数化 + 全视图联动（文件树/分支/GitBar/终端 cwd）+ 新建校验/删除回退；用户拍板编辑器不再深化 |
| M2 深化轮 | 分支图 DAG（git-info 拓扑数据 + GitGraph 视图 + lane 算法）+ EventQuery 类型过滤（长会话 todo 读取不再全量 replay）；布局 key 版本化 v3 |
| 布局实施轮 | 应用布局系统实施（dockview-react 8.1，T5）：DockLayout 宿主 + VIEWS/DEFAULT_LAYOUTS + 编程壳/聊天应用迁移 + 持久化/导航右键重置/主题桥接；浏览器实测全通过 |
| 布局架构拍板轮 | 应用布局系统拍板（§四·B 补充 2 v0.23，VS Code workbench 模型）：dockview 8.1 定案（T5）/对话单实例绑定场景/其他视图多开/默认布局+右键重置；实施排下一步动作 1 |
| 编程壳规划轮 | 吸收原则定调（网上最优解→官方插件→台账 UPSTREAM_TRACKING.md）+ TerminalPane 一期（xterm.js+portable-pty，ConPTY 应答坑，07618ce）；剩余项目切换/codegraph/浏览器仿真 |
| 对话宿主化轮 | 拍板 9 执行：ChatPane 宿主组件（full/panel）+ 会话 app 场景字段（迁移/API/引擎透传）+ 编程壳右栏 Tab + activateApp/ensureAppSession；隔离 home 全链路实测通过 |
| 架构讨论轮 | 内核主权评估定调（不换 dsh，§15.4）+ 对话宿主化拍板（§四·B 补充）+ 导航收口（底部仅设置+wiki 占位，a8397f8）；架构 v0.22 |
| M2 编程壳 | 独立壳起步（0fb2acb）：todo 工具+事件投影闭环 / 编辑器+写文件 / 分支图起步 / step_queue 删除（A6 修订）+步数预算提示；浏览器实测模型真实调工具全链路 |
| M1 验收 | 运行时自修真实 bug 全链路通过（8254bd7）+ 6 问题登记（本轮，报告 ACCEPTANCE_M1） |
| 回头看+对标 | 架构回头看两报告 + 三调研笔记 + 架构 v0.21 |
| 代码回看 | P0×3 + P1 修复（5c6451b） |
| 拍板轮 | 质量门方案 A / pi 改名 agents / 编程立项材料（fa5019b） |
| Steward 验收 | 采集器全链路 + 锚点修复（059c9e6） |
| Steward 续接 | 静默窗口/低成本模型/boot 汇报/前端状态页 + 窗口预算修复（b799dc3, 18a15e9） |
| CI 提速 | VMware 自托管 runner 接管质量门 3 Rust job（3005936）；sccache GHA 实证放弃 |
| pi 废除②③ | subagent 换 bm-loop（4997e8b）；legacy 删空 + asupersync 迁 vendor（0592cab） |
| 阶段 1 主干 | A1-A7/B1-B6 全部落地（见归档 commit 索引） |

## 下一步动作（按建议顺序，都可直接开工）

1. ~~**应用布局系统实施（已拍板 2026-08-15，架构 §四·B 补充 2 v0.23）**~~ → **已完成（2026-08-15）**：dockview-react 8.1（T5）+ DockLayout 宿主 + VIEWS/DEFAULT_LAYOUTS + 编程壳/聊天应用迁移 + 布局持久化/导航右键重置/主题桥接，浏览器实测全通过。新应用有可停靠视图时在 DEFAULT_LAYOUTS 声明一行即可；manifest 动态注册随 §四·C
2. **对话宿主化剩余**：skill **系统提示**按场景注入（引擎按 app 过滤扩展已做，勿混）
3. **codegraph 转官方插件**（@sdsrs/code-graph 0.116，scopes=["coding"]）→ 浏览器仿真 B 方案
4. **界面层插件化落地设计（§四·C）**：出方案文档待拍板（学 dsh 槽位；dock 是 ui-slots 超集）
5. **Steward 采集器挂任务计划程序**——待用户点头
6. ~~**MCP 组合根 / 插件禁用收回**~~ → **已修（2026-08-17）**：空管理器常驻；`tools_for_app` + executor 按 `enabled_plugins` fail-closed；禁用/卸载/MCP 连断均 invalidate

## 注意坑（浓缩操作要点，完整背景见归档 §〇·五）

**构建/测试**
- bm-compat 测试必须 `--test host --test load --test execute --test events --test session`（lib cfg(test) 缺上游 dev-deps，裸跑报 proptest 找不到）
- standalone 起服务必须 `--features embed`（否则 `/` 404）；本地测试/编译加 `CARGO_PROFILE_DEV_DEBUG=0`（debug exe 2GB 坑）
- **服务运行中不能编译**（exe 被锁，链接失败"拒绝访问"）；`cargo build | tail` 的退出码是 tail 的（吞失败码）——先停服务再编，别用管道接退出码
- 引擎选择：bm 默认已反转；resolve 逻辑在 bm_engine（env > settings > 默认）

**API/前端**
- Windows curl 中文 JSON 报 invalid unicode（含 em-dash 等 Unicode 标点）——验收用纯 ASCII 或浏览器
- `/api/sessions/{id}/events` 是 SSE 流，curl 会挂住——验证事件用 messages 面或日志
- IAB 浏览器 fill 不触发 React onChange（按钮 disabled 不解除）——须真实键盘 type

**引擎/压缩**
- MiniMax 流式须 `stream_options.include_usage`（默认 usage:null）；缓存字段在 `prompt_tokens_details.cached_tokens`
- pi/bm 对比口径：bm input=全量、pi input=未命中（勿双重计数）
- chars/4 粗估对中文低估 ~2×（水线判定用 max(粗估, 真实 usage)）；413/400 已修（工具结果 5MB 硬顶 + 窗口/2 预算双点）
- 压缩参数来源 = `[compaction]` 配置经 `CompactionConfig::effective(provider, model)` 换算（overrides 优先；enabled=false = 不挂压缩插件裸跑）；bm-compactor 经内核 KernelBuilder 注册为 "compactor" 服务，引擎从 kernel 取
- fork 后子分支 **seq 1 = branch/fork 标记**（首事件非空）——写会话生命周期工具（session.*，M3 起）时注意：fork 不再产生空分支

**插件/工具**
- 桥调用首参 secret 不绑定 JS 形参；tool_result 事件 content 用 ContentBlock 数组（`[{type:"text",text}]`）
- 内置工具 schema 须注册进 ToolRegistry（模型看不到就不会调）；SELF_TOOLS 跳过搜索类工具是设计
- 目录型插件须 extension.json；改插件源须同步 `~/.boenmind/extensions/` 副本；Disposer 必须交回 apply 的 Vec

**Steward**
- 管家提示词须覆盖式声明置尾（模型拒绝扮演管家）；验收用全新会话（身份历史污染）；inject 的 wake_after_seconds 会被回合内 set_wake 覆盖；验收加速 `BM_STEWARD_PACING_MIN_S=10`；两段式起服务（先无 env 建会话 → 带 BM_STEWARD_SESSION 重启）
- 静默窗口监视事件日志 head_seq（非共享 progress）；回合失败自动清 next_wake_at（防失败风暴，已修）

**subagent**
- 子进程协议 pointer 是 camelCase（`/assistantMessageEvent/delta`）；子进程无插件引擎（工具面=内置∩csv）
- `(&BTreeMap).clone()` 克隆引用须 `(*x).clone()`；`lines().next_line()` 返回 Result<Option> 要 transpose；取消传播靠 kill_on_drop

## 待拍板

1. ~~**编程应用 7 拍板点**~~ → **已拍板（2026-08-15 用户拍定）**：① M1 零新增直接验收；② **M2 独立壳应用起步**（注意：与文档原建议"现有前端加卡片"不同，用户选了独立壳）；③ 迁移门槛 **M3**；④ 三平台 T **后置**；⑤-⑦ 按文档建议执行（CI 方案 A 长期用 / pi 死数据随专项清 / 记忆写回契约 M1 后做）——M1 已过，M2 已起步
2. **界面层插件化机制拍板（§四·C，用户已拍板"要插件化，学 deepseek"）**：落地设计文档待出，拍板点：同域 bundle vs iframe 隔离；slot 粒度；前端包 manifest 字段
3. **对标吸收清单拍板**（docs/REVIEW_LANDSCAPE_2026-08-15.md §六）：高优先 9 条（dsh slot 机制/memory 契约字段/事件订阅/晶体模板/淡化三机制/pdf 基准/ponytail 技能/商店路线/.claude-plugin 兼容）执行时机——多数按阶段落地（记忆→阶段 5、slot→阶段 4、商店→§四·C），建议无需单独立项，随阶段吸收即可；另有 ACKEN 项目请用户提供来源后复核
4. `PI_SUBAGENT_*` 环境变量命名残留（自研协议通道仍用 pi 前缀）——改名待拍板
5. 商店"货架"方案（自维护清单 vs 对接 pi.dev）——悬置，随插件生态壮大再定
6. GitHub Billing 处理（用户操作；不处理则 CI 永久本地化，macOS 构建链发版时另想办法）
7. 远期（有触发时机，不急）：前端隔离机制（阶段 4）、沙箱层级（阶段 3）、平台驱动 ABI 纪律
8. ~~**pi 路径清理收尾**~~ → **已执行（2026-08-15 用户拍板）**：chat.rs pi 分支删除（dfb52f7，pi 退出生产路径，AppState 删 agents/aborts）
9. ~~**对话宿主化 + 插件作用域**~~ → **部分完成（2026-08-15/16）**：ChatPane + `session.app` + 引擎按 app 过滤扩展 ✅；§四·C 槽位 **未做**；skill 系统提示按场景注入 **未做**
10. ~~**设置中心 7 条**~~ → **已拍**（2026-08-16）：桌面全删留开关 / 专家同池 / 作用域 / `[apps]` 单源 / SKILL settings.json / 编程三专家 / 分级。见 SETTINGS_ARCHITECTURE §二

## 关联文档

- 架构：docs/everything-is-plugin-architecture.md（v0.26）
- 登记表：docs/EXTENSION_POINTS_REGISTRY.md
- 架构文件交叉审查：docs/REVIEW_ARCH_CROSS_2026-08-17.md
- 文档地图：docs/README.md（全部文档角色 + 决策轨迹速查）
- 对标调研：docs/REVIEW_LANDSCAPE_2026-08-15.md（笔记：docs/research/2026-08-15/）
- 归档（已完成/已解决）：docs/archive/（架构回头看 / 代码回看 / 长程测试 / 服务面图纸 / M1 验收 / 编程立项 / 阶段 0 计划 / 桌面壳 / 阶段 1 历史全量）
