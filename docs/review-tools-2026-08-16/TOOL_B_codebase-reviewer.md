# 工具B：codebase-reviewer 独立审查报告

> 审查日期：2026-08-16 ｜ 工具：codebase-reviewer skill（Phase 1 Deep Review）｜ 模式：只读
> 独立产出声明：本报告由本审查者独立完成，未与其他审查工具（工具 A/C）通信。

---

## 一、审查概览（范围/方法）

### 范围
- **已审**：`backend/crates/**`（bm-protocol / bm-kernel / bm-storage-turso / bm-server / bm-core / bm-loop / bm-memory / bm-compat / bm-compactor）、`backend/plugins/**`（6 个自研插件）、`backend/tests/event_log/**`、`frontend/src/**`（App/stores/api/components/lib/i18n）。
- **排除**：`backend/vendor/**`、`backend/target`、`frontend/node_modules`、`frontend/dist`、`docs`、`artifacts`、`packaging`、`scripts`、`hooks`、`*.log`。

### 方法
1. **Map**：workspace Cargo.toml（10 个成员 crate）→ 逐 crate 模块清单 → 入口点（`bm-server/main.rs`、`serve_inner`）→ 数据流（chat 请求 → bm-loop 回合 → EventFlusher → TursoEventStore；插件 hostcall → HostThread → HostServices）→ 配置（config.toml + 12 个 `BM_*` env）→ 测试策略（单元 + `tests/architecture.rs` 依赖守卫 + event_log 集成套件 + proptest）。
2. **逐文件分析**：通读约 40 个核心文件（约 1.5 万行），其余（bm-compat 巨型模块、前端组件）抽样 + 定向 grep 验证。
3. **实证验证**：`cargo check -p bm-event-log-tests --tests` 实测编译（发现断裂测试）；grep 验证死代码/无消费者扩展点（`pi_name`、`StepEnd`、`enqueue_turn`、`EventBus`、`extension_policy` 等）。
4. 每条发现均有文件:行号 + 证据；无法完全确认的标注「待验证」。

---

## 二、Codebase Map（简短）

```
frontend (React+TS, zustand, i18n 4 语言)
   │  REST + SSE (fetch 流式)
   ▼
bm-server (axum) ── AppState: config/db/dual_writer/kernel/compat/steward/terminal
   ├─ chat_bm ──► bm-loop ReactLoopAgent (turn/step 回合循环, EventFlusher 真序落日志)
   │               ├─ llm: OpenAiClient (OpenAI 兼容流式)
   │               ├─ executor: QuickJsToolExecutor → bm-compat CompatEngine (专用线程 + 命令通道)
   │               │              └─ QuickJS 沙箱 (PiJsRuntime, 34k 行单文件) ← 插件 TS
   │               └─ kernel: EventLog + bm-compactor 压缩策略
   ├─ compat 权限链: host.rs check_capability → request_approval (SSE 弹窗 → oneshot, fail-closed)
   ├─ routes/*: sessions/plugins/skills/providers/workspace/updates/steward/pdf_omni/terminal/refine/todos
   └─ 后台任务: agent 空闲淘汰 / 孤儿事件清理 / Steward 调度器(10s) / 心跳
bm-kernel: KernelBuilder → Registry(服务面 PortBox) + Loader + EventBus + EventLog
bm-protocol: 纯类型契约 (零运行时依赖): CoreEvent/CustomEvent/Port traits
bm-storage-turso: EventStorePort 的 SQLite 实现 (单写者 Mutex + 应用层 seq 分配 + WAL)
bm-core: config/db/plugins/skills/updates/workspace/compaction/provider 解析
```

**数据流**：`POST /api/chat` → `chat_bm` → `get_or_create_loop_agent`（会话串行锁 + agent 锁）→ `run_turn`：UserMessage/TurnStart 落日志 → 每步投影 `derive_messages` → LLM 流（chunk 经 EventFlusher 攒批 append_batch）→ 工具执行（QuickJS 桥，权限链）→ 软/硬压缩触发 → TurnEnd。前端历史从 SQLite `messages` 表读，实时投影从 event_log 订阅（250ms 轮询 SSE）。

