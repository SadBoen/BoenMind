# 工具C：ln-24-architecture-auditor 独立审计报告

> 独立只读架构审计（2026-08-16），三工具交叉审查之工具 C。
> 执行契约：ln-24-architecture-auditor skill（levnikolaevich）。本报告未与其他审查工具通信。
> 仓库：`D:\96_CoderWorld\BoenMind`（Windows，路径以正斜杠书写）。只读：未修改任何代码或架构文档。

**Verdict: FAIL**（存在有证据的未解决安全边界缺陷与所有权歧义；详见结论）

---

## 审查概览（范围/方法）

### 范围
- backend/crates/**（bm-protocol/bm-kernel/bm-storage-turso/bm-server/bm-core/bm-loop/bm-memory/bm-compat/bm-compactor）
- backend/plugins/**（coding-memory/ctx-compactor/pdf-omni/refine-suggest/role/web-search）
- backend/tests/event_log/**、frontend/src/**
- 排除：vendor/**、target、node_modules、dist、docs、packaging、scripts、hooks、*.log
- 意图文档：docs/everything-is-plugin-architecture.md（v0.24，系统设计基线，当前）、docs/HANDOFF_KERNEL_PHASE1.md（活交接）

### 方法
1. 读意图文档 + workspace manifest + 服务入口（bm-server main/lib/serve_inner）+ git 状态（服务面铺开六批 commit 链，工作树干净）。
2. 全量静态盘点：13 服务面的注册点与**全部生产消费者**逐一 grep 追踪（`kernel.port::<dyn ...>` / `kernel.service::<...>` 调用点）；LoopHooks 10 挂点的实现清单；EventBus/subscribe/fork/enqueue_turn/declare_event!/Custom 事件的生产使用面。
3. 关键流程走查：chat 回合（chat.rs → bm_engine → run_turn → EventFlusher → turso append_batch → SSE）、权限询问链（host.rs capability check → request_approval）、Steward 调度、压缩事务（compact.rs 三事件）、todo 投影、subagent 子进程。
4. 每条发现按 skill 证据规则验证：可执行接线/调用路径/公开契约为准，目录名推断不产生发现；对照文档 §5.1/§5.2/§5.4/§6.9/§15.4 标注漂移。
5. 材料性门：仅保留产生具体变更/所有权/测试/部署/失败成本的发现；未验证项标注 UNVERIFIED。

---

## Checklist 完成度（44/44 complete）

**Checklist: 44/44 complete**
**Incomplete: None**（披露性限制见下）

| 节 | 项数 | 状态 |
|---|---|---|
| 1. Discover the Actual Architecture | 9 | 9/9 |
| 2. Audit Pattern Fitness and Ownership | 8 | 8/8 |
| 3. Audit Contracts and Dependencies | 10 | 10/10 |
| 4. Evaluate Evolution and Alternatives | 8 | 8/8 |
| 5. Validate Findings and Report | 9 | 9/9 |

披露性限制（不影响完成度判定）：
- 任务范围排除了 packaging/scripts/hooks/Dockerfile，部署边界分析基于 Cargo manifests 与入口（main.rs/serve_inner）完成；未做运行时诊断（只读，不启动服务）。
- bm-compat 引擎文件（extensions_js.rs 等 46K 行上游派生）按"框架约定/生成代码"过滤，不参与模式适配性评判；其加载/宿主接线（host.rs/load.rs/execute.rs/compat_engine.rs）正常审计。

---

## 实际架构（执行态，非文档态）

```
┌ 前端（React SPA，双 DE：ClassicShell 默认 + Desktop 壳，共用 APPS/VIEWS/DEFAULT_LAYOUTS 静态注册表）
├─ axum /api/*（~40 路由，auth=CORS 白名单 + 可选 BOENMIND_TOKEN）
├─ bm-server = 组装层（AppState 单一组合根，8 组件；kernel/compat/steward/terminal 全部 Optional）
│   ├─ bm_engine: ReactLoopAgent 装配（BmLoopAgent = StreamHooks + OpenAiClient + QuickJsToolExecutor）
│   │    └─ 会话串行锁 + agent 锁；agent 状态全在事件日志，弃置重建零损失
│   ├─ compat_engine: QuickJS 引擎宿主（专用线程 + 命令通道 + 7 种 hostcall）
│   ├─ kernel(KernelBuilder)：event_store/compactor 插件 + 11 个服务面 Port 注册
│   ├─ steward: 10s tick 调度器 + steward.json + set_wake 工具
│   └─ 内置域逻辑：todo_tool/subagent_tool/subagent_child/terminal/pdf_omni/governance/roles
├─ 双存储：boenmind.db（sessions/messages/tasks 权威） + event_log 表（双写 sidecar）
└─ 插件生态（三轨并存，文档 §15.1 已标注）：
    轨1 QuickJS TS 插件（6 个出厂，hostcall 通信，与 kernel 注册表零接触）
    轨2 loop 契约轨（Compactor trait / LoopHooks，bm-compactor 唯一实现）
    轨3 组装层编译内置（内置工具/Steward/subagent/pdf_omni/refine/skills）
```

关键流程：
- **聊天回合**：POST /api/chat → 用户消息落 db → run_turn（UserMessage/TurnStart/Step*/chunk 攒批 append_batch 落 event_log → 投影 derive_messages → LLM → AssistantMessage/ToolResult → TurnEnd）→ 助手文本再落 db → SSE 推送。原子性归属：event_log 归 loop（EventFlusher），messages 表归 bm-server 收尾，两存储无事务边界。
- **插件工具**：模型调用 → ToolRegistry（可见性）→ QuickJsToolExecutor 硬编码 if/else 分派（subagent/set_wake/todo/内置名 → 其余 QuickJS）→ hostcall pump 内 check_capability（仅 QuickJS 路径）→ 权限询问/决策记忆（fail-closed）。
- **权限**：插件 hostcall（http/exec/session/...）经 ExtensionPolicy（默认 prompt 模式；exec/env 默认拒绝）→ 询问链；**内置工具与 loop ToolGate 全程无检查**。
- **压缩**：步边界软触发（水线）→ compact()：投影 → 选区间 → 摘要 → 三事件一次 append_batch（Start/Summary{Replace}/End），摘要失败不遮蔽。

