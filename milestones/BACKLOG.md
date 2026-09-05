# 未结事项总台账(BACKLOG)

> 定位:全仓「还欠什么」的**唯一入口**(ADR-0015 确立)。
> **维护口径(2026-09-02 用户裁决更新)**:已闭合条目**移出台账不再留行**,闭合史可溯 git 提交史与 `milestones/HISTORY.md`;台账只留未结项,每项须有来源与状态;裁决「不做」的进 §7 备忘防重开。
> 状态口径:OPEN=未动工 / DEFERRED=用户拍板后置 / INPROGRESS=进行中。

## 1. 新工进行与待审批次

| 条目 | 范围与口径 | 状态 |
|---|---|---|
| 对话意图门控与闲聊防乱调用工具纪律 | 用户提报(2026-09-05):打招呼/闲聊时模型自主触发全仓扫描等过度动作。目标:参考业界Agent设计,在底层意图识别与回合组装建立防线,禁止无明确指令时擅自执行文件与系统探测 | OPEN(待排期) |
| 对话工具声明式按需挂载 (Role/Agent 工具白名单) | 用户架构既有规划(查案追记):当前 `turn.rs` 对 `chat_tools` 采取全量广播注入(所有注册能力全塞大模型背包),导致工具越多上下文背包膨胀越严重。目标:在 Role(角色)或 AgentSpec 中增加 `allowed_tools` / `allowed_capabilities` 声明式白名单(如日常助手仅挂载 fs.read/search/web_search,调试角色才挂载 context_inspector 等分析工具),实现按需动态装配与背包瘦身;暂不实现,排期待定 | OPEN(架构既有计划,待排期动工) |
| 系统审计日志与对话上下文生命周期边界 | 2026-09-05 讨论:会话删除时 context-log.jsonl 联动清理策略,审计日志(events.jsonl)保留元数据范围 | OPEN(待后续讨论) |
| 上下文压缩(Compression)独立 MCP 工具 | 2026-09-05 讨论:超长会话滚动摘要、折叠与智能裁剪,独立为后续专门 MCP 插件,不与透视器混淆 | OPEN(待后续讨论) |
| 记忆(Memory)检索对话级自动注入 | 2026-09-05 讨论:turn.rs 回合组装时对接 SQLite FTS5 memory.search 自动召回相关记忆并注入提示词 | OPEN(待后续讨论) |
| 模型自编工具结果(mimo 质量备忘) | 同轮实测:问 counter.bump 时模型未发起调用直接编造「bumped successfully」(/admin/context 证实 0 工具轮);对话区无 [调用] 标记即可辨真伪,强提示词可压不断根;随模型侧观察,不立项 | OPEN(记录在案) |
| Skill v0.2 第二步(scripts 执行面) | 第一步(合同 Minor: version + references)与 ADR-0016(Broker 七步管线覆盖脚本设计)已闭合交付;**第二步**:等待用户审阅确认 ADR-0016 后接入 wasmtime 执行引擎写代码 | OPEN(待 ADR-0016 确认后动工) |
| VPS v0.0.5 发版后验证清单 | 随包扫描双目录已修+直通工具内联回喂已修+模型调用硬顶 30s→120s(BOEN_TURN_TIMEOUT_SECS 可配)均已落 main(f894663+本批);VPS 侧 web-multisearch 已远程装好并批准在役(2 工具)。待用户明示发版→VPS 升级后复测:①直通工具(echo/counter)对话秒回 ②真模型联网问答(web_search)全链路 ③关于页/常规设置不回归,闭合后移出 | OPEN(随下次发版) |
| W8 遗留:能力执行 cwd 注入 | ADR-0018 只做到回合 system prompt 注入;MCP/context-mode 等需要 cwd 的能力执行面尚未消费会话绑定工作区(该插件默认也未启用);2026-09-05 回看补记:内置 fs.* 同族——fs 工具相对路径在多工作区场景回退注册表首个根而非会话绑定根(guard.rs roots[0]),能力调用在核心层系无会话设计(system_session),修需穿合同面;与 Skill v0.2 执行线同批评估,继续经 Broker 管线、不新增特权通道 | OPEN(依 ADR-0016/0017 排期) |
| W8 遗留:workspace_id 跨重启持久 | 现为进程内会话作用域(ADR-0018 决策 3:Web 会话指针重启即失效,持久化无用户可见收益);待会话列表真数据(W 后续)一并评估是否随会话恢复 | OPEN |
| web_multisearch:Parallel Search 接入 | 用户已供 Key(2026-09-04);其 `search_queries` 要求数组,通用 JSON 适配器只能发字符串(实测 422),需在插件加内置特例解析(仿 jina);模板死路两处(tavily/linkup)已随 2026-09-05 回看批修复闭合 | OPEN(待排期) |