**测试策略**：crate 内单元测试丰富；每 crate 一个 `tests/architecture.rs` 依赖方向守卫（机器强制 L9 纪律，含 Cargo.toml 文本解析 + 源码引用扫描，禁止依赖 bm-server/bm-core）；`bm-event-log-tests` 集成套件（replay 确定性 / dual-write / fork / ignorable / proptest）——**但该套件当前编译失败（见 BUG-001）**；`bm-loop/tests/engine_tests.rs` 有完整回合循环测试。

---

## 三、发现清单（按六维分组）

### ARCH 架构

#### ARCH-001 | 插件权限档位配置是"死配置"：UI 完整、持久化完整、执行层从不读取
**Severity:** High
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:600`；`backend/crates/bm-core/src/config.rs:50-57,326-327`
**Observation:** 前端设置页提供 default/safe/balanced/yolo 四档权限模式（`frontend/src/components/settings/PluginsSettings.tsx:46-77` 写 `extension_policy` + `extension_allow_dangerous` 到 config.toml），后端 `AppConfig` 完整定义并持久化这两个字段——但全库 grep 确认**无任何生产代码读取它们**。插件引擎装配处硬编码 `ExtensionPolicy::default()`（Prompt 模式，default_caps=[read,write,http,events,session]，deny=[exec,env]）。后果：用户选"yolo"不会放开 exec/env，选"safe"也不会收紧——安全设置界面产生虚假的安全感/期望。同时 manifest 里声明的 `capabilities` 数组（如 web-search/extension.json）也从未被解析进 `per_extension`（`ExtensionPolicy.per_extension` 恒空），纯文档。
**Question:** `extension_policy` 字段是否计划接入 `CompatEngine` 装配（经 KernelBuilder 或 init_compat）？在接入前是否应从前端隐藏该设置或标注"未生效"？

#### ARCH-002 | 内核"四件套"之事件总线（EventBus）生产路径零消费者
**Severity:** Medium
**File(s):** `backend/crates/bm-kernel/src/bus.rs:1-40`；`backend/crates/bm-kernel/src/lib.rs:208-215`
**Observation:** `Kernel::emit`/`on`（四种分发模式：观察/waterfall/parallel/serial）除测试外无任何生产调用方（grep 实证）。架构文档宣称事件总线是内核四件套之一；A5 订阅注释（`bm-kernel/src/event_log.rs:345-346`）自述"阶段 1 用轮询…A6 自研 loop 落位后换内核事件总线直推（无轮询延迟）"——即总线是"设计了但未接线"的扩展点。事件流目前靠 250ms 轮询 + 前端 SSE。
**Question:** 总线是否纳入接线排期？若短期不接，建议在 bus.rs 头注标注"未接线"状态，避免读者误以为插件事件流已走总线。

#### ARCH-003 | bm-compat 巨型单文件 + 两套能力评估逻辑并行
**Severity:** Medium
**File(s):** `backend/crates/bm-compat/src/extensions_js.rs`（33,950 行）；`backend/crates/bm-compat/src/host.rs:36-64` vs `extensions_js.rs:78-112`
**Observation:** `extensions_js.rs` 33,950 行单文件（含编译期内嵌 JS 字面量宏、全部宿主桥、大量测试），维护/审阅成本极高。更实质的问题：插件能力裁决存在**两套独立实现**——`host.rs::check_capability`（async 路径，可走询问链）与 `extensions_js.rs::check_exec_capability`（同步 exec 路径，Prompt=deny）——同一语义两份代码，未来权限规则演化时必然漂移。
**Question:** 是否计划将 extensions_js.rs 拆分为 runtime/桥接/策略三模块，并让 exec 裁决复用 `check_capability` 的单一事实源？

#### ARCH-004 | 双存储事实源过渡态：messages 表与 event_log 并行落盘
**Severity:** Medium（过渡态，架构文档已承认）
**File(s):** `backend/crates/bm-server/src/bm_engine.rs:11-12,860`；`backend/crates/bm-server/src/lib.rs:369-389`
**Observation:** 同一会话事实双写：`add_message` 落 SQLite messages 表（前端历史读取源），loop 同时把 UserMessage/AssistantMessage 落 event_log（"唯一事实源"）。两条链的恢复/删除/清理语义不一致（`delete_session` 清 DB 不清日志，孤儿日志靠 90 天定时清理）。这是阶段 0 迁移的刻意中间态，但已有多处"降级路径"（`getmessagesurface` 无日志时回落 messages 表），分支/压缩只对日志链生效——前端历史展示与模型可见历史将随时间产生语义分裂（如压缩后前端仍显示被遮蔽的旧消息）。
**Question:** 前端历史迁移到 event_log 投影的排期？过渡期是否需要"压缩同步折叠 messages 表"的对账机制？

#### ARCH-005 | 服务面（SERVICE_FACES）双轨并行：Port 服务与直调并存，调用方需感知两条路径
**Severity:** Low
**File(s):** `backend/crates/bm-server/src/service_faces.rs:86-124`；`backend/crates/bm-server/src/routes/sessions.rs:28-48`
**Observation:** 每个服务面（settings/stats/llm/skill/session/gate/notify…）都实现"kernel.port 可用走服务、不可用直调 bm-core"的双分支（`session_port()`、`read_settings` 等），调用点代码量翻倍且 kernel 不可用的路径（dual_writer=None）在现实中几乎不可能触发——`serve_inner` 中 kernel 与 dual_writer 同开关，只在事件日志打开失败时同时缺席。两条路径的行为一致性靠测试维持，无机器保障。
**Question:** 是否可以把"kernel 不可用"收敛为启动失败（fail-fast），从而删除全部双分支？或至少为双分支写一致性测试？

---

### SEC 安全

#### SEC-001 | 本地 HTTP API 无 CSRF 防护：任意网页可驱动本地 Agent 执行任意命令/写任意文件
**Severity:** High
**File(s):** `backend/crates/bm-server/src/lib.rs:296-316,138-144`；`backend/crates/bm-server/src/routes/workspace.rs:15-20`
**Observation:** CORS 白名单（`cors_origin_allowed`）只阻止浏览器**读取**跨源响应，无法阻止跨源**发送**请求（表单提交/`fetch` no-cors 的 POST 仍会送达）。`BOENMIND_TOKEN` 默认不设置（桌面壳），`auth_middleware` 形同虚设。因此：用户浏览器打开任意恶意网页 → 该页可 POST `http://127.0.0.1:17321/api/chat` 驱动本地 Agent 跑回合（内置 bash 工具对模型全开、无需询问，见 SEC-002）→ 任意命令执行；或 POST `/api/workspace/file` 携带 `root` 参数（`resolve_root` 接受**任意绝对路径**，不受 safe_join 保护——safe_join 只校验 rel 相对 root，root 本身无校验）→ 任意文件读写；或 POST `/api/plugins/install-source`、`/api/updates/apply`。响应被 CORS 挡住但攻击者不需要响应。
**Question:** 是否对非 GET 请求校验 `Origin`/`Referer` 必须为本机来源，或要求自定义请求头（前端 `authHeaders()` 已存在，可要求 `X-BoenMind: 1` 类头）？建议至少对 workspace 写/chat/updates/plugins 四组端点加 Origin 校验。

