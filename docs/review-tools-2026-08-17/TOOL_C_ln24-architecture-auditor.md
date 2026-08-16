# Architecture Audit

**Verdict:** FAIL

**Checklist: 44/44 complete**

**Incomplete: None**

> 独立只读架构审计（2026-08-17），工具 C：`ln-24-architecture-auditor`。未与其他审查员通信，未改代码。
> 仓库：`D:\96_CoderWorld\BoenMind`（branch `main`）。范围：frontend + backend；排除 `backend/crates/bm-wiki` 功能完整性、wiki GUI、`/api/wiki`、`wiki_*` 工具本身；排除 vendor/target/node_modules。wiki 仅评估是否污染既有边界。
> Git：工作树仅有无关改动 `frontend/src-tauri/Cargo.toml`、`frontend/src-tauri/Cargo.lock` 与未跟踪 `dist-win/`——不计入既有架构。
> 只读：未启动服务、未写诊断缓存。运行期可达性以静态接线为准。

---

## Checklist

### 1. Discover the Actual Architecture — 9/9

- [x] 读仓库指令、架构文档、manifest、入口、配置所有权规则。证据：`docs/everything-is-plugin-architecture.md`（v0.25，系统设计基线，当前）、`docs/HANDOFF_KERNEL_PHASE1.md`、`docs/EXTENSION_POINTS_REGISTRY.md`、`docs/README.md`、`docs/design/SETTINGS_ARCHITECTURE_2026-08-16.md`（实施中）、`backend/Cargo.toml` 与各 crate manifest、`bm-server/src/lib.rs`（`router`/`serve_inner`/`AppState`）。
- [x] 区分系统设计基线与上轮审计基线并分类。证据：系统设计基线 = `everything-is-plugin-architecture.md`（current）；上轮审计基线 = `docs/review-tools-2026-08-16/TOOL_C_ln24-architecture-auditor.md` + `docs/REVIEW_TOOLS_CROSS_2026-08-16.md`（prior audit）；扩展点登记表 current；设置架构 proposed/in-progress；wiki 设计文档 `docs/design/WIKI_APP_2026-08-17.md` 本轮排除功能审。
- [x] 映射包/模块/层/进程/存储/外部系统/公开接口。证据见「Actual architecture」。
- [x] 记录所有权与独立构建/失败边界。证据：Cargo workspace 11 生产 crate + `tests/event_log`；进程边界 = 单 `bm-server`（可选 Tauri 壳同进程调 `init`）+ QuickJS 专用线程 + subagent 子进程；存储 = `boenmind.db` + `event_log`；外部 = LLM HTTP、MCP stdio/HTTP。
- [x] 识别主导组织模型与竞争模型。证据：主导 = 插件边界 + 组装根（万物皆插件 / ProviderPort / 默认 bm-loop）；竞争 = 三轨并存（QuickJS / loop 契约 / 组装层编译内置）+ 双存储过渡。
- [x] 追踪关键流。证据：`POST /api/chat` → `chat.rs` → `chat_bm` → `run_agent_turn`；权限 = `BuiltinGate`/`McpGate`/`ask_capability`；配置 PUT = `routes/config.rs`；压缩 = `kernel.port::<dyn Compactor>`。
- [x] 对照文档当前态/目标态/迁移与可执行结构。证据：§5.1 已如实标注 messages 表为当前真相源；§5.4 把关链对内置高权限工具已接线；配置双持有与文档「组合根单一配置」漂移（本轮新发现）。
- [x] 检查 Git 状态。证据：`main`；HEAD `458f21e`（wiki 应用）；脏文件仅 tauri Cargo.* + 未跟踪 `dist-win/`。
- [x] 保持只读并披露诊断。证据：无运行期诊断、无生成分析产物。

### 2. Audit Pattern Fitness and Ownership — 8/8