## 2. 流程收尾

| 条目 | 来源 | 状态 |
|---|---|---|

## 3. 技术债(缓办,均不影响日常使用)

### 3.1 掉链项(前序回看承诺「随某里程碑补」未接住;来源 FULL-REVIEW §4)

| 条目 | 来源 | 状态 |
|---|---|---|
| Orchestrator 单进程等价映射复测(独立进程形态) | M5-review §6-1 | OPEN |
| Task 级停滞窗口/并发上限配置字段 | M6-review §6-1(M5 部分闭合) | OPEN |
| 成员级 max_concurrent_tools 定标 | M6-review §6-1 | OPEN |
| 多代 stdio 进度聚合 | M7-review §6-5 | OPEN |
| P-06 RSS 独立采样接入 perf 套件 | M1 §6-2 → M2 → M3 §6-4 后无接续 | OPEN |

### 3.2 后端与前端重构债

| 条目 | 说明 | 状态 |
|---|---|---|
| F-05 | 200+ 行函数重构债(与 L-01/R-08 同批) | OPEN(缓办) |
| F-07 | bm-surface-http → bm-persist 直依赖待裁决(收口或留档);2026-09-05 回看补记:webadmin.rs 还在 HTTP Handler 里直接 spawn MCP 进程/装配 StdioMcpTransport/管理 McpHub 连断与 Provider 密钥播种,装配职责宜下沉运行时,与本条同批收口 | OPEN |
| F-11 | memory_drawer_verdict 硬编码权限规则与 ADR-0006 张力(broker.rs 已补注;合同化重构待排期) | OPEN |
| F-12 | bm-core → bm-persist 依赖倒置(2026-09-05 回看发现):内核 Cargo.toml 直依赖实现层,内核代码直接使用 sqlite_state 行 DTO(CapabilityRow/ApprovalRow/GrantRow 等);宜将端口 trait 与持久化入参 DTO 收归 bm-core::ports,投影转换归 bm-persist | OPEN(缓办) |
| P3 大文件拆分 | broker.rs(1657 行)/turn.rs(1694)/task_ops.rs(1710)/sqlite_state.rs(1205)/webadmin.rs(1882 行);broker 建议拆法=mod+policy(GrantLedger)/credential/executor/audit;前端同族=context.tsx(2200+ 行,可拆 TrendChart/TokenWaterGauge/PromptRecipe/FileEffects 子模块)与 PluginsPage.tsx(1800+ 行) | OPEN(缓办) |
| P4 非测试 unwrap 甄别清理 | 全仓约 400 处 unwrap 需区分测试/非测试逐步替换;非测试 panic 10 处均系不变量断言,评估=维持现状 | OPEN(缓办) |
| P5 Capability 抽象演进 | 同步 invoke 无超时护栏(trait 注释已写明选型约束)/错误 String→结构化枚举/统一单 async trait 评估留 M 系列回看 | OPEN(缓办) |
| /v1 错误信封结构化 | 前端靠 `detail.includes("工作区")` 识别工作区错误(runtime.tsx),文案一改即失效;根修=扩展错误码(如 webui.workspace_unavailable,注册表 extensions/*.json+CI R6 同步,合同 Minor 仪式)或 /v1 400 带 JSON code 字段,前端按码分支 | OPEN(待排期) |
| session_chats 重启重建(重启续聊) | 多轮对话台账纯内存(runtime.rs 有意取舍注释),服务重启后会话在而记忆空;修=启动恢复阶段从事件流回放重建台账(注意 20 轮/24K 双上限裁剪口径) | OPEN(待排期) |
| core_loop 崩溃处置升级待裁决 | 现状=panic 仅 error 日志观测(命令方即刻收到错误,无挂死,2026-09-05 复核证实);升级选项=崩溃即进程 exit(70) 交 systemd Restart=on-failure 拉起,属运维行为变更待用户拍板 | OPEN(待用户裁决) |
| 前端会话历史回放端点 | 会话切换/页面刷新后消息列表无法恢复(现切会话即复位视图,bm-session-switched 已接线);需按 session_id 拉历史消息的端点(或 /admin/context 聚合)后前端回放 | OPEN(待排期) |
| skill.v0_1 Rust 强类型投影 | 前端已有 SkillItem 类型,bm-contract 侧仅 JSON 常量无结构体;补 SkillDefinition+镜像测试(随 Skill v0.2 第二步动工前落) | OPEN(低) |
| 审批等待轮询改推送 | 回合管线对审批/异步工具 400ms 轮询单写者通道(GetOperation/GetOpResult,上限 300s);可改 watch/oneshot 通知,降单写者拥塞 | OPEN(低) |
| 前端静态分析 | ESLint + Stylelint 接入 CI | OPEN |
| theme.css !important 收敛 | 玻璃段 4 处(毛玻璃化刻意选型,收敛须换实现手法) | OPEN(低) |
| 持久读错误折叠为空收口 | 来源 FULL-REVIEW-2026-09-05 §7:handle.rs 启动恢复 8 处与 events_for_session 等对 store 读错 `unwrap_or_default()` 折叠为空=故障消音成假数据,与「宁可拒开」相悖;损坏 grant 行被跳过会致 bootstrap 协调权重签发(安全侧,需故障注入端到端验证后按拒写口径统一) | OPEN(高优候选) |
| emit 形状校验失败制造事件日志 seq 空洞 | 来源 FULL-REVIEW-2026-09-05 §7:runtime.rs emit 坏形状事件先占 seq 后拒写,后续落盘事件 seq 跳号,违反 INV-3(Judge contiguous 可检出,生产路径无保护);修=坏形状事件不占 seq 或占位补 tombstone | OPEN |
| MCP 治理四件 | 来源 FULL-REVIEW-2026-09-05 §7:①reload 不强杀旧子进程(僵尸窗口);②respawn 无去抖/无上限;③`restart_limit` 配置解析后零消费(死配置,删或实施);④HttpMcpTransport 裸 send 无超时(远端挂起=调用悬挂) | OPEN |
| fs.write/edit 原子写+大小上限 | 来源 FULL-REVIEW-2026-09-05 §7:fs.write/fs.edit 直接覆写原文件(进程崩溃留半截文件),同仓 atomic_write 标准未应用;且无大小上限(MAX_FILE_BYTES 只拦 search) | OPEN |
| system.exec cwd 沙箱化 | 来源 FULL-REVIEW-2026-09-05 §7:cwd 参数未在 input_schema 声明即被消费(additionalProperties 默认放行),不经 fs_tools 工作区白名单;审批卡为主防线,补 schema 显式化+cwd 白名单校验 | OPEN |
| FileSecretStore KDF 化 | 来源 FULL-REVIEW-2026-09-05 §7:主密钥 `&material[..32]` 截断非 KDF(HKDF/PBKDF2);get/put/delete 每次全量解密重加密 O(n);建议热路径 KDF+按需惰性 | OPEN |
| 前端 context 面契约锚定与类型漂移 | 来源 FULL-REVIEW-2026-09-05 §7:①w1/context.tsx 手维护 evMap/kind 字符串无后端锚定,枚举改名即静默掉卡;②`McpCandidatesResult` 在 PluginsPage 本地与 api.ts 双声明已漂移(source/bundled_dir 缺失)——收敛到 api.ts 单源 | OPEN |
| webapp 401 正向引导链路 | 来源 FULL-REVIEW-2026-09-05 §7:主 App 启动不查 /api/portal/state,门户会话过期(服务器重启即失效)后用户只见红条需自行猜 /login;启动查 state+401 统一跳登录 | OPEN |
| glm_http 错误分类与单测 | 来源 FULL-REVIEW-2026-09-05 §7:非 2xx 一刀切 Unavailable(400/401/429 不分,4xx retryable 靠 is_server_error 巧合);feature 门控默认不编,零单测 | OPEN(低) |
| 测试裸 sleep 收口 | 来源 FULL-REVIEW-2026-09-05 §7:m7_health 200ms/1000ms 裸等待依赖调度时序(断言「迟到完成不污染收据」),慢机器易撕破;改 wait-for 终态轮询 | OPEN(低) |
| bm-cli 零单测 | 来源 FULL-REVIEW-2026-09-05 §7:CLI wire 调用错误码映射(ExitCode 表)无自动化回归 | OPEN(低) |
| MCP 插件杂项 | 来源 FULL-REVIEW-2026-09-05 §7:web-multisearch usage.rs 手写历法推月(跨月边界±1 天乱)/aggregate 超时无优雅取消;context-inspector 全量读 context-log 进内存(大目录 OOM 面)+stdio 主循环同步阻塞;两插件与主仓 stdio 框架三份重抄(独立 exe 原则既知代价) | OPEN(低) |
| openai_compat model 字段忠实性 | 来源 FULL-REVIEW-2026-09-05 §7:chunk/响应 model 恒回 default_model,W6 按条路由 requested_model 时回包撒谎(OpenAI 兼容面语义);恒 default_model 分支/非流式分支 webapp 永不消费(为第三方保留)一并评估 | OPEN(低) |

### 3.3 低优杂项

| 条目 | 说明 | 状态 |
|---|---|---|
| MCP 子进程 stderr 采集 | 现为 `Stdio::inherit()` 直通 server.log(W2 刻意诊断选型);后续可管道采集入插件页 | OPEN(低) |

## 4. 用户拍板后置(DEFERRED,裁决记录见 milestones/PENDING.md)

| 条目 | 来源 | 状态 |
|---|---|---|
| S4 draining 两步摘除实测 | M7 §6-1 → M8 §6-4 → M9 §6 | DEFERRED(留档后置) |
| lease 通道真实吞吐实测 | M4 §6-1 → M8 §6-4 | DEFERRED(留档后置) |
| 桌面安装包(`cargo tauri build` 出 .exe) | M3 §6-1 → M8 §6-1(D-M8-3) | DEFERRED(搁置不排期,骨架在 shell/tauri) |
| memory 条目级删除所有权 | M9-review §6-2 | DEFERRED(演进项) |
| 空谓词子任务兼容期 | 来源 FULL-REVIEW-2026-09-05 §6-1:授权子集已按安全侧收紧(child 空谓词越出具体 parent=拒绝);若现网有以空谓词成功创建的子任务用法,升级后此类 spawn 将被拒——是否需要兼容期 UI 提示/存量豁免 | DEFERRED(待用户裁决) |
| 失败调用 usage 接入 | 来源 FULL-REVIEW-2026-09-05 §6-2:失败回合已计入回合配额(token 记 0);若网关对失败调用回执部分 usage,是否按实际接入记账(需 provider 侧解析失败响应 usage,合同字段或需 Minor 增发) | DEFERRED(待用户裁决) |
| 门户 logout 端点 + Cookie Secure 标志 | 来源 FULL-REVIEW-2026-09-05 §6-3:前端无退出按钮、后端无 logout 口;会话 Cookie 缺 `Secure` 标志(TLS 部署前置);建议随下次发版补齐,涉前端+portal 两面 | DEFERRED(待用户裁决,建议随发版) |

## 5. 候选队列(用户提过、未排期)

| 条目 | 范围与口径 | 状态 |
|---|---|---|
| context-mode Rust 后续安全执行面 | 将当前可选宿主执行替换/增强为真正的 wasmtime/WASI 或操作系统级沙箱；补齐上游客户端 hooks、skills、网络抓取的 BoenMind 适配（首版仅迁移 MCP 核心） | OPEN(依 ADR-0017 与 ADR-0016 后续排期) |
| 前端跨模块通信收编(2026-09-04 风格统一轮遗留) | `bm-*` CustomEvent 广播(7 处:chat-new/open-music/ws-refresh/workspaces-changed/roles-changed/providers-changed)收编为带类型的统一事件表或轻量 store,消除字符串事件名散落 | OPEN(架构债,不影响功能) |
| 弹窗表单容器模板统一 | 各设置子页 Dialog 表单间距/结构各异(space-y-4 vs grid gap-3),抽 FormDialog 模板统一留白与 footer 规范 | OPEN(低优先,随下次触及相关页面顺手做) |

- 使用反馈轮:当前阶段,按「先真实使用一周」节奏收集反馈;
- App 产品面完整设计(manifest ui_panels 自动导航/数据域隔离/生命周期):远期,音乐播放器 App 先行探路。

## 6. 已闭合备忘(留出处,勿重开)

- **内置能力全量 MCP 化 = 用户裁决不搞(2026-09-02)**;复核意见=同步 trait 快路径系刻意选型(零进程开销),全量子进程化对单用户场景属倒退;热插拔诉求由 §1「MCP 完整热插拔」承接。
- 合同 Minor 三笔:tools maxItems 0→16 / finish_reason 加 tool_calls / invoke_response 加 tool_calls(commit 40988a7,W4)。
- PENDING.md 的 11 条 D-M 系列 user 裁决全部闭合(见 milestones/PENDING.md 表)。
- S5(S4 部分实现)、S9(verification 三分法,M5/M7)裁决见 FULL-REVIEW §2.4。
- 其余已交付/已修复条目(审计 F-01~F-04/F-10、W4b 三件套、上下文透视面板、前端治理两轮、api_dsh 移除、release 线重写、坏 MCP 条目容错、冒烟套件等)已按 2026-09-02 维护口径移出台账,可溯 git 史与 HISTORY.md。
- capability 操作不落 operations 表复核确认闭合(M4-review §6-4:纯内存系统容器态,规范状态由 approvals/grants/outbox 完整承载,行为正当)。