#### SEC-002 | 模型工具面（read/write/bash）绕过权限链且无路径圈禁
**Severity:** Medium
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:1046-1058`（QuickJsToolExecutor 直调 BuiltinTools）；`backend/crates/bm-server/src/builtin_tools.rs:200-207`（resolve 对绝对路径原样放行）
**Observation:** bm 引擎的模型工具面（内置 7 工具 schema 全注册进 `build_loop_agent`）执行时经 `QuickJsToolExecutor` 直连 `BuiltinTools`，**不经过** `HostServices`/`check_capability` 权限链——与插件 `pi.tool()` 走询问链不同，模型侧 bash 无条件放行（设计如此："内置工具集 schema 进模型可见面全开"）。叠加提示词注入面（网页内容、文件内容进上下文），本地任意文件读写 + 命令执行无护栏。`BuiltinTools::resolve` 对绝对路径不做 cwd 圈禁（注释自认"不复制 cwd 圈禁"）。
**Question:** 该设计是否有意保持"本地个人助手全权模式"？若是，建议在系统提示与文档中显式声明风险面；若否，建议至少提供 `cwd` 圈禁开关或对 `bash` 加执行确认链（复用 PermissionBridge）。

#### SEC-003 | 插件默认放行 http 能力：静默内网探测/数据外传（SSRF 面）
**Severity:** Medium
**File(s):** `backend/crates/bm-compat/src/extensions.rs:1083-1092`（default_caps 含 "http"）；`backend/crates/bm-server/src/compat_engine.rs:247-325`（http hostcall 无 URL 限制）
**Observation:** `ExtensionPolicy::default()` 的 `default_caps` 包含 `http`——Prompt 模式下"allowed"命中即放行，**不触发询问**。任意（含 npm/git 装来的第三方）插件可静默访问 `http://169.254.169.254/`（云 metadata）、内网服务，并经 http 回传。叠加 ARCH-001（per-extension 覆盖未实现），用户无任何手段单独收紧某个插件的网络能力。
**Question:** `http` 是否应移出 default_caps（改每次询问或按域名白名单）？manifest `capabilities` 何时真正驱动 per-extension 策略？