---

## Fitness summary

| Area | Status | Evidence |
|---|---|---|
| Pattern fitness and ownership | FAIL | 权限链归属分裂（F1）；双写原子性归属分裂（F4）；服务面投机泛化（F2） |
| Contracts and boundaries | CONCERNS | Port JSON 边界 + 双路径退化；压缩插件可换契约失效（F3）；TS 插件轨与注册表轨隔离（F9） |
| Dependency topology | PASS | bm-protocol 零运行时依赖；六 crate tests/architecture.rs 机器强制依赖方向；未发现禁边/环 |
| Physical structure and configuration | CONCERNS | 组装层 bm-server 承载 ~3400 行应用域逻辑；env 读取基本集中（steward 唯一读取点）；context_window 死配置（F7） |

---

## 审计发现

编号 | 类别 | 严重度 | 位置 | 观察与建议

### F1 | 架构 | High（P1） | 权限链归属分裂：内置工具完全绕过权限门，威胁方向倒置

**位置**：bm_engine.rs:238-249（`on_tool_pre` 恒 `ToolGate::Allow`，注释明言"权限裁决是 B5（此处恒 Allow）"）；compat_engine.rs:1016-1029（内置工具/子代理/todo 直接执行，无任何能力检查）；host.rs:131-160（`check_capability` + `request_approval` 只存在于 QuickJS hostcall pump 内）；对照文档 everything-is-plugin-architecture.md §5.4（把关链"tool/call → pre-execute → approval → execute"）与 §6.5（taint/审批设计）。

**证据链**：`StreamHooks::on_tool_pre` 只发 ToolCallStart SSE 事件后返回 Allow（bm_engine.rs:238-249）；`QuickJsToolExecutor::execute` 对 `bash/read/write/edit/grep/find/ls`、`subagent`、`todo`、`set_wake` 四条分支均无权限判断（compat_engine.rs:969-1029）；唯一有闸的路径是 QuickJS 插件发起的 hostcall（host.rs:131-160：prompt 模式 → 询问链，exec/env 默认拒绝）。即：**沙箱内不可信插件被层层设闸，而能执行任意进程的内置 bash 与子代理完全无闸**——提示注入（模型读到网页/文件内容）即可触发任意命令执行且无用户同意。