- [x] 按行为识别模式并评分。证据：组合根 `AppState`、Port 注册表、loop 钩子、双写 sidecar、权限询问链。
- [x] 抽象是否消除真实波动。证据：`ProviderPort`/`Compactor` port 消除厂商表/策略实现波动；`tools`/`notify`/`scheduler`/`credentials` 待接线面按用户定调不记缺陷。
- [x] 层方向、域所有权、编排深度、副作用边界。证据：六 crate `tests/architecture.rs` 守卫；`run_agent_turn` 统一编排；wiki 引擎独立 crate、组装层仅门面。
- [x] 跨组件事务/会话/连接/流/后台工作所有权。证据：event_log 单写者锁在 storage；会话串行锁在 bm-server；compat 专用线程；Steward 10s tick。
- [x] 状态变更关键流的原子性所有者。证据：用户消息 `chat.rs` 先写 db；loop `EventFlusher` 拥有日志；助手文本 `run_agent_turn` 收尾再写 db。双写已冻结标注。
- [x] 只读/纯接口的隐藏写。证据：`SkillPort::list` 只读；`SkillPort::set_enabled` 写 **kernel 持有的 config 副本**（与 `AppState.config` 分裂，见 F1）。
- [x] 平行机制/半完成迁移/无消费者扩展点。证据：扩展点登记表 14 面 10 消费中 4 待接线；总线/挂点按定调不升 P0/P1。
- [x] 失败处理与重试归属。证据：权限 fail-closed（超时拒绝）；压缩摘要失败不遮蔽；event_log 写失败不阻断主链路（冻结策略）。

### 3. Audit Contracts and Dependencies — 10/10

- [x] 公开 API/服务/事件/持久化契约。证据：`frontend/src/api/client.ts` 消费 `/api/*`；SSE 形状在 `chat.rs` 注释；`bm-protocol` 事件/Port。
- [x] 边界泄漏/布尔模式/不稳定序列化。证据：Port JSON 边界仍在（LlmPort 往返 `LlmConfig`）；前端 `AppConfig` TS 类型是后端超集的子集，运行时 GET 全量回写，未见抹字段路径。
- [x] 解析内部依赖边。证据：各 `Cargo.toml`；`bm-wiki` 零内核依赖；`bm-mcp` 仅依赖 `bm-protocol`；`bm-server` 组装全部。
- [x] 禁/允依赖规则。证据：机器守卫 `FORBIDDEN = bm-server/bm-core` 于 protocol/kernel/loop/memory/compactor/storage。
- [x] 环、禁边、扇入扇出。证据：未发现禁边/环；`bm-mcp` 的 `McpService` 未进 `bm-protocol`（kernel 以 `Any` 注册，生产 lookup 走 `AppState.mcp`）。
- [x] 耦合的具体成本。证据：配置双副本使 skill 启停与聊天注入面分家（F1）。
- [x] 事件名/模式生产者消费者一致。证据：todo 读 `todo/write`；前端历史读 messages 表；events SSE 读 event_log。
- [x] 物理结构与垃圾抽屉。证据：bm-server 仍是应用域主仓（轨 3）；wiki 模块化进独立 crate，组装层只留 `wiki_tools`/`routes/wiki`。
- [x] 配置边界。证据：`AppState.config`（tokio RwLock）与 kernel `shared_config`（std RwLock clone）两份；门闩 `ask_high_risk` 启动快照。
- [x] 运行时发现。证据：路由在 `router()` 静态注册；服务面在 `KernelBuilder`/`register_port`；BuiltinGate/McpGate 在 `serve_inner` 装配进 executor。

### 4. Evaluate Evolution and Alternatives — 8/8

- [x] 当前架构痛点。证据：配置所有权分裂（新）；双写冻结窗口（保留已接受）；组装层持续膨胀（wiki 又加门面，可接受）。
- [x] 系统设计基线对照。证据：§5.1 漂移已标注；§5.4 执行态已对齐高权限内置工具；配置「单一组合根」未写进基线但被 Port 实现破坏。
- [x] 上轮审计基线对照。证据：见「与 2026-08-16 对比」。
- [x] 外部模式/框架行为核验。证据：已打开并核验 OWASP CSRF Origin 校验、OWASP Authorization「每次请求用当前策略」、Tokio shared-state、OWASP LLM01:2025。
- [x] 与最简可信替代比较。证据：把 `shared_config` 改成与 `AppState.config` 同一 `Arc`，并让门闩读实时档位——单点修复，无需换模式。
- [x] 倾向有界修补而非重写。证据：F1 建议即此。
- [x] 拒绝投机现代化。证据：不建议删待接线扩展点；不建议现在收口双写。
- [x] 仅在需要时给迁移序。证据：F1 可一次有界改；无需分阶段迁移。

### 5. Validate Findings and Report — 9/9