#### SEC-004 | 子代理子进程模式判定依赖环境变量，本地进程可冒充触发
**Severity:** Low
**File(s):** `backend/crates/bm-server/src/subagent_child.rs:42-50`
**Observation:** `should_enter_child_mode` 在设置 `PI_SUBAGENT_PARENT_PID` 后**仅凭该 env 即进入 child 模式**（`(has_json || has_parent) && (has_no_session || has_parent)` 中 `has_parent` 单边满足）。任何能在用户账户下启动 bm-server 二进制的进程/脚本可借 child 模式消耗其 API 额度或读取配置。本地威胁模型下风险有限。
**Question:** 是否要求 `--mode json` 与 env 同时满足，或对 parent pid 做存活校验？

---

### PERF 性能

#### PERF-001 | 每步全量重放投影 + 全 payload 双序列化 + sha256：长会话 O(n²)
**Severity:** Medium
**File(s):** `backend/crates/bm-loop/src/engine.rs:344-394,716-726`
**Observation:** 步循环内每次迭代：`flusher.flush()`（屏障）→ `derive_messages`（整分支全量重放 + 逐段重建投影）→ `estimate_context`（对全部消息逐条 `estimate_tokens`）→ 随后 `prompt_hash_of_parts` 对 system+tools+payload 做**两次**完整 `serde_json::to_string` + sha256。128K 窗口 ~数百条消息、几十步的回合，纯本地 CPU 成本可观（每步全量投影 + 双 JSON 序列化）。
**Question:** 是否考虑投影增量缓存（仅在 flush 后重放新增事件段）？`prompt_hash` 的 payload 序列化可否复用 `build_payload` 的同一份 JSON 缓冲？

#### PERF-002 | ctx-search 每次检索全量读盘 + 全量 JSON 解析索引
**Severity:** Low
**File(s):** `backend/plugins/ctx-compactor/index.ts:290-334`
**Observation:** `searchIndex` 对项目桶内全部 `.jsonl` 文件逐行 `readFileSync` + `JSON.parse`，每次检索 O(索引总量)。8MB 轮转上限下单桶可达数万条，模型高频检索时（修剪占位符会引导模型 ctx_search）延迟显著。
**Question:** 是否在内存维护条目索引（启动/落盘时增量构建），或按 key 前缀分片文件？

#### PERF-003 | roles.json 每次模型请求（每步）读盘解析
**Severity:** Low
**File(s):** `backend/crates/bm-server/src/roles.rs:35-37,60-62`
**Observation:** `RoleInjector::inject` 在 `on_request` 每步调用，每次都 `std::fs::read_to_string` + 全量 JSON 解析（即使角色未激活）。与 PERF-001 叠加放大每步固定成本。
**Question:** 是否按文件 mtime 做短缓存（如 1s）？

