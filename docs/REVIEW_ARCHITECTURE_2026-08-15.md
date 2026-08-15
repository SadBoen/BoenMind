# 架构回头看报告（2026-08-15 第二轮）

> 触发：用户"再来一次回头看，主要是指架构"。方法：架构文档 v0.20（docs/everything-is-plugin-architecture.md）
> 全文对照真实代码——子代理代码级核查（37 次工具调用）+ 关键论断本人复核（subagent 引擎、内核接线面、fork/todo 事件）。
> 上一轮回头看 = 代码质量回看（docs/REVIEW_CODE_2026-08-15.md），本轮 = **架构符合度**。随行调研报告见 docs/research/2026-08-15/。

## 结论摘要

**架构骨架与代码基本同构、无推翻性偏差。** 本轮最大发现：**"内核已建成但未接线"**——内核四件套里只有事件日志在生产路径使用，注册表/插件加载器/事件总线（Plugin trait）零生产引用。"万物皆插件"目前的真实形态是**双轨**：QuickJS pi 生态轨（用户看到的 6 个 TS 插件）+ loop 契约轨（Compactor/LoopHooks 可换实现）；组装层仍有大量编译期内置功能（7 内置工具/Steward/subagent/pdf_omni 核等）——**不是所有功能都是插件**（详见 §五）。

次要问题：文档漂移若干（已落地未勾）、压缩参数双轨未打通、插件域注册式事件/GlobalSeq/fork 事件类型三个"承诺首版应落"的小缺口。

## 一、达标项（架构承诺已兑现，有代码证据）

| 承诺 | 证据 |
|---|---|
| 内核行数预算 <1.5 万 | bm-protocol 908 + bm-kernel 2975 + bm-loop 2177 = **6060 行**（含准内核）；加 compactor/memory 也仅 6401 |
| 依赖方向守卫（L9）机器化 | bm-kernel/tests/architecture.rs：FORBIDDEN=[bm-server, bm-core]，Cargo.toml 解析 + 源码 use 扫描双层负向验证；bm-compactor 同款守卫 |
| bm-protocol 零运行时依赖 | 仅 serde + serde_json |
| 压缩策略拆分（§6.9 定调） | 已落地（6cbe56d）：bm-loop 只留 Compactor trait + 三事件事务协议 + 硬触发兜底；bm-compactor 新 crate（水线/尾保留/摘要 prompt 全部公开字段 = 插件自治） |
| 事件信封全要素 | version(=1，旧数据缺省 0 拒绝)/seq/session_id/branch_id/ignorable/surface_op(Replace)/source_seqs 全实现 |
| 核心域事件全覆盖 | turn/step/user/assistant×2/tool×2/request/header + compaction×3 + memory/write + **todo/write** + session/end-seed（todo/write 已实现 = M2 活任务清单事件协议现成） |
| 阶段 1 随行小修 | Interrupted 补写、fork 父前缀折叠（存储层）、deferred 拓扑加载、PortBox、per-plugin Disposer 全部落地 |

## 二、漂移项（文档 vs 代码不一致，须修文档）

1. **§十 清单未勾已落地项**：compact.rs 拆出（已落地）、Steward 调度器（已落地）仍标 `[ ]`。
2. **§6.9 "拍板点：现在拆 vs B6 后拆"** 已过时——已拆完且收口（双开对比也在 18a15e9 完成）。
3. **§14.4 ③"pi 目录残留清理"** 状态过期——pi 目录已改名 agents（fa5019b），剩余 models.json 死数据挂专项拍板点。
4. **压缩参数双轨未打通（真实漂移）**：bm-compactor/lib.rs 头注声称"参数由组装层从 bm-core 配置换算注入"，实际 bm_engine.rs:327 直接 `DefaultCompactor::default()`；bm-core/compaction.rs 的 DEFAULT_WATERMARK=0.50（旧 pi 配置层）仍在——两处水线并存未换算，配置改水线当前无效。**要么打通换算（随 M1 后），要么改头注如实标注。**

## 三、未落地项（按计划后置 / 真缺口）

**按计划后置（阶段 2-5，不算欠账）**：工具把关链升级、平台驱动 ABI、前端 SDK/投影引擎/应用插件贡献点（阶段 4）、session.* 工具集（阶段 5）、记忆 MemoryPlugin trait 六方法（阶段 5）——注意 M3 断点续跑会先触及 resume/archive，届时 session.* 从阶段 5 提前单件落地。

**真缺口（架构承诺"首版应落"而未落）**：