- [x] 结构发现经调用/注册/契约路径验证。证据：F1 调用链见下。
- [x] 过滤生成代码/框架约定/测试架构。证据：`bm-compat` 上游派生引擎不参与模式评判；其 host 接线已审。
- [x] 材料性门。证据：无消费者扩展点未立项；双写冻结列为保留已接受债务而非新 P0/P1。
- [x] 每条发现含已打开核验的实践参考 URL。证据：F1 链接已 fetch。
- [x] P0–P3 分级。证据：1 条 P1。
- [x] 每条含边界、后果、为何不可接受、最小下一步。证据：F1。
- [x] 建议按前置排序。证据：「Evolution order」。
- [x] Verdict 规则。证据：存在未解决的正确性+安全策略所有权缺陷（P1）→ FAIL。
- [x] 返回架构图、适配性、发现、限制与残余风险。证据：本文件。

披露性限制（不影响完成度）：未做运行期诊断；`bm-compat` 大文件按框架约定过滤；wiki 功能完整性按任务排除。

---

## Actual architecture

### 模块、边界、入口、外部系统

单进程本地 AI 心智平台。Cargo workspace 成员：`bm-core`、`bm-server`、`bm-protocol`、`bm-kernel`、`bm-storage-turso`、`bm-loop`、`bm-compat`、`bm-compactor`、`bm-memory`、`bm-mcp`、`bm-wiki`（本轮不审功能）、`tests/event_log`。前端 React SPA（`frontend/src`，契约消费在 `api/client.ts` + `stores/app-store.ts`）。

```
前端 React（ClassicShell 默认 + Desktop 壳；APPS/VIEWS 静态注册表）
  └─ /api/*（axum）
       中间件：CORS 本机白名单 → auth（可选 BOENMIND_TOKEN）→ origin CSRF（非 GET）
bm-server = 组合根 AppState
  ├─ bm-loop（默认引擎）+ StreamHooks + OpenAiClient + QuickJsToolExecutor
  ├─ bm-compat QuickJS 宿主线程（TS 插件轨）
  ├─ Kernel（event_store + 服务面 Port + bm-compactor 插件）
  ├─ BuiltinGate / McpGate（与 PermissionStore 共享）
  ├─ Steward / terminal / 内置工具 / pdf_omni / governance / roles
  └─ wiki 门面（routes/wiki + wiki_tools）→ bm-wiki（独立 crate，无内核依赖）
存储：boenmind.db（sessions/messages 当前消息真相源）+ event_log（sidecar；todo 投影已闭环）
外部：LLM HTTP、MCP server（stdio / streamable HTTP）
```

主导模型：插件边界 + 组装层编译内置（轨 3 仍是产品逻辑主体）。服务面声明 14+1（registry 含 `provider`/`mcp`）。默认 loop = bm-loop。

### 关键流与运行时接线

- **聊天回合**：`POST /api/chat` → 用户消息落 db → `chat_bm` 解析 provider/model（读 `AppState.config`）→ `get_or_create_loop_agent`（身份 = provider/model/thinking）→ `run_agent_turn`（超时/attach/`run_turn`/detach/助手文本落 db）。Steward 共用同一 `run_agent_turn`。
- **权限**：executor 在分派前调 `BuiltinGate::check`（仅 `bash`/`subagent`）与 `McpGate::check`（`mcp__*`）。插件 hostcall 仍走 `ExtensionPolicy`。`on_tool_pre` 仍恒 `Allow`（卡片事件，裁决已下沉到 executor）。
- **压缩**：`kernel.port::<dyn Compactor>("compactor")`；第二实现不再静默 downcast 失败回落参数。`context_window` 经 `EffectiveCompaction` 注入 `LoopConfig`。
- **记忆**：会话经 `kernel.port::<dyn MemoryPort>` 取全局目录单例；缺失才本地回落。
- **配置写路径分裂**：`PUT /api/config` / 插件启停写 `AppState.config`；`SkillPort` 启停写 kernel `shared_config`（启动时 `config.clone()`，之后无同步）。

### 当前态 / 目标态 / 过渡 / 漂移

| 态 | 内容 |
|---|---|
| 当前执行态 | bm-loop 默认；权限门已挂高权限内置工具与 MCP；双写冻结；配置两份内存副本 |
| 目标态（基线） | 事件日志唯一事实源；插件互相看见；服务面可换 |
| 活跃过渡 | 双写至 M3；扩展点待接线；应用插件化阶段 4 未做 |
| 漂移 | 文档 §5.1 已对齐双写；配置所有权未文档化且执行态双持有（新） |

wiki：`bm-wiki` 不依赖 kernel/loop/server；`bm-server` 单向依赖并在组装层挂 REST + 场景工具。未发现反向依赖或内核污染。