#### PERF-004 | 事件流订阅 250ms 轮询 + 每会话常驻 watchdog 任务
**Severity:** Low
**File(s):** `backend/crates/bm-kernel/src/event_log.rs:374-398`；`backend/crates/bm-server/src/routes/sessions.rs:190-205`
**Observation:** 每个打开的会话订阅 = 每 250ms 一次 SQLite 查询 + 每 1s 一次 watchdog tick，多会话/多标签页叠加为持续 DB 轮询（订阅无去重：同一会话两个前端标签页开两条订阅）。单用户规模可接受，但总线直推（ARCH-002）应作为演进方向。
**Question:** 是否对同会话订阅做引用计数合并，或限制订阅数？

---

### QUAL 代码质量（含（精简）/（复用）标注）

#### QUAL-001 | （精简）死契约变体：StepEnd / SessionEndSeed / HeaderReason::Resume 无任何写者
**Severity:** Medium
**File(s):** `backend/crates/bm-protocol/src/event.rs:141,192,81`
**Observation:** grep 全库：`CoreEvent::StepEnd`、`CoreEvent::SessionEndSeed`、`HeaderReason::Resume` 仅在 event.rs 自身定义与 `core_type_name` 映射中出现，loop 从不产生（StepStart 有、StepEnd 无；SessionEndSeed 无任何写者；Resume 无任何构造点）。契约层死变体让读代码者误以为协议支持这些语义。
**Question:** 删除，或补上写者（如 SessionEndSeed 在 `clear_session`/删除会话时落盘）？

#### QUAL-002 | （精简）ProviderKind::pi_name() 24 路映射仅测试引用
**Severity:** Medium
**File(s):** `backend/crates/bm-core/src/config.rs:154-181`
**Observation:** `pi_name` 注释自认"pi 引擎已废除…仅为既有数据兼容而保留"，但生产代码零调用（唯一消费方是测试断言）。24 个 provider 的映射表纯死代码，且随 ProviderKind 新增变体（Xiaomi/Antling/Baseten 等）持续膨胀。
**Question:** 删除并移除测试，还是存在未审到的子代理/插件按名解析路径？（建议再 grep `pi_name` 确认）

#### QUAL-003 | （精简）ReactLoopAgent 的入队 API 仅测试使用
**Severity:** Medium
**File(s):** `backend/crates/bm-loop/src/engine.rs:197-204,254-265`
**Observation:** `enqueue_turn`/`pending_turns`/`run` 生产路径零调用（bm_engine 直接 `run_turn`）。头注自述"M2 起 step 级队列已删"，但 next-turn 队列 API 保留且无消费者（含 `turn_queue` 字段与 `current_position`）。
**Question:** 删除队列 API 与字段，或声明为未来多回合源预留？

#### QUAL-004 | （精简）declare_event! 宏零使用
**Severity:** Low
**File(s):** `backend/crates/bm-protocol/src/event.rs:266-311`
**Observation:** 插件域强类型事件宏（生成结构体 + CustomEvent 转换）仅测试引用；实际插件事件（todo/memory 等）全部走 `CoreEvent` 变体，CustomEvent 无生产写者。宏的 `EVENT_TYPE` 命名（`stringify!($plugin).stringify!($name)` → "WikiPlugin.WikiIndexed"）与文档宣称的"app.wiki.indexed"风格也不一致。
**Question:** 保留待插件生态使用，还是调整命名约定（小写命名空间）？

#### QUAL-005 | （复用）前端三处 SSE 解析循环几乎相同
**Severity:** Medium
**File(s):** `frontend/src/api/client.ts:635-657,702-722,746-766`
**Observation:** `chat` / `subscribeEvents` / `subscribeTerminal` 各自实现同一套"fetch 流 → TextDecoder → buffer 按 \n\n 切分 → data: 解析"逻辑（约 20 行 × 3），后续 SSE 事件类型扩展将三处同步修改。
**Question:** 抽取 `streamSSE(url, headers, onData, signal)` 公共 helper（三个调用点统一）。