**为什么不可接受**：文档 §5.4/§6.5 描述的"工具把关链"与"结构性拒绝提示注入"是该产品的安全卖点；当前执行态中把关链对主路径（内置工具=模型日常手脚）不成立，"权限询问链已就位"（§7.2 阶段 2 状态）对内置工具不成立。本地优先单用户形态缓解了严重度，但归属歧义（loop 有门不用、hostcall 层有门管不到内置工具）是证据确凿的未解决安全边界缺陷。

**最小安全步**：二选一并文档化——① 内置工具执行前经同一 Gate（loop `on_tool_pre` 挂权限裁决，bash/subagent 至少走一次询问/决策记忆）；② 显式把"内置手脚 = 可信边界，不经审批"写进 §5.4 并说明提示注入缓解由其他层承担（如 §6.5 taint 落地）。推荐 ①，改动有界（一个裁决点 + 复用 permission_store）。
**实践参考**：本项目文档 §5.4/§6.5（权威契约）；[OWASP LLM Top 10 LLM01:2025 Prompt Injection](https://genai.owasp.org/llm-top-10/)（已验证：注入可改变模型意图行为，工具执行面是其放大器）。

---

### F2 | 架构/精简 | Medium（P2） | 服务面 13 面：5 面无任何消费者、8 面消费者全部"退化直调"双路径，违背自定 YAGNI 判据

**位置**：注册：lib.rs:418-483（11 个 with_port + compactor 插件）、lib.rs:509-530（tools/notify/gate 运行期注册）、lib.rs:547-554（scheduler）；trait 定义：bm-protocol/src/port.rs:121-303；消费者：bm_engine.rs:295-312（llm）、chat.rs:157-193（gate）、service_faces.rs:86-124（settings）、routes/skills.rs:15-59、routes/sessions.rs:24-68（session）、routes/sessions.rs:321（stats）；对照文档 §十 v0.21 接线判据（"后续接线判据（YAGNI）='第一个第二实现出现时'"）。

**证据**：全库 grep `kernel.port::<dyn ...>` 生产调用点共 9 处，对应 8 个面（event_store 由 kernel 内部持有、compactor 由 service 取用）；`memory/notify/scheduler/tools/credentials` 五个面**零生产 lookup**（仅 service_faces.rs 的实现与 lib.rs 的注册）。有消费者的面全部写成 `if let Some(kernel) && let Ok(port) { 走端口 } else { 直调 bm-core/db }` 双路径：routes/sessions.rs:37-49 同一 create 逻辑写两遍（端口返回 Value vs 直调再 to_value）、bm_engine.rs:295-312 llm 端口 JSON 往返后反序列化回 `LlmConfig`（与直调 `resolve_llm_config` 同一函数）。没有任何一个面存在"第二个实现"。

**后果**：~510 行 service_faces.rs + ~180 行 trait + 每消费者双路径（routes/sessions.rs:24-121 全线翻倍；错误类型两轨不一致：ProtocolError vs AppError，状态码映射分裂）；端口与直调行为各自演进时产生漂移面；五个死面成为无消费者扩展点。这是把"简单代码搬进接口/注册表"的典型形态——注册 13 面发生在自定 YAGNI 判据（首个第二实现）之前。

**最小安全步**：保留 event_store（真两实现：turso/内存）与 compactor；memory/notify/scheduler/tools/credentials 五个无消费者面**先撤销注册与 trait 保留声明**（代码可留，注册删除即可，回滚零风险）；有消费者的面待首个第二实现出现时再正式化，届时删除双路径。
**实践参考**：本项目文档 §十 v0.21 判据（权威）；Rust 官档 [std::any：Any 仅支持具体类型 downcast，不适用于 trait object 存在性判定](https://doc.rust-lang.org/std/any/)（已验证——支撑 F3 与 F2 的"注册表存 Any 服务"代价分析）。

---

### F3 | 架构 | Medium（P2） | 压缩插件"可换"契约被具体类型 downcast 破坏：换实现会被静默忽略

**位置**：bm_engine.rs:385-396（`kernel.service::<bm_compactor::DefaultCompactor>("compactor")` → `unwrap_or_default()` → 硬编码 `bm_compactor::DefaultCompactor { watermark: ..., ..base }`）；对照文档 §6.9（"策略层已插件化：bm-compactor 经 KernelBuilder 注册可换可关"）与 §15.4 演进路径（"策略层（已插件化）"）。

**证据**：消费者按**具体类型** `DefaultCompactor` downcast 而非 `dyn bm_loop::Compactor`；若注册第二个实现（不同具体类型），downcast 失败 → `unwrap_or_default()` **静默回落**默认参数（水线 0.5），替换面不生效且无告警；每会话参数实际由组装层 `EffectiveCompaction` 覆写，注册进 registry 的插件实例参数（除 min_middle_tokens 经 `..base` 保留外）基本被丢弃。宣称的"换实现"路径在运行期不成立。

**最小安全步**：消费者改为经 `kernel.port::<dyn Compactor>`（把 Compactor 提升为 port 或经 LoopConfig 直接注入 trait object），或诚实删除 registry 绕行改为构造注入，并在文档去掉"可换"承诺直至真实替换路径验证。
**实践参考**：Rust 官档 [std::any（Any::downcast 仅具体类型）](https://doc.rust-lang.org/std/any/)（已验证）；本项目文档 §6.9（权威契约）。

---

### F4 | 架构 | Medium（P2） | 双写迁移未收口：会话数据原子性归属分裂在 messages 表与 event_log 两个存储，崩溃窗口无对账

**位置**：dual_write.rs:1-9（"事件日志写失败**不阻断**主链路……主链路数据始终是权威。事件日志是渐进式吸收的新家，不是闸门"——与文档 §5.1"一切状态的唯一事实源"**方向相反**）；chat.rs:114-120（用户消息先落 db 再进 loop）；bm_engine.rs:856-861（助手文本在 run_turn 返回后才落 db，event_log 已含 TurnEnd）；recover_interrupted_turns（bm-storage-turso/event_log.rs:210-230）只补 TurnEnd{Interrupted}，**不对账两存储内容**；前端历史与 REST 读 db（routes/sessions.rs:87-121），todo 投影读 event_log。

**证据链**：崩溃窗口①：add_message(db) 后、UserMessage 事件落盘前 → 日志缺用户消息；窗口②：TurnEnd 落盘后、add_message 前 → db 缺助手文本（UI 丢失，日志有）。两窗口都无修复/对账路径，前端今天以 db 为准（丢文本），M3 断点续跑（文档标注的迁移门槛）"未开始"——双写作为长期并行机制没有消费者压力推动收口。

**最小安全步**：实现一个启动对账任务（以 event_log 为准重建/校验 messages 表，幂等），或显式把 M3 提为 backlog 冻结双写范围；在此之前在 §5.1 如实标注"当前真相源 = messages 表，事件日志为 sidecar（阶段 0 过渡态）"。
**实践参考**：本项目文档 §5.1（权威契约，标注漂移）；事件溯源权威材料可参考本项目 §3.6 L5/L16 自身吸收记录（事件源状态 = 投影重建）。

---

### F5 | 架构 | Medium（P2） | 内核事件总线（四件套之一）零生产接线；订阅仍 250ms 轮询，文档承诺的"loop 落位后换总线直推"未兑现

**位置**：bm-kernel/src/bus.rs（374 行，四分发模式，仅自身测试引用）；kernel.emit/on 全库零生产调用；subscribe_events 注释 bm-kernel/event_log.rs:341-344（"阶段 1 用轮询……A6 自研 loop 落位后换内核事件总线直推（无轮询延迟）"）+ 轮询实现 event_log.rs:364-400（每订阅者每 250ms 一次 store.read）；消费者 routes/sessions.rs:244（前端 SSE 事件流）。

**证据**：A6 loop 已是默认引擎（chat.rs:119 唯一执行引擎），但订阅仍轮询；每个前端会话面板订阅即 spawn 一个 250ms 轮询任务（turso SQL read/次）；总线 374 行实现成为死代码候选。轮询与总线**二选一**的状态悬置：若轮询正确，总线应删；若总线为演进目标，轮询应被替换——现状是平行机制并存且无消费者推动。

**最小安全步**：短期把轮询间隔按订阅数/活跃度退避（如 500ms-2s），或移除订阅者时停止任务（现状 stop 需外部置位）；中期决策：总线接线（loop 事件经总线直推）或删除总线实现，二选一，避免双机制维护。
**实践参考**：本项目文档 §5.2/§15.1（权威契约）。

---

### F6 | 精简 | Low（P3） | LoopHooks 10 挂点仅 4 个有生产实现，6 个新增挂点零实现

**位置**：bm-loop/src/points.rs:53-99（10 个钩子）；实现清单：bm_engine.rs:217-260（StreamHooks：on_request/on_stream_chunk/on_tool_pre/on_tool_post）、subagent_child.rs:260-263（SubagentHooks：仅 on_stream_chunk）；`on_pre_step/on_request_error/on_turn_stopping/on_context_build/on_compact_begin/on_compact_end/on_turn_end/on_provider_select` 全库无生产实现。

**证据**：服务面铺开轮新增 5 个挂点（context-build/compact-begin/compact-end/turn-end/provider-select，points.rs:50-52 注明"每个 = 一个真实需求"），但均无消费者——扩展点先行注册而无变异面。on_request_error 在 engine.rs:509-521 有调用语义（返回 false = 不重试），默认行为即全部失败。

**最小安全步**：保留（成本极低、接口稳定），但文档标注"仅记忆注入/流式转发在使用，其余挂点待接线"；新挂点按 F2 同一 YAGNI 判据（真实消费者出现时再加）。
**实践参考**：本项目文档 §十 v0.21 判据。

---

### F7 | 精简 | Low（P3） | CompactionOverride.context_window 死配置 + 模型窗口硬编码 128K

**位置**：bm-core/src/compaction.rs:44-45（`CompactionOverride.context_window` 字段，序列化在案）vs compaction.rs:73-87（`effective()` 从不读取该字段）；bm_engine.rs:378-379（`context_window: 128_000` 硬编码，注释"暂取默认 128K——后续从模型注册表换算"）。

**证据**：配置面暴露"按模型覆盖窗口"能力但执行面从不消费；硬触发判定（engine.rs:730-732）与工具结果预算（window_tool_budget_bytes）全部基于 128K 常量——换小窗口模型时硬触发可能失灵（超窗不触发压缩即回合失败），换大窗口模型时浪费预算。属"配置承诺与执行脱节"。

**最小安全步**：二选一——让 `effective()` 消费 `context_window` 并注入 LoopConfig，或删除该配置字段与文档声明（死配置清理）。
**实践参考**：无外部依赖，以本项目 §6.9 参数插件自治原则为准。

---

### F8 | 架构 | Low（P3） | fork 分支机制零生产发射者：分支维度是事件日志的"死维度"

**位置**：bm-protocol/src/port.rs:100-113（`fork_branch`/`branch_heads` 端口方法）；bm-storage-turso/event_log.rs:47-51（branch_heads.forked_at 迁移）、95-122（repair_heads 按分支维护）；CoreEvent::BranchFork（bm-protocol/src/event.rs）；全库 grep：`branch/fork` 事件与 `fork_branch` **无任何生产调用**（仅测试）。

**证据**：文档 §5.1 标注"branch_id 字段与 fork 事件类型已落地（2026-08-15）"，但 session.* 工具集（M3）未开始——机制（表结构 + 端口方法 + 事件 + 投影折叠语义）已全部落地生产 schema，发射者为零。schema/语义死重量在每次读路径（branch_heads 表维护、repair_heads）产生持续成本，且 `branch_heads.parent_branch/forked_at` 全部 NULL。

**最小安全步**：接受为 roadmap 承诺项（建议在文档标注"机制已落地、发射者随 M3"）——非缺陷，但列入本轮发现防止其成为无人认领的"已落地"幻觉。
**实践参考**：本项目文档 §5.1（roadmap 承诺，标注即可）。

---

### F9 | 架构 | Medium（P2） | QuickJS TS 插件轨与 kernel 服务注册表轨完全隔离：核心承诺"插件互相看见（ctx.<key>）"对现存插件种群不可达

**位置**：bm-compat/src/host.rs:44-114（HostServices 端口 = execute_tool/exec/http/session/ui/events + request_approval，共 7 种 hostcall 通道，**无 service/registry 通道**）；bm-kernel/src/ctx.rs（Ctx::service/register_service 仅 Rust 侧可及）；backend/plugins/*（6 个出厂 TS 插件全部经 `pi.tool/pi.http/pi.session/pi.events` 通信，无法触及 kernel 注册表）；对照文档 §5.2（"插件按 key 找服务，不 import 实现"）、§5.5（"服务注册表 = 一组 Port trait……插件实现 Port"）。

**证据**：TS 插件（现存的全部真实插件）与 kernel 注册表之间无任何桥接通道；服务面铺开（13 面）只服务 Rust 侧插件（实际只有 compactor 一个，且只注册不消费）；"插件互相看见"的承诺（文档 §〇"把插件装起来、让插件互相看见"）当前仅对理论上的 Rust 插件成立。文档 §15.1 标注了"三轨"，但未标注"注册表轨对 TS 轨不可达"这一能力边界。

**最小安全步**：文档 §5.2 明确标注"服务注册表当前仅 Rust 插件可达；TS 插件经 hostcall 面通信"；若 TS 插件需要消费服务面（如 settings），在 compat 增加 settings/session 类 hostcall（已存在 session）而不是让 TS 轨空转等待。属契约边界澄清 + 路线决策，非新实现。
**实践参考**：本项目文档 §5.2/§十五（权威契约）。

---

### F10 | 复用 | Low（P3） | memory 注入与 role 注入是同一"system 段有界追加"算法的两份实现

**位置**：bm-memory/src/lib.rs:94-114（`inject_payload`：找 system 消息追加/无则插入首条）；bm-server/src/roles.rs:35-56（`RoleInjector::inject`：同款算法，仅块文本格式不同）。

**证据**：两处实现逐行同构（find system → 追加 → 无则 insert(0)），差异只有注入块前缀（`[长期记忆]` vs `[当前角色：…]`）与上限来源；文档 D8（PromptSection 注册表）正是该挂点的规划统一家（§3.1 状态"待接线"）。算法复制在两处各自演化（如未来有界字符策略变更需改两处）。

**最小安全步**：抽出共享"system 段追加器"（有界字符 + find-or-insert），memory/role 两调用方复用；或直接按 D8 PromptSection 注册表落地（-100 身份/0 人格/100-199 工具分段）时统一。
**实践参考**：本项目文档 §3.1 D8（规划中的统一家）。

---

### F11 | 精简 | Low（P3） | kernel memory 服务注册全局单例（零消费者），与每会话 MemoryFilePlugin 实例并存双写同一 facts.md

**位置**：lib.rs:425-429（KernelBuilder 注册 `MemoryPortAdapter(Arc<Mutex<MemoryFilePlugin>>)` 全局单例，指向 app_dir/memory/facts.md）；bm_engine.rs:360-363（每会话另开 `MemoryFilePlugin::open` 同文件）；MemoryPortAdapter（bm-memory/src/lib.rs:129-151）全库零生产 lookup（见 F2）。

**证据**：kernel 注册的全局实例没有任何消费者（grep 无 `port::<dyn MemoryPort>` 生产调用）；每会话实例承担全部真实注入/写入；两个实例对同一 facts.md 并发 append（单行 append 容忍，但去重逻辑各自内存态，同一事实可能双写/互不可见）。双实例 = 平行机制 + 死服务面的叠加。

**最小安全步**：随 F2 一起撤销 memory 面注册（或让 memorize 入口真正经该服务消费，二选一）；若保留服务面，改为持有与每会话相同的共享单例（消除双实例）。
**实践参考**：本项目文档 §6.1（记忆插件契约，阶段 5）。

---

### F12 | 精简 | Low（P3） | inbox 回合队列（enqueue_turn/run/pending_turns）零生产调用者："回合源三分法"的统一队列未接线

**位置**：bm-loop/src/engine.rs:197-204（`enqueue_turn`/`pending_turns`）、254-265（`run()`）；生产路径 bm_engine.rs:842-851 直接 `run_turn`（单一 TurnRequest）；steward 调度（bm_engine.rs:1078-1106）也是直接 run_turn。

**证据**：`turn_queue` 结构体字段与三个公开方法全库无生产调用（仅测试）；文档 §14.2/§5.3 将"inbox 回合队列（next-turn）"描述为循环核心机制（"所有回合源汇入同一队列，天然串行"）；HANDOFF 已挂账"inbox 未接线（挂编程应用 M2）"。当前回合串行由 bm-server 的会话串行锁保证，队列机制是冗余平行设计（未接线形态）。

**最小安全步**：接受为挂账项，或删除队列字段/方法（回滚零风险，串行语义由上层锁承载）；M3 若需要回合内注入（inject 不唤醒语义）再按需恢复。
**实践参考**：本项目文档 §5.3/§14.2（规划承诺，当前未执行）。

---

### F13 | 架构 | Low（P3） | bm-server 组装层承载 ~3400 行应用域逻辑，"组装层编译内置"轨占产品逻辑主体

**位置**：bm-server/src 顶层模块 20 个：todo_tool.rs(305)/subagent_tool.rs(987)/subagent_child.rs(476)/terminal.rs(246)/pdf_omni/(1460+)/governance.rs(313)/steward.rs(539)/roles.rs(156)/permission_store.rs(398)/builtin_tools.rs(725)/compat_engine.rs(1344) 等；对照文档 §7.2 阶段 4（应用插件机制未做）、§15.1 三轨标注。

**证据**：todo/subagent/terminal/pdf_omni/governance/steward 均为产品域逻辑，全部编译内置在"组装层"crate（文档自己标注为轨 3"组装层编译内置"）；"应用=软件安装"（§四·C）与"能力插件化"（阶段 3-5）未落地前，bm-server 是事实上的"应用层"——其物理位置与"插件化"模型的差距随功能增长（M2 编程应用每轮都加模块）持续扩大；任何"换插件/关插件"语义对这些模块不成立。

**最小安全步**：接受为演进中形态（文档已标注），但建议在 bm-server 内建立子域边界（todo/subagent/terminal/pdf_omni 各自 mod 内聚已具备）；阶段 4 收编应用插件时按模块迁移，避免整包重构。
**实践参考**：本项目文档 §7.2/§四·C（演进承诺）。

---

### F14 | 其他 | Low（P3） | 事件日志 SSE 订阅轮询无退避且随会话面板数线性叠加（并入 F5 的落地代价）

**位置**：bm-kernel/event_log.rs:364-400（每订阅者 250ms 固定间隔轮询）；routes/sessions.rs:244；前端 subscribeEvents（frontend/src/api/client.ts:686）。

**证据**：订阅任务只在外部 stop 置位或读错误时退出；前端每开一个会话面板即订阅（todo/事件流），多面板时对 turso 的读压力线性叠加（250ms × N）；无指数退避/空闲停轮。本地单用户规模可容忍，但属无界轮询机制，且与 F5 同根（总线未接线）。

**最小安全步**：随 F5 一并决策；短期可对无新事件订阅做退避（如连续空轮询 ×2 间隔，上限 2s）。
**实践参考**：本项目文档 §6.3（投影同步两阶段协议）。

---

## 亮点（做得好且值得保持的）

1. **依赖方向机器化守卫**：bm-protocol/bm-kernel/bm-loop/bm-memory/bm-compactor/bm-storage-turso 六个 crate 各有 tests/architecture.rs（Cargo.toml 全形态解析 + 源码隐藏引用扫描），"吸收不进核心"（铁律 3）真正机器强制——这是本代码库最值得保持的纪律。
2. **事件日志存储层原子性**：turso `append`/`append_batch` 在单写者锁内完成 INSERT + branch_heads 同事务（bm-storage-turso/event_log.rs:306-442），且 `repair_heads()` 启动自愈——阶段 0 复核发现的"单条 append 非事务"缺陷已被正确修复并有回归测试。
3. **压缩事务协议（骨架/手脚分离）**：bm-loop/compact.rs 只承载三事件事务（Start/Summary{Replace}/End，一次 append_batch）+ fail-safe（摘要失败不遮蔽、不丢历史）；策略（水线/尾部/摘要 prompt）在 bm-compactor 插件。这是"插件自治 vs 核心协议"的正确切法，是 §6.9 拆法的最佳实践。
4. **EventFlusher 真序冲刷**：攒批 append_batch + 屏障语义（flush 保证读回自己写入前已落盘）+ Drop 兜底防任务泄漏（engine.rs:758-872）——流式 chunk 突发场景的正确批处理设计。
5. **崩溃恢复幂等**：`recover_interrupted_turns` 启动补写 TurnEnd{Interrupted}（bm-storage-turso/event_log.rs:210-230），与 dsh 语义对齐。
6. **权限询问链的 fail-closed 纪律**（QuickJS 路径）：决策记忆命中直返 → 询问 → 60s 超时按拒绝 → always 回写（compat_engine.rs:122-185）；无会话上下文（加载期）拒绝——该纪律应扩展到 F1 的内置工具面。
7. **工具结果双点裁剪**：写入点（5MB 硬顶 + 窗口/2 预算）与投影读取点（窗口预算）双重裁剪（engine.rs:617-636, 1001-1007），防单条超限结果永久污染会话历史——对 400/413 级故障的结构性预防。
8. **会话串行 + agent 锁双锁纪律与身份匹配取消**（lib.rs:44-48 bm_aborts + bm_engine.rs:700-709）：先结束的只删自己的条目，P0 回看成果已固化。
9. **Steward 治理工程化**：pacing min/max 夹区间（防热循环烧 token）、in_flight 防重叠投喂、静默窗口（head_seq 不变即掐断）、失败清 next_wake_at（防失败风暴）——"管家自驱动"从设想收敛为可运营机制（steward.rs + bm_engine.rs:1041-1110）。
10. **诚实标注文化**：文档对每个 partial/尾账/歧路都有显式标注（§7.2 状态、§10 清单、§十五、HANDOFF 注意坑），且 2026-08-15 架构回头看发现的"内核未接线"已被服务面铺开轮实质推进（Registry/loader/Plugin 轨已接入生产）——发现与修复循环运转良好。
11. **前端静态注册表形态**（APPS/VIEWS/DEFAULT_LAYOUTS）：编译期类型安全、宿主组件零改动嵌入、布局持久化/重置/主题桥接——作为 §四·C 动态加载前的中间形态是合理的最小实现，没有提前造 iframe/联邦的过度设计。

---

## 结论

**Verdict: FAIL**（有证据的未解决安全边界缺陷 + 所有权歧义；无 P0 级缺陷）

理由（按 skill 判定规则）：
- **F1（权限链归属分裂）**满足 FAIL 的"evidenced unresolved security boundary defect"：把关链文档承诺（§5.4/§6.5）与执行态分裂——loop 的 ToolGate 生产恒 Allow、内置最高权限工具（bash/subagent）零闸，而沙箱插件被严格设闸，威胁方向倒置。修复有界（一个裁决点），本地优先形态缓解但未消除。若团队显式判定"内置手脚 = 可信边界"并写入文档、且 taint/提示注入缓解在路线图上，该发现可降级为 CONCERNS。
- 其余发现为材料性非阻塞问题：F2/F5/F9 属投机泛化与平行机制并存（文档自定 YAGNI 判据被超前执行）；F3 属"可换"承诺失效；F4 属过渡迁移未收口且无对账；F6-F14 为精简/复用/物理结构低优先项。

### 演进顺序（按前置依赖与风险削减排序）
1. **立即（正确性/安全边界）**：F1 内置工具接入权限门或显式文档化信任边界。
2. **立即（低风险清理）**：F2 撤销 5 个零消费者服务面注册 + 消费者去双路径；F11 随 F2 消除 memory 双实例；F7 修死配置或删声明。
3. **短期（机制二选一）**：F5/F14 事件总线接线或删总线 + 轮询退避；F3 修压缩插件取用契约。
4. **中期（迁移收口）**：F4 双写对账任务或显式冻结 + M3 排期；F8/F12 标注 roadmap 承诺状态。
5. **长期（能力边界）**：F9 文档化 TS 轨/注册表轨边界并决策 hostcall 服务通道；F13 阶段 4 按模块收编。

### 残余结构风险（盲点）
- bm-compat 46K 行上游派生引擎作为第一方代码长期维护（HANDOFF 注意坑仍记载其测试须显式 --test 列表）——本次未运行其测试（只读约束），测试面健康度 UNVERIFIED。
- 前端三通道并存（chat SSE / REST messages / events SSE）与双存储真相源（F4）在 M3 前将持续作为既有事实，未发现额外缺陷。
- 本报告基于静态接线证据；未做运行期诊断（只读约束），任何"运行期可达性"声明以代码路径为准。