---

## Fitness summary

| Area | Status | Evidence |
|---|---|---|
| Pattern fitness and ownership | FAIL | 配置/策略所有权分裂（F1）：组合根与 kernel Port、权限门闩三处各持一份启动快照或 clone，热更新后聊天注入、LLM 解析、档位闸门不一致 |
| Contracts and boundaries | CONCERNS | Port JSON 边界与双路径退化仍在，有消费者的面可工作；TS 轨与注册表轨隔离仍在（已登记，非本轮缺陷）；前端契约与后端超集靠运行时透传 |
| Dependency topology | PASS | 六 crate architecture 守卫仍在；`bm-protocol` 仅 serde；`bm-mcp`/`bm-wiki` 不反向依赖组装层；无禁边/环 |
| Physical structure and configuration | CONCERNS | 配置双副本是结构缺陷（并入 F1）；组装层仍承载应用域（轨 3，可接受演进）；env 读取仍集中在 StewardConfig |

---

## Findings

| Priority | Problem | Evidence and justification | Required resolution |
|---|---|---|---|
| P1 | 运行时配置所有权分裂：`AppState.config`、kernel `shared_config`、权限门闩启动快照三套事实源，热更新后策略/技能/密钥不收敛 | **边界**：组合根应对一份可变配置；`serve_inner`（`lib.rs` 约 575 行）`shared_config = Arc::new(RwLock::new(config.clone()))` 与随后的 `AppState.config` 再包一层，之后再无写回。`PUT /api/config`（`routes/config.rs:50`）只写 `state.config`；`SkillPortImpl::set_enabled`（`service_faces.rs:279-282`）只写 kernel 副本并落盘；`set_skill` 优先走 Port 再 `invalidate_loop_agents`；下一回合 `get_or_create_loop_agent` 用 `state.config` 拼 `enabled_skills_prompt`——UI 启停的 skill **不进入**聊天注入面。`BuiltinGate`/`McpGate` 的 `ask_high_risk`/`ask` 在启动用 `extension_policy != Some("permissive")` 固化（`lib.rs:682`/`788`），`put_config` 不重建门；`CompatEngine` 的 `ExtensionPolicy` 同样启动注入。`get_or_create_loop_agent` 仅在 provider/model/thinking 变化时重建，密钥/端点经 `LlmPort` 读过期 clone。危险方向：进程以 yolo/permissive 启动后改 safe——bash/subagent **继续直放**。本地单用户也有设置页与热保存，不是理论纯度问题。双写冻结、待接线扩展点不在本条。 | 单一可变配置句柄：kernel Port 与 `AppState.config` 共用同一 `Arc<RwLock<AppConfig>>`（或 Port 只读投影且所有写经组合根）。门闩改为每次 `check` 读当前 `extension_policy`（或提供 `reload` 并在 `put_config` 调用）。`put_config` 在 providers/compaction/policy 变更后 `invalidate_loop_agents`。等价形状：废弃 kernel 侧 config 副本，Llm/Skill/Credentials 直接读 `AppState`。回滚 = 恢复 clone。实践参考：[Tokio 共享状态教程](https://tokio.rs/tokio/tutorial/shared-state)（已打开：跨任务共享一份 `Arc<Mutex<_>>`，clone 的是句柄不是数据）；[OWASP Authorization Cheat Sheet — Validate the Permissions on Every Request](https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html)（已打开：每次请求用可全局配置的**当前**授权策略）。 |

无其他候选通过材料性门。

---

## Evolution order and residual risks

1. **立即（正确性 + 策略边界）**：收口 F1——一份配置、门闩读实时档位、`put_config` 失效 agent。
2. **不要做**：删除待接线服务面/LoopHooks/EventBus/enqueue_turn/fork（用户定调；登记表已标明接线方）。
3. **不要在 M3 前做**：双写对账（已冻结；误判风险 > 毫秒窗口收益）。
4. **可选演进**：阶段 4 按模块把组装层域逻辑迁出；TS 轨若需消费服务面再加 hostcall，而不是空等注册表。

**已接受例外**

- 双写：当前真相源 = messages 表；event_log sidecar；崩溃窗口无对账。代码头与架构 §5.1 一致。保留至 M3。
- 扩展点待接线：4 个服务面 + 8 个 LoopHooks + 总线/队列/fork。无正确性/安全实害。
- CSRF：无 Origin/Referer 放行（curl/同源）。浏览器跨站通常带 Origin。OWASP 建议双缺省拒绝，本地 CLI 权衡可接受。
- `workspace` `root` 接受绝对路径：项目切换产品语义；跨源发送已由 origin 中间件挡住。
- `on_tool_pre` 恒 Allow：裁决在 executor，不再是旁路。
- 子代理子进程硬编码 128K 窗口且无压缩：短任务，无材料性。
- `McpService` 不在 `bm-protocol`：生产走 `AppState.mcp`；kernel 注册无 lookup。待接线，不立项。

**盲点**

- 未跑集成测试/未启服务。
- `bm-compat` 大体积上游派生测试面 UNVERIFIED。
- 前端三通道（chat SSE / REST messages / events SSE）在双写冻结下仍是既有事实。
- wiki 功能与 `/api/wiki` 契约完整性按范围排除。

---

## 与 2026-08-16 对比（new / resolved / retained）

对照上轮 C 报告 F1–F14 与交叉报告「已声称修复」项，以**当前可执行代码**判定。

| 上轮项 | 判定 | 当前证据 |
|---|---|---|
| F1 内置工具绕过权限门 | **resolved** | `builtin_gate.rs` + executor `gate.check`（`compat_engine.rs:1128`）；bash/subagent 询问链；单测覆盖记忆命中/拒绝/permissive。`on_tool_pre` 仍 Allow 但是卡片钩子，不再是唯一闸门。 |
| F2 无消费者服务面 / 双路径 | **retained（降级，不定缺陷）** | 登记表：tools/notify/scheduler/credentials 待接线。用户定调：挂点未接线 ≠ 该删。无正确性实害。 |
| F3 Compactor 具体类型 downcast | **resolved** | `kernel.port::<dyn Compactor>`；自定义实现告警并保留自身参数，不再 `unwrap_or_default` 静默回落。 |
| F4 双写未收口 | **retained（已接受冻结）** | `dual_write.rs` + 架构 §5.1 标注 messages 表为当前真相源、冻结至 M3。窗口仍在，但对账任务按决策不做。 |
| F5 EventBus 零接线 / 250ms 轮询 | **retained（待接线）** | `subscribe_events` 仍 250ms；总线无生产 emit。不定 P0/P1。 |
| F6 LoopHooks 多数无实现 | **retained（待接线）** | 12 挂点中生产仍主要是 request/chunk/tool_pre/tool_post。 |
| F7 context_window 死配置 | **resolved** | `compaction.rs` `effective()` 读取 override；`bm_engine.rs` 注入 `LoopConfig`；单测断言 64K override。 |
| F8 fork 零发射者 | **retained（roadmap）** | 机制在、发射者仍无。登记表标 M3。 |
| F9 TS 轨与注册表隔离 | **retained（能力边界）** | hostcall 仍 7 通道，无 registry。登记表未承诺 TS 已互通。 |
| F10 memory/role 注入双实现 | **retained（P3 债务，本轮不过门）** | 算法仍两份；D8 未落地。无正确性实害。 |
| F11 memory 双实例 | **resolved** | 会话经 MemoryPort 取全局 `open_dir` 单例；仅服务缺失回落本地。 |
| F12 enqueue_turn 无生产调用 | **retained（待接线）** | 登记表：M3 inject。 |
| F13 组装层上帝 crate | **retained（演进形态）** | 仍成立；wiki 以独立 crate + 门面接入，方向正确。 |
| F14 轮询无退避 | **retained（并入 F5）** | 未变。 |
| P0-3 CSRF Origin | **resolved** | `origin_middleware` 挂在 router；本机 Origin/Referer 白名单；单测放行/拒绝。 |
| P1-3 档位 → ExtensionPolicy | **resolved（启动时）/部分被 F1 新伤覆盖** | `extension_policy_from_config` 映射 permissive/allow_dangerous；门闩启动读取同一字段。热更新不传播 = 本轮 F1。 |
| P2-1 run_agent_turn | **resolved** | `chat_bm` 与 `run_steward_turn` 共用 `run_agent_turn`。 |
| **本轮 F1 配置三持有** | **new** | 上轮不存在 kernel `shared_config` 与 `AppState.config` 长期分叉（当时 Port 刚铺开、skill 热路径未形成对打）。 |

**上轮 Verdict FAIL 的主因（F1 权限门）已不成立。本轮 FAIL 主因是新的配置/策略所有权分裂（P1）。**

---

*本报告由 ln-24-architecture-auditor 独立产出；证据以可执行接线为准。*