#### QUAL-006 | 权限策略存在两套独立裁决实现（与 ARCH-003 同源）
**Severity:** Medium
**File(s):** `backend/crates/bm-compat/src/host.rs:36-64` vs `backend/crates/bm-compat/src/extensions_js.rs:78-112`
**Observation:** `check_capability`（含询问链路由）与 `check_exec_capability`（同步路径，Prompt=deny 且不询问）对同一 capability 语义给出不同决策路径；`is_env_var_allowed` 又独立用 `SecretBrokerPolicy::default()`。三处安全语义分散。
**Question:** 收敛为单一 `check_capability` + 显式 `sync: bool` 参数。

#### QUAL-007 | bm-storage-turso 描述与事实不符（checkpoint 模块已删）
**Severity:** Low
**File(s):** `backend/crates/bm-storage-turso/Cargo.toml:5`
**Observation:** description 仍写"+ checkpoint 策略"，但 `src/` 已无 checkpoint 模块（配套 BUG-001：测试仍引用已删 API）。
**Question:** 更新描述；处理 BUG-001 时一并决定 checkpoint 策略的去留。

#### QUAL-008 | 代码注释密度极高但存在"注释承诺 ≠ 实现"的残留（如权限记忆格式兼容）
**Severity:** Low
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:17-19` 等
**Observation:** 全库注释质量高（多数注释与实现一致），但发现数处注释引用已删除组件（"与 legacy 同款模型"、"pi 上游"反复出现）——pi 引擎已于 2026-08-15 删除，读代码者需反复核对"legacy"指什么。建议在 bm-compat 模块头统一说明 legacy 参照基线版本号（`pi_agent_rust@44ddf80` 已在多处注明，尚可）。
**Question:** 无阻塞；建议后续清理对已删组件的引用措辞。

---

### BUG 缺陷

#### BUG-001 | event_log 集成测试套件整体编译失败（已实证）
**Severity:** High
**File(s):** `backend/tests/event_log/tests/checkpoint_recovery.rs:8`
**Observation:** `cargo check -p bm-event-log-tests --tests` 实测：`error[E0432]: unresolved imports bm_storage_turso::CheckpointState, CheckpointStore`。checkpoint 模块已从 bm-storage-turso 删除（`src/` 下无此模块，Cargo.toml 描述残留"checkpoint 策略"），但测试仍引用。该 crate 编译失败意味着 **checkpoint_recovery / dual_write_30rounds / fork_branch / ignorable_guard / orphan_purge / proptest_append_replay / replay_determinism 全部 7 个集成测试套件当前无法运行**——事件日志最核心的可靠性验证处于断裂状态。若 CI 只跑单 crate 测试（`cargo test -p bm-kernel` 等），此断裂会被静默放过。
**Question:** 修复方向二选一：(a) 恢复 CheckpointStore/CheckpointState 实现（Cargo.toml 描述暗示本有计划）；(b) 删除该测试文件并更新描述。当前状态必须处理——事件日志的确定性/回放/双写验证是"日志是唯一事实源"承诺的根基。

#### BUG-002 | 工具执行中取消 → ToolResult 缺失，日志留下永久未闭合调用
**Severity:** Medium
**File(s):** `backend/crates/bm-loop/src/engine.rs:604-615,626-635`
**Observation:** 工具执行阶段 `tokio::select!` 命中 `cancel.changed()` 时直接 break，此时**工具可能已被执行**但 ToolResult 未落日志；而 ToolCall 事件已在前一步落盘（engine.rs:575-584）。后果：日志中该 (turn,step) 的 tool/call 无对应 tool/result；投影侧 `attach_tool_result` 找不到 call_id → 该工具调用永远"未闭合"；`projection_to_openai_messages` 会过滤未闭合调用（engine.rs:975-979），模型输入面自洽，但审计面残缺，且 `SurfaceToolCall.result=None` 状态将伴随该会话永久存在（除非被压缩遮蔽）。
**Question:** 取消窗口内已开始执行的工具，是否应在其返回后补落 ToolResult（标注 cancelled）再退出？

#### BUG-003 | ctx-compactor 索引轮转在任意字节位置截断 JSONL
**Severity:** Low
**File(s):** `backend/plugins/ctx-compactor/index.ts:181-186`
**Observation:** `kept = kept.slice(kept.length / 2)` 在非行边界截断，截断处 JSONL 行损坏；下次读取时该行被 `validLines` 过滤丢弃（自愈但丢一条记录），且截断后拼接新条目可能产生半行。同理 `appendEntry` 的全量读-改-写窗口（模型工具串行执行下无并发，低风险）。
**Question:** 改为按 `\n` 边界截断（如 `kept.indexOf('\n', kept.length/2)`）。

#### BUG-004 | 权限询问弹窗与 SSE 通道生命周期：prompt 结束即丢询问
**Severity:** Low（fail-closed 已兜底）
**File(s):** `backend/crates/bm-server/src/chat.rs:29-44`；`backend/crates/bm-server/src/compat_engine.rs:162-170`
**Observation:** `send_permission_request` 经 `session_streams` 当前 prompt 通道推送；若询问发生在 prompt 收尾窗口（通道已移除），事件丢失、用户无弹窗、60s 后 fail-closed 拒绝——插件工具在回合末被静默拒绝且模型看不到原因（ToolResult ok=false，模型通常无法自解释）。权限桥注释自认"事件丢失（询问仍会超时 fail-closed，无泄漏）"，但**模型侧的体验**是"工具莫名失败"。
**Question:** 是否在拒绝时给 ToolResult 附带"权限询问超时（无用户响应）"的明确文案，帮助模型收敛重试策略？

#### BUG-005 | init_compat 权限存储打开失败时的 tempdir 泄漏
**Severity:** Low
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:878-894`
**Observation:** `tempfile::tempdir().map(|d| d.keep())` 在每次启动失败路径都会永久遗留一个临时目录（`keep()` 后不再清理），且回退路径的决策记忆写进系统 temp 目录——权限决策"记忆"落在一个重启即丢、且与其他进程共享的位置。路径罕见（仅权限文件损坏时），但语义混乱。
**Question:** 损坏时应 fail-closed 用内存态（不落盘），还是明确落 `app_dir` 的备份文件？