1. **插件域注册式事件机制**（§5.2 两层分治的第二层）：declare_event 等价物未实现，app/*、infra/*、goal/*、schedule/* 域无注册通道——目前只有核心域强类型 enum。
2. **GlobalSeq 全局事件游标**（§11.4 拍板点 9 承诺"契约层先留口"）：未找到；bm-compat scheduler.rs:779 的 global_seq 是 QuickJS 宏任务序号，同名不同物。
3. **fork/merge 事件类型**（§5.1 承诺"首版落 branch_id 字段与 fork/merge 事件"）：branch_id 三维寻址与存储层 fork_branch（含测试）已落，但事件类型未落——fork 是存储级操作不是日志事实。与上轮代码回看"fork 无事件类型"记录一致，保持待办。

## 四、核心落差：内核已建成但未接线（本轮最大发现）

**事实**（本人复核确认）：

- bm-server 对 bm-kernel 的全部使用 = EventLog / InMemoryEventStore / subscribe_events / projection（bm_engine.rs、compat_engine.rs、subagent_child.rs、routes/sessions.rs）。
- **Registry / loader / Plugin trait / 事件总线（emit/waterfall/parallel/serial）在生产路径零引用**——它们有完整实现和测试，但没有一个生产组件经它装配。
- 内核四件套的接线状态：日志原语 ✅ 已接线；注册表/加载器/事件总线 ❌ 自足的未接线内核。

**"万物皆插件"的现实形态 = 双轨 + 一内置**：

| 轨 | 机制 | 现状 |
|---|---|---|
| QuickJS 插件轨 | bm-compat（vendored 53K 行）+ 6 个 TS 插件（web-search/ctx-compactor/pdf-omni 薄壳/refine-suggest/bookmark/hello） | ✅ 用户真实使用的"插件" |
| loop 契约轨 | Compactor trait（bm-compactor 可换）+ LoopHooks（bm-memory 注入） | ✅ 可换实现，默认随组装层发布 |
| 组装层内置（非插件） | 7 内置工具/Steward/subagent/pdf_omni Rust 核/refine/skills/updates/权限/前端单页应用 | ❌ 编译期硬编码，占功能面大头 |

**影响判断与接线判据**：

- 功能不受影响——所有已验收能力都真实可用（阶段 1 的既定 strangler 形态）。
- 但注册表/加载器/事件总线约 3K 行"建成未接线"是纯成本 + 腐烂风险（L9 测试防方向倒退，不防接线面腐烂）。
- **接线时机建议（YAGNI："第一个第二实现出现时"接）**：① 记忆插件化（阶段 5，file/vector 实现出现时 MemoryPlugin 走内核 registry）；② 网络策略插件（10057 类"换实现"诉求出现时）；③ 平台驱动（mac 端口出现时）。在此之前不强制接线，但架构文档 §十 应**诚实登记"内核未接线"状态**，避免误导为"已是插件化运行时"。

## 五、"当前是不是所有功能都是插件"——直接回答

**不是。** 清点（证据 file:line 见子代理审查记录）：

| 功能 | 落点 | 是否插件 |
|---|---|---|
| web-search / ctx-compactor / pdf-omni(薄壳) / refine-suggest / bookmark / hello | backend/plugins/（TS + QuickJS 沙箱） | ✅ 真插件 |
| 压缩策略 | bm-compactor（Compactor trait） | ✅ 契约插件 |
| 记忆注入 | bm-memory（LoopHooks::on_request + facts 传送带，215 行雏形） | ⚠️ 半插件（六方法 trait 未实现，写入口 = governance.rs 硬编码规则，memory/write 事件空壳无生产者/消费者） |
| 7 内置工具 read/write/edit/grep/find/ls/bash | bm-server/builtin_tools.rs（653 行，文件头明言"不查插件注册表"） | ❌ 组装层编译内置 |
| Steward 管家 | bm-server/steward.rs（472 行）+ governance.rs（312 行） | ❌ 硬编码（架构定位=内置应用插件，形态未成） |
| subagent 子代理 | subagent_tool.rs（987）+ subagent_child.rs（475）——子进程跑**自研 bm-loop**（InMemory 日志 + BuiltinTools + OpenAiClient，已核实） | ❌ 普通工具注册 |
| pdf_omni 解析核心 | bm-server/src/pdf_omni/（9 文件 2686 行 Rust 编排） | ❌ 编译内置（TS 薄壳经 loopback REST 调它） |
| refine 审批 / skills / updates 热升级 / 权限桥 / 插件管理面 | bm-core + routes | ❌ 编译内置（旧 pi 核残余） |
| 前端（Chat/设置/插件/skills/管家页） | frontend 单页应用（导航状态切换，无路由/无贡献点） | ❌ 应用插件层未落地（阶段 4 计划内） |

**一句话**：真插件只有 TS 插件层 + loop 契约层两轨；组装层与 bm-core 内仍有大量编译期内置功能——这是阶段 1 既定形态，但"内核插件机制未接线"的进度落后于架构文档暗示的状态，须如实登记。

## 六、与上轮代码回看的衔接

- 上轮记录的"§5.1 两处偏差（fork 无事件类型、压缩锁恢复未实现）"：本轮复核 fork 事件类型仍缺（存储层 fork 已落，见 §三）；压缩锁恢复保持待核待办。
- inbox 双队列未接线 / prompt_hash / BM_STEWARD_* env 集中化 / 15min 任务驻留——挂 M2 的项不动（交接文档已有）。

## 七、建议（优先级排序）

1. **文档同步（半小时级）**：§十 勾选已落地项（compact 拆出/Steward 调度器）、§6.9 拆法状态更新、§14.4③ 状态、§5.1 fork 事件类型与 §5.2 插件域事件的"已落/未落"如实标注、"内核未接线"登记 + 接线判据。
2. **压缩参数打通或头注修正**（二选一，M1 后顺手）：bm_engine 组装层从配置换算注入 vs 标注 default 直用。
3. **编程应用 M1 不受影响**（事件日志 + bm-loop + 工具集全就绪）；M2 活任务清单的 todo/write 事件协议现成；M3 断点续跑先落 session.resume/archive 单件。
4. **记忆写回契约**（memory/write 生产者接线）挂 Steward 深化轮——枚举空壳已在，成本低。
