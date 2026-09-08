# 未结事项总台账(BACKLOG)

> 定位:全仓「还欠什么」的**唯一入口**(ADR-0015 确立)。
> **维护口径(2026-09-02 用户裁决更新)**:已闭合条目**移出台账不再留行**,闭合史可溯 git 提交史;台账只留未结项,每项须有来源与状态;裁决「不做」的进 §7 备忘防重开。
> 状态口径:OPEN=未动工 / DEFERRED=用户拍板后置 / INPROGRESS=进行中。
> 出处注记指向的规格/回看/结算/时间线文件已按 ADR-0027(2026-09-08)移出仓,均溯 git 史。

## 1. 新工进行与待审批次

| 条目 | 范围与口径 | 状态 |
|---|---|---|
| 对话意图门控·硬门控设计 | 用户提报(2026-09-05):闲聊误触发工具。2026-09-06 二轮已落软防线(turn 挂工具时注入「工具纪律」System 段,84d1bb0);本条收窄为代码层硬门控(意图识别语义判定)的产品设计裁决 | OPEN(待设计裁决) |
| 上下文压缩(Compression)独立 MCP 工具 | 2026-09-05 讨论:超长会话滚动摘要、折叠与智能裁剪,独立为后续专门 MCP 插件,不与透视器混淆 | OPEN(待后续讨论) |
| 记忆(Memory)检索对话级自动注入 | 2026-09-05 讨论:turn.rs 回合组装时对接 SQLite FTS5 memory.search 自动召回相关记忆并注入提示词;2026-09-07 外部复盘复核扩口径:生产 server 装配面=model.invoke+exec/job_output+fs.*+skill scripts,`memory_capabilities()` 全仓零装配(仅 bm-testkit 测试引用),M5 记忆 CRUD/FTS5 实际只在测试台存活——「能力挂载+检索注入」一并待设计裁决(复核存档 REVIEW-2026-09-07-v0.0.13-external §三);2026-09-07 第三方评审复核补记:memory_search 的 LIKE 兜底对检索词长度/频次无限制(长词全表扫理论面,生产零装配下暂无实害),随本条一并设计 | OPEN(待后续讨论) |
| 模型自编工具结果(mimo 质量备忘) | 同轮实测:问 counter.bump 时模型未发起调用直接编造「bumped successfully」(/admin/context 证实 0 工具轮);对话区无 [调用] 标记即可辨真伪,强提示词可压不断根;随模型侧观察,不立项 | OPEN(记录在案) |
| Skill v0.2 第二步(scripts 执行面) | 第一步(合同 Minor: version + references)与 ADR-0016(Broker 七步管线覆盖脚本设计)已闭合交付;**第二步**:等待用户审阅确认 ADR-0016 后接入 wasmtime 执行引擎写代码 | OPEN(待 ADR-0016 确认后动工) |
| Agent 工具面远期增强三件(ADR-0022 候补) | ①Code Mode 式多轮往返脚本合并(DSH 已验证 5 次往返并 1 次);②Hermes tool_search 渐进披露(工具清单超预算时降级网关元工具,防 MCP 树撑爆上下文);③按模型条件化工具 schema(对标 Hermes patch 的动态裁剪,实测省 148 tok/次);来源=ADR-0022 调研报告 §9 P2/远期,主批未含 | OPEN(待排期) |
| 工具级辅助模型提供方(看图/TTS 等多模态手脚) | 用户提报(2026-09-06):允许给特定工具配置独立的 LLM provider——首个受益者=图片查看工具(工作区图片经视觉模型转文字描述回喂主模型,对标 DSH read_image/Pi read 视觉附件),同族还有 TTS 语音等;涉及 providers 注册表复用、能力 manifest 增发 provider_ref(合同 Minor)、turn 工具执行面外挂模型调用点 | OPEN(待排期) |
| VPS v0.0.5 发版后验证清单 | 随包扫描双目录已修+直通工具内联回喂已修+模型调用硬顶 30s→120s(BOEN_TURN_TIMEOUT_SECS 可配)均已落 main(f894663+本批);VPS 侧 web-multisearch 已远程装好并批准在役(2 工具)。待用户明示发版→VPS 升级后复测:①直通工具(echo/counter)对话秒回 ②真模型联网问答(web_search)全链路 ③关于页/常规设置不回归,闭合后移出 | OPEN(随下次发版) |
| W8 遗留:能力执行 cwd 注入 | ADR-0018 只做到回合 system prompt 注入;MCP/context-mode 等需要 cwd 的能力执行面尚未消费会话绑定工作区(该插件默认也未启用);2026-09-05 回看补记:内置 fs.* 同族——fs 工具相对路径在多工作区场景回退注册表首个根而非会话绑定根(guard.rs roots[0]),能力调用在核心层系无会话设计(system_session),修需穿合同面;与 Skill v0.2 执行线同批评估,继续经 Broker 管线、不新增特权通道 | OPEN(依 ADR-0016/0017 排期) |

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
| P3 大文件拆分 | broker.rs(1657 行)/turn.rs(1694)/task_ops.rs(1710)/sqlite_state.rs(1205);broker 建议拆法=mod+policy(GrantLedger)/credential/executor/audit;前端同族=context.tsx(2200+ 行,可拆 TrendChart/TokenWaterGauge/PromptRecipe/FileEffects 子模块)、PluginsPage.tsx(1800+ 行)与 thread.tsx(930 行,可拆 ApprovalDrawer/UserMessage/AssistantMessage/Composer);**webadmin.rs 已拆毕移出**(2026-09-07,11 子模块按域,commit 见 HISTORY);2026-09-07 架构评审扩口径:mcp.rs 1399 行(三种传输+McpHub+装载)/handle.rs 1119 行(start() 545 行含恢复+清点+bootstrap)/runtime.tsx 581 行/openai_http 与 glm_http 62% 重复抽公共客户端;2026-09-08 审计复核补记:runtime/handlers.rs 1158 行(会话/审批/操作/任务域混杂)、webadmin/mcp.rs 1235 行(随包插件编排面,与 JsonStore 收口同批评估为宜) | OPEN(缓办) |
| P4 非测试 unwrap 甄别清理 | 全仓约 400 处 unwrap 需区分测试/非测试逐步替换;非测试 panic 10 处均系不变量断言,评估=维持现状;2026-09-07 外部审查复核补记:load_world_rows(runtime.rs:208-309)expect×14 属同族——「恢复失败=拒开」是 handle.rs:81-94 明示设计决策,维持 fail-fast,可选小改=错误信息可读化(数据损坏时报「哪个 id 不合法」而非裸 expect) | OPEN(缓办) |
| 配置域读写样板收口(JsonStore) | 来源 2026-09-07 外部审查复核:providers/skills/roles/mcp 四组各一套 file→read_to_string→from_str→atomic_write 样板;skills 缺 write helper(写盘内联重复两处 skills_set/skills_delete);损坏口径已统一(2026-09-07 复盘复核批:skills/roles 改 Result 化=损坏拒绝覆写,与 providers/mcp 同口径,回归测试在 webadmin_tests);剩=泛型 JsonStore 收口与 write helper 补齐 | OPEN(低) |
| 前端 API 层收敛 | 来源 2026-09-07 外部审查复核:api.ts(508 行)只覆盖 W2 管理面;W1 运行时侧 8 处裸 fetch 绕过(main.tsx:16 / runtime.tsx×5 / thread.tsx:622 / AboutPage.tsx:65),收敛到 api/ 单源、类型与后端 schema 对齐 | OPEN(低) |
| P5 Capability 抽象演进 | 同步 invoke 无超时护栏(trait 注释已写明选型约束)/错误 String→结构化枚举/统一单 async trait 评估留 M 系列回看;2026-09-07 第三方评审复核补记:直通工具同步收据现走 spawn.rs inline_sync 特判分支(09-03 真实 P1 修复,有专项测试),收据 Sync/Async 两变体合同化属同族演进 | OPEN(缓办) |
| /v1 错误信封结构化 | 前端靠 `detail.includes("工作区")` 识别工作区错误(runtime.tsx),文案一改即失效;根修=扩展错误码(如 webui.workspace_unavailable,注册表 extensions/*.json+CI R6 同步,合同 Minor 仪式)或 /v1 400 带 JSON code 字段,前端按码分支 | OPEN(待排期) |
| core_loop 崩溃处置升级待裁决 | 现状=panic 仅 error 日志观测(命令方即刻收到错误,无挂死,2026-09-05 复核证实);升级选项=崩溃即进程 exit(70) 交 systemd Restart=on-failure 拉起,属运维行为变更待用户拍板 | OPEN(待用户裁决) |
| skill.v0_1 Rust 强类型投影 | 前端已有 SkillItem 类型,bm-contract 侧仅 JSON 常量无结构体;补 SkillDefinition+镜像测试(随 Skill v0.2 第二步动工前落) | OPEN(低) |
| 审批等待轮询改推送 | 回合管线对审批/异步工具 400ms 轮询单写者通道(GetOperation/GetOpResult,上限 300s);可改 watch/oneshot 通知,降单写者拥塞 | OPEN(低) |
| 异步执行器并发排队致工具假性超时 | 来源 2026-09-07 实战复现:fs.read 等异步能力共用执行通道,并发流洪峰下排队 >60s 即被回合侧判「工具执行超时」(操作本身最终 succeeded,独立调用毫秒级);评估=本地快路径(fs.*)改直通同步执行,或执行器多通道化 | OPEN(排障实录见 PLAYBOOK §3) |
| 新建对话按钮 1280×720 视口被顶栏覆盖 | 来源 2026-09-07 IAB 实测:小视口下 `button[title=新建对话]`(y≈9px)被 chat-head 层盖住,鼠标不可点(JS 直击可触发);桌面常规分辨率待复测,修=调层叠或按钮挪位 | OPEN(P3 UI) |
| 基线正文熔入欠账(ADR-0022..0025/§18 W10) | 来源 2026-09-07 文档内容级体检批:基线 §23 索引缺 ADR-0022..0025(本批已补行),正文未熔入四决策(原生工具协议/随包插件生命周期/limits 配置面/后台转轨,全文 grep 0 命中);§18 W 序列止于 W9(无 W10);§2.3「任何功能都不允许成为内核特权」未就地标注 ADR-0020 封闭清单例外按语(封闭清单正文已另处熔入)。按硬纪律 3「增补熔入正文」待下批回看统一处理 | OPEN(低) |
| 前端静态分析 | ESLint + Stylelint 接入 CI | OPEN |
| theme.css !important 收敛 | 玻璃段 4 处(毛玻璃化刻意选型,收敛须换实现手法) | OPEN(低) |
| FileSecretStore KDF 化 | 来源 FULL-REVIEW-2026-09-05 §7:主密钥 `&material[..32]` 截断非 KDF(HKDF/PBKDF2);get/put/delete 每次全量解密重加密 O(n);建议热路径 KDF+按需惰性 | OPEN |
| 前端 context 面契约锚定与类型漂移 | 来源 FULL-REVIEW-2026-09-05 §7:①w1/context.tsx 手维护 evMap/kind 字符串无后端锚定,枚举改名即静默掉卡;②`McpCandidatesResult` 在 PluginsPage 本地与 api.ts 双声明已漂移(source/bundled_dir 缺失)——收敛到 api.ts 单源 | OPEN |
| glm_http 错误分类与单测 | 来源 FULL-REVIEW-2026-09-05 §7:非 2xx 一刀切 Unavailable(400/401/429 不分,4xx retryable 靠 is_server_error 巧合);feature 门控默认不编,零单测;**错误分类已对齐 openai 口径**(2026-09-07 架构评审 P1-22:改用 openai_http::map_status),剩=补 feature 门控单测 | OPEN(低) |
| 测试裸 sleep 收口 | 来源 FULL-REVIEW-2026-09-05 §7:m7_health 200ms/1000ms 裸等待依赖调度时序(断言「迟到完成不污染收据」),慢机器易撕破;改 wait-for 终态轮询 | OPEN(低) |
| bm-cli 零单测 | 来源 FULL-REVIEW-2026-09-05 §7:CLI wire 调用错误码映射(ExitCode 表)无自动化回归 | OPEN(低) |
| MCP 插件杂项 | 来源 FULL-REVIEW-2026-09-05 §7:web-multisearch usage.rs 手写历法推月(跨月边界±1 天乱)/aggregate 超时无优雅取消;context-inspector 全量读 context-log 进内存(大目录 OOM 面)+stdio 主循环同步阻塞;两插件与主仓 stdio 框架三份重抄(独立 exe 原则既知代价) | OPEN(低) |
| openai_compat model 字段忠实性 | 来源 FULL-REVIEW-2026-09-05 §7:chunk/响应 model 恒回 default_model,W6 按条路由 requested_model 时回包撒谎(OpenAI 兼容面语义);恒 default_model 分支/非流式分支 webapp 永不消费(为第三方保留)一并评估 | OPEN(低) |
| /admin 面 Bearer 鉴权 | 来源 REVIEW-2026-09-07-architecture §3.1(P1-1/P1-2 后端半边):此前只登记在 W1/W2 规格,BACKLOG 无条目=跟踪断链,本次补立;现状=公网靠门户墙+portal 密码(VPS 已设),回环未配置密码时管理面裸奔;修=admin 中间件纳入 require_bearer+前端 admin fetch 全量带令牌 | OPEN |
| Grant 谓词精确模式设计裁决 | 来源 REVIEW-2026-09-07-architecture(P1-5 复核残留):审批抽屉刻意只授抽屉谓词(子集匹配是审批主路径),但委派 args_eq 全参快照场景下「已列键相等+额外键放行」存在超批准参数面;收紧需区分授权来源,涉 Grant 字段增发(合同 Minor),待设计裁决 | OPEN(待裁决) |
| fs_download 流式化 | 来源 REVIEW-2026-09-07-architecture(P1-16):256MB 上限内整读内存(zip 缓冲即响应体,单份);改流式 zip 低优 | OPEN(低) |
| skill_wasm 超时硬杀边界 | 来源 REVIEW-2026-09-07-architecture(P1-21):spawn_blocking 阻塞线程不可硬杀,现有 fuel 2e9 指令硬顶+tokio timeout 双限兜底;如需更紧可按 timeout 折算 fuel,涉脚本兼容评估 | OPEN(低) |
| 日期注释迁移 | 来源 REVIEW-2026-09-07-architecture(P2):bm-core 36 处「2026-09-0x 回看收紧」式变更记录注释应迁 commit message/ADR 源码不留;批量低优 | OPEN(低) |
| 日志风格统一 | 来源 REVIEW-2026-09-07-architecture(P2):boenmind-server/webadmin eprintln/println 与 tracing 两套并存,收敛为 tracing 单口径 | OPEN(低) |
| 轮询游标样板收口 | 来源 REVIEW-2026-09-07-architecture(P2):openai_compat 阻塞/流式两处+sse.rs 的 cursor+replay_since 循环抽公共迭代器 | OPEN(低) |
| manifest wire_name 自描述(工具短名演进) | 来源 2026-09-07 第三方评审报告复核(短名硬编码属实,系 2026-09-06 刻意交付勿擅动):spawn.rs 的 SHORT_WIRE_NAMES 硬编码表+system.exec→powershell/bash 平台分支,可下沉为 CapabilityManifest 可选字段 wire_name 自描述(manifest schema additionalProperties=true,增发=合同 Minor),内核注册表自动派生替代硬编码 | OPEN(低,待裁决) |
| Autorun 结构化终态判定 | 来源 2026-09-07 第三方评审报告复核(属实):autorun 完成哨兵纯靠模型自然语言输出 `[[AUTORUN_DONE]]` 前缀匹配(autorun.rs),弱模型吞标记即无法主动收束(停滞检测/max_turns 为兜底);评估=增结构化路径(哨兵工具调用/JSON 契约) | OPEN(低) |
| session_chats 规范化落盘 | 来源 2026-09-07 第三方评审报告复核:重启续聊现由 context-log.jsonl 逆向重建(history.rs rebuild_session_chats,W5 已验收刻意设计,勿修清单在册勿擅动);报告「诊断日志数据流倒挂」定性被驳回(context-log 系 A4 决策下对话正文唯一落盘),但 SQLite 表/事件增发规范化承载属合理演进方向,涉合同 Minor | OPEN(低,待裁决) |
| webadmin 管理操作事件审计 | 来源 2026-09-07 第三方评审报告复核(报告称「第二权力中心」):/admin 面直接磁盘/进程/配置操作不经 EventBus 审计属实,但管理面不入冻结合同系基线既有决策,单写者纪律管核心状态机;收敛提案=管理意图走核心管理命令入审计流,涉新 ADR | OPEN(低,待裁决) |
| provider/网关报错原文透传 | 来源 2026-09-08 限制归零批取证:模型网关 4xx 报错原文在 openai_http map_status 处被丢(detail_ref=None),用户只见「模型降级链耗尽/回合执行失败」,「命令总运行数量大于60」类真实死因查无现场;修法=错误原文进错误信封+context-log 快照(注意凭据脱敏边界,exec_log fail-closed 先例) | OPEN |

### 3.3 低优杂项

| 条目 | 说明 | 状态 |
|---|---|---|
| MCP 子进程 stderr 采集 | 现为 `Stdio::inherit()` 直通 server.log(W2 刻意诊断选型);后续可管道采集入插件页 | OPEN(低) |
| webapp 版本号 CI 自动对齐 | 来源 2026-09-07 外部审查复核:workspace Cargo.toml 与 webapp/package.json 手动同步(现 0.0.12 已对齐);可 release 流程 cargo metadata 提取写入或 CI 校验步防脱步 | OPEN(低) |
| bm-testkit 测试文件命名统一 | 来源 2026-09-07 外部审查复核:34 个文件 m1-m9/w5/w8/gt01/perf/无前缀混用;统一为 {category}_{feature} 纯机械改名,破坏 git blame,缓办 | OPEN(低) |
| 审批无人在线时的通知机制 | 来源 2026-09-07 外部审查复核:审批可达性已修(/admin/approvals 轮询+YOLO),但前端未连接时审批仍会长时间挂起;可加系统通知/声音/轮询提示等 | OPEN(低) |
| 同批 tool_calls 拒绝联动(产品语义) | 来源 2026-09-07 外部复盘复核(REVIEW-2026-09-07-v0.0.13-external §三):单回合多 tool_calls 顺序串行执行系单写者刻意语义不动;但用户拒绝其中之一后,同批其余工具仍各自独立过 Broker 审批并执行——是否改「拒绝即取消同批余下」属产品设计裁决 | OPEN(低,待裁决) |
| 前端长会话渲染性能 | 来源 2026-09-07 外部复盘复核:ThreadPrimitive.Messages 无虚拟滚动(多次「加载更早消息」后 DOM 全量堆积,大代码块/密集表格掉帧,可评估 @tanstack/react-virtual);流式期间逐 token 全文正则重扫+ReactMarkdown 全树重建(可加渲染节流);2026-09-08 审计复核补记:loadOlder 游标只增无累计上限(连点可无限前插),初始载入已有 50 条/页分页防护;与 P3 context.tsx/thread.tsx 拆分同族 | OPEN(低) |

## 4. 用户拍板后置(DEFERRED,历史裁决溯 git 史)

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
- PENDING.md 的 11 条 D-M 系列 user 裁决全部闭合(原表已随 ADR-0027 删除,溯 git 史)。
- S5(S4 部分实现)、S9(verification 三分法,M5/M7)裁决见 FULL-REVIEW §2.4(溯 git 史)。
- 其余已交付/已修复条目(审计 F-01~F-04/F-10、W4b 三件套、上下文透视面板、前端治理两轮、api_dsh 移除、release 线重写、坏 MCP 条目容错、冒烟套件等)已按 2026-09-02 维护口径移出台账,可溯 git 史。
- capability 操作不落 operations 表复核确认闭合(M4-review §6-4:纯内存系统容器态,规范状态由 approvals/grants/outbox 完整承载,行为正当)。