---

### IMP 改进

#### IMP-001 | 权限档位真实接入（承接 ARCH-001）：让 UI 设置生效
**Severity:** Medium
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:597-601`
**Observation:** 装配点已具备全部输入（`AppConfig.extension_policy`/`extension_allow_dangerous`），只需把档位映射到 `ExtensionPolicy`（safe→Strict、balanced→Prompt+收窄 default_caps、permissive→Permissive、yolo→Permissive+exec/env 放行），并从 manifest `capabilities` 构建 `per_extension`。成本低、收益直接（安全设置页从"装饰"变"真实"）。
**Question:** 排期？

#### IMP-002 | CSRF 防护（承接 SEC-001）：Origin 校验 + 状态变更端点审计
**Severity:** Medium
**File(s):** `backend/crates/bm-server/src/lib.rs:198`
**Observation:** 在 auth_middleware 同层加 `origin_middleware`：非 GET 请求校验 Origin 在本机白名单内（复用 `cors_origin_allowed` 的 host 判定），否则 403。对 curl/CLI 场景可加 `X-BoenMind-Client: cli` 头豁免。
**Question:** 是否可行？注意保留无 Origin 头请求（同源/非浏览器）的兼容。

#### IMP-003 | EventBus 接线替代轮询订阅（承接 ARCH-002）
**Severity:** Low
**File(s):** `backend/crates/bm-kernel/src/event_log.rs:339-400`
**Observation:** `subscribe_events` 的轮询 tail 有 250ms 延迟与常驻 DB 查询；内核总线已就绪。可在 EventLog append 成功后 `bus.emit`，订阅改走总线（保留 replay-prefix）。
**Question:** 排期？

#### IMP-004 | 增量投影缓存（承接 PERF-001）
**Severity:** Low
**File(s):** `backend/crates/bm-loop/src/engine.rs:344-352`
**Observation:** 会话内投影可增量：EventLog 记录每分支已折叠到的 seq，`derive_messages` 增量折叠 + 缓存 `SurfaceMessage` 列表（压缩 Replace 时失效）。loop 是唯一写者（串行锁），缓存一致性易保证。
**Question:** 是否在压缩事务落地后随 bm-loop 一并优化？

#### IMP-005 | 测试缺口：权限链与 SSE 契约无端到端测试
**Severity:** Low
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:1199-1270`（单元级已有）等
**Observation:** 权限询问链（SSE 推 → 前端回传 → oneshot 决策 → 记忆回写）只有单元级测试；`chat_bm` 的 SSE 事件形状、`respond_permission` 路由、`stop_chat` 取消语义无集成测试。前端 SSE 解析三处复制（QUAL-005）也缺乏契约测试。
**Question:** 是否在 bm-server 增加 axum 路由级集成测试（tower::ServiceExt oneshot）覆盖 chat/stop/permission-response？

---

## 四、亮点（做得好的，具体）

1. **依赖方向机器守卫**：每个 crate 的 `tests/architecture.rs` 同时解析 Cargo.toml（含 target 限定/多行表）与扫描源码引用（`use`/全限定路径/隐藏引用），违反即测试失败——"内核不依赖应用层"的铁律有机器背书（`bm-kernel/tests/architecture.rs:1-50`）。
2. **事件日志正确性工程**：应用层 seq 分配 + 单写者 Mutex + 事务内 INSERT/head 更新 + `repair_heads` 启动自愈 + `recover_interrupted_turns`（A4）+ ignorable 未知事件守卫 + A7 迁移链骨架 + `check_replace_interval` 遮蔽校验——每一层都有测试锚定（`bm-storage-turso/src/event_log.rs:95-98,110-122`）。
3. **压缩事务协议与策略分离**：三事件事务（Start→Summary+Replace→End）、摘要失败 fail-safe 不遮蔽、`strip_think_blocks` 处理推理污染——实测经验（P1/P2 回看）被系统化沉淀为代码与测试（`bm-loop/src/compact.rs:96-107,178-190`）。
4. **安全默认值意识**：权限询问 fail-closed、60s 超时、决策记忆持久化（atomic tmp+rename）、`extension-permissions.json` 与上游格式兼容且损坏时拒绝静默清空；自更新走 minisign/ed25519 验签 + 资产 URL 白名单（`bm-core/src/updates.rs:242-296`）。
5. **工作区路径防护扎实**：`safe_join` 拒绝 `..` 段 + canonicalize 后 `starts_with` 校验（符号链接已解析）；`write_file` 对"目标不存在"场景改为父目录校验（`bm-core/src/workspace.rs:30-47,119-144`）。
6. **测试纪律**：`bm-loop/tests/engine_tests.rs`（1008 行）、engine.rs 内嵌流式钩子测试、权限桥 fail-closed/记忆命中测试、`proptest_append_replay` 属性测试；前端 i18n 4 语言 461 keys 结构同构（`typeof` 约束）。
7. **代码自文档化**：模块头注释准确描述设计意图、实测教训（P0-P4）与决策背景，代码审查者可快速进入上下文。

---

## 五、结论

BoenMind 后端是我近期见过的最"工程化自觉"的个人项目：事件日志的语义分层（protocol 契约 / kernel 语义 / turso 实现）、依赖方向守卫、压缩事务协议、权限 fail-closed 默认值，均达到生产级思考水准；前端 i18n/类型纪律（零 `any`）与组件复用（ChatPane/DockLayout 宿主化）也相当干净。

核心问题集中在**"中间态架构的完成度"**：① 权限档位配置是完整 UI + 零后端接入的死功能（ARCH-001，连带 SEC-003 的 http 静默放行）；② 事件总线、队列 API、declare_event 宏等"预留扩展点"无消费者（ARCH-002/QUAL-001~004）；③ 最刺眼的是一条已实证的断裂：**event_log 集成测试套件整体编译失败**（BUG-001）——"日志是唯一事实源"的验证根基当前不存在，建议最先修复。其次是本地 API 的 CSRF 面（SEC-001，含任意 root 路径写文件）与模型工具全开无护栏的组合风险，值得在下一个安全轮处理。

总体评估：架构方向正确、实现细节扎实，处于"骨架已立、接线与收尾未完成"的过渡期；本报告 26 条发现中，2 条 High（ARCH-001 功能失效 + BUG-001 测试断裂）、1 条 High 安全（SEC-001），建议按 BUG-001 → SEC-001 → ARCH-001 的顺序处理。
