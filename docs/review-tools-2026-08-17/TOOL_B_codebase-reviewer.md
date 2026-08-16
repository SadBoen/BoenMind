# Tool B — codebase-reviewer Phase 1（2026-08-17）

> 审查日期：2026-08-17 ｜ 工具：codebase-reviewer skill（Phase 1 Deep Review）｜ 模式：只读、不改代码
> 独立产出声明：本报告由本审查者独立完成，未与其他审查工具（工具 A/C）通信。
> 范围硬排除：`backend/crates/bm-wiki/**`、前端 wiki 视图/组件/i18n wiki 段/api wiki、vendor/target/node_modules/dist。允许检查 bm-server **组装层**对 wiki 的污染。
> 上轮（2026-08-16）已修项：不作为新问题复述，除非本次 cargo/读代码证明仍在。

---

## 代码地图（入口/鉴权链/数据流/测试策略）

```
frontend (React 19 + TS + zustand + i18n)
   │  REST + 统一 readSSEStream（chat / events / terminal）
   ▼
bm-server (axum) ── AppState
   入口：
     - backend/crates/bm-server/src/main.rs  → serve / --mcp-serve / subagent child
     - Tauri 壳共用 bm_server::serve（默认 127.0.0.1:17321）
   鉴权链（外→内）：
     CorsLayer(本机 Origin 谓词)
       → auth_middleware（仅当 BOENMIND_TOKEN 非空：Bearer 精确相等）
         → origin_middleware（非 GET/HEAD/OPTIONS：Origin 或 Referer 必须本机）
           → routes
   数据流：
     POST /api/chat
       → chat.rs 校验会话 + add_message(user) 写 messages 表
       → bm_engine::chat_bm
       → get_or_create_loop_agent（会话串行锁）
       → run_agent_turn → ReactLoopAgent::run_turn
            UserMessage/TurnStart → LLM 流 → ToolCall/ToolResult → 软/硬压缩 → TurnEnd
       工具分派 QuickJsToolExecutor：
            BuiltinGate(bash/subagent) → BuiltinTools
            McpGate(mcp__) → McpService
            wiki_* 前缀 → wiki_tools（组装层污染，见 ARCH-005）
            其余 → CompatEngine / QuickJS
   配置：
     ~/.boenmind/config.toml + BM_* / BOENMIND_* env
     AppState.config（tokio RwLock，PUT /api/config 更新）
     另有启动时 clone 的 kernel shared_config（std RwLock）——与前者不同步（BUG-001）
   测试策略：
     各 crate 单元测试 + tests/architecture.rs 依赖方向守卫
     backend/tests/event_log（bm-event-log-tests）集成：replay / dual-write / fork / orphan / proptest
     bm-loop/tests/engine_tests.rs 回合循环
     前端：tsc -b 为构建闸门，无独立 e2e 套件在本次范围
```

**工作区 crate（排除 wiki 功能审查）**：bm-protocol / bm-kernel / bm-storage-turso / bm-server / bm-core / bm-loop / bm-memory / bm-compat / bm-compactor / bm-mcp。

---

## 实证记录（你跑了什么命令、退出码、关键输出）

| # | 命令 | 退出 | 关键输出 |
|---|------|------|----------|
| 1 | `cd D:/96_CoderWorld/BoenMind/backend && cargo check -p bm-server --offline` | **0** | `Finished dev profile ... in 4.62s`。仅 vendor `asupersync` unreachable-code warning，**非本仓库代码**。 |
| 2 | `cargo check -p bm-kernel -p bm-loop -p bm-compat -p bm-core --offline` | **0** | `Finished ... in 2m 56s`。同上 vendor warning。 |
| 3 | `cd D:/96_CoderWorld/BoenMind/frontend && pnpm exec tsc -b --pretty false` | **0** | 无输出（类型检查通过）。 |
| 4 | `cargo check -p bm-event-log-tests --tests --offline` | **0** | `Finished ... in 2.91s`。上轮 BUG-001（CheckpointStore 断裂）**已不复现**。 |

安全敏感点均已打开实现（非仅 grep）：

- CSRF：`origin_middleware` + `cors_origin_allowed` / `referer_allowed`（`bm-server/src/lib.rs:264-457`）
- `BOENMIND_TOKEN`：`auth_middleware`（`lib.rs:384-404`）；前端 `client.ts:453-473` + `App.tsx` TokenGate
- `safe_join`：`bm-core/src/workspace.rs:30-47`（拒 `..` + canonicalize + starts_with）
- `resolve_root`：`routes/workspace.rs:15-20`（**任意绝对路径原样采用**）
- Origin 白名单：localhost / 127.0.0.1 / [::1] / tauri.localhost，**任意端口**
- exec/bash：`builtin_gate.rs` 对 bash/subagent 询问；`BuiltinTools::resolve` 仍不圈禁绝对路径
- SSRF：`bm-core/src/providers.rs:97-133`、`bm-mcp/src/discover.rs:200-230` 仅校验 **字面 host**；插件 `pi.http`（`compat_engine.rs:283-363`）**无 URL 校验**
- `api_key`：`GET /api/config` 整份 `AppConfig` 回传（含明文 key）
- permission：`extension_policy_from_config` 已接线；档位语义坍缩 + 运行时不热更新

上轮已修且本次证实仍有效（不当新问题）：

- event_log 集成测试可编译
- BuiltinGate 已挂到 `QuickJsToolExecutor`
- CSRF Origin/Referer 中间件已存在
- 双写范围冻结注释已写明（`dual_write.rs:11-18`）
- `extension_policy` 已从死配置变为启动期映射
- `run_agent_turn` 统一 chat/steward
- memory 经 kernel port 单例（缺失时仍回落每会话实例）
- Compactor 以 `Arc<dyn Compactor>` 注册
- `context_window` 从 compaction override 读取
- 前端 tsc 绿、SSE 三处合一、`usePolling` 存在、权限档位 UI 映射存在

---

## Architecture（ARCH-xxx）

### ARCH-001: Kernel 服务面持有启动期 config 快照，与 AppState.config 永久分叉
**File(s):** `backend/crates/bm-server/src/lib.rs:575-635`；`backend/crates/bm-server/src/routes/config.rs:50`；`backend/crates/bm-server/src/bm_engine.rs:318-339,567-575`
**Severity:** High
**Observation:** `serve_inner` 用 `config.clone()` 建 `shared_config`，交给 `LlmPortImpl` / `CredentialsPortImpl` / `SkillPortImpl` / `ProviderPortImpl`。`PUT /api/config` 只写 `state.config`，**从不回写** `shared_config`。`build_loop_agent` 优先走 `kernel.port::<LlmPort>("llm")`，因此新 agent 的 `api_key`/`base_url` 也来自启动快照。`get_or_create_loop_agent` 仅比较 `provider_id/model/thinking`，密钥轮换不会重建 agent。设置页“保存即生效”对 LLM 凭证/厂商列表在服务面路径上不成立。
**Question:** 是否让这些 Port 持有与 `AppState.config` 同一把锁（或 `put_config` 同步 clone + `invalidate_loop_agents`）？当前双源是否为过渡期有意为之？

### ARCH-002: EventBus 仍是“四件套”中未接线的生产死件
**File(s):** `backend/crates/bm-kernel/src/bus.rs`；`backend/crates/bm-kernel/src/lib.rs:209-214`；`backend/crates/bm-kernel/src/event_log.rs:339-346`
**Severity:** Medium
**Observation:** `Kernel::emit`/`on` 生产路径仍无调用方（仅测试 + loader 测试）。`subscribe_events` 注释仍写“阶段 1 用轮询…A6 换内核事件总线直推”。上轮 ARCH-002 未变。
**Question:** EventBus 是否排入接线？短期不接是否在模块头标注“未接线”，避免读者以为插件事件已走总线？

### ARCH-003: 插件能力裁决仍是两套独立实现
**File(s):** `backend/crates/bm-compat/src/host.rs:36-64`；`backend/crates/bm-compat/src/extensions_js.rs:83-119`
**Severity:** Medium
**Observation:** `check_capability`（async，Prompt 可询问）与 `check_exec_capability`（同步，Prompt=deny）并行。`is_env_var_allowed` 再独立走 `SecretBrokerPolicy::default()`。权限规则演化必漂移。上轮 QUAL-006 仍在。
**Question:** 是否收敛为单一 `check_capability` + 显式 `sync`/`can_prompt` 参数？

### ARCH-004: 双事实源仍分裂；冻结已文档化但读路径未闭环
**File(s):** `backend/crates/bm-storage-turso/src/dual_write.rs:11-18`；`backend/crates/bm-server/src/chat.rs:114-120`；`backend/crates/bm-server/src/bm_engine.rs:12,997`
**Severity:** Medium
**Observation:** 注释已诚实写明“当前真相源 = messages 表；event_log 为 sidecar；双写范围冻结至 M3”。前端历史仍读 DB；压缩/fork 只作用于日志。过渡态被承认，语义分裂风险仍在（压缩后 UI 仍显示被遮蔽旧消息）。
**Question:** M3 收口排期？过渡期是否至少在压缩成功后同步折叠 messages，或 UI 改读 event_log 投影？

### ARCH-005: wiki 场景工具侵入通用组装/分派中枢（允许检查的组装层污染）
**File(s):** `backend/crates/bm-server/src/lib.rs:33-35,226`；`backend/crates/bm-server/src/routes/mod.rs:19`；`backend/crates/bm-server/src/bm_engine.rs:366-380`；`backend/crates/bm-server/src/compat_engine.rs:1215-1230`
**Severity:** Medium
**Observation:** 本次不审 wiki 功能本身。但通用 crate `bm-server` 在 **lib 模块、router merge、场景 match、执行中枢 `wiki_` 前缀** 四处硬编码 wiki。新场景（第二个 APP）将继续膨胀 `compat_engine` 分派与 `build_loop_agent` 的 match。这与“场景工具按 session.app 组装、系统增强插件全局生效”的注释意图相反——场景实现泄漏进通用内核路径。
**Question:** 场景工具是否应收口为注册表（app → definitions/execute），使 bm-server 核心不再出现 `wiki_` 字面量？

### ARCH-006: 权限档位 UI 四档、后端启动期两态，且不热更新
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:1003-1027`；`backend/crates/bm-server/src/lib.rs:673-683,788`；`frontend/src/stores/app-store.ts:395-418`；`backend/crates/bm-core/src/config.rs:100-106`
**Severity:** High（完成度/安全预期）
**Observation:** 上轮 ARCH-001“死配置”已修：启动时 `extension_policy_from_config` 会读档位。但：(1) `safe`/`balanced`/`default` 全部落到 Prompt，注释自称“四档”实际只有 permissive vs 其他；(2) `yolo` = permissive + `extension_allow_dangerous` 去掉 exec/env deny；(3) `BuiltinGate.ask_high_risk` 与 `McpGate` 在启动时用 `!= Some("permissive")` 固化，`put_config` 不重建门；(4) CompatEngine 的 `ExtensionPolicy` 只在 `init_compat` 注入一次。用户从 yolo 切回 safe **直到重启都不会收紧 bash/MCP/插件 http**。
**Question:** 档位是否应成为运行时单一事实源（gate 读 `state.config`，或 `put_config` 重建 policy/gate 并 invalidate agents）？`safe` 与 `balanced` 是否应有可观察差异（例如 safe 把 `http` 移出 default_caps）？

### ARCH-007: SERVICE_FACES 双轨仍在，kernel 不可用几乎只是理论分支
**File(s):** `backend/crates/bm-server/src/service_faces.rs:84-124`；`backend/crates/bm-server/src/bm_engine.rs:310-339`
**Severity:** Low
**Observation:** 每个 Port 调用点仍是“kernel.port 成功走服务，否则直调 bm-core”。`serve_inner` 里 kernel 与 dual_writer 同开关；日志打开失败则引擎直接 503（`build_loop_agent` 无 kernel 即失败）。双分支维护成本高、一致性无机器保障。上轮 ARCH-005 残留。
**Question:** 是否 fail-fast（无 kernel 不启动 HTTP）并删除退化分支？

---

## Security（SEC-xxx）

### SEC-001: Origin 白名单接受任意本机端口，CSRF 修复可被同机网页绕过
**File(s):** `backend/crates/bm-server/src/lib.rs:264-289,406-427`
**Severity:** High
**Observation:** `origin_middleware` 已挡住 `evil.example.com`（上轮 SEC-001 主体已修）。但 `cors_origin_allowed` 对 `http://localhost:*` / `127.0.0.1:*` **不限端口**。任意本机网页（恶意 npm 包的 dev server、另一本地工具、`python -m http.server`）Origin=`http://localhost:9999` 可通过 CSRF 检查，进而 `POST /api/chat`、`POST /api/workspace/file`、`POST /api/terminal`、`POST /api/plugins/install-source`。桌面默认不设 `BOENMIND_TOKEN`。这是“只拦远程网页、不拦本机任意源”的洞。
**Question:** 是否把允许 Origin 收紧到前端实际端口（5173 / 17321 / tauri）+ 自定义头（如 `X-BoenMind-Client`）？或对状态变更强制 Bearer，即使桌面也生成一次性本地 token？

### SEC-002: 插件 `pi.http` 默认静默放行且无任何 URL 限制（SSRF / 数据外传）
**File(s):** `backend/crates/bm-compat/src/extensions.rs:1079-1091`；`backend/crates/bm-server/src/compat_engine.rs:283-325`；`backend/crates/bm-compat/src/host.rs:45-61`
**Severity:** High
**Observation:** `ExtensionPolicy::default()` 的 `default_caps` 含 `"http"`。Prompt 模式下 default_caps 命中 = **Allow，不询问**。`BridgeServices::http` 用用户/插件提供的 URL 直接 `reqwest` GET/POST，无 scheme/host/私网/重定向限制，无响应体上限。第三方插件或提示词注入即可打 `169.254.169.254`、内网、外传工作区内容。提供商/MCP 发现路径有 SSRF 校验，**插件 HTTP 宿主桥没有**。上轮 SEC-003 主体仍在。
**Question:** `http` 是否移出 default_caps（每次询问或域名白名单）？hostcall 是否复用 `validate_base_url`（并补 DNS 解析，见 SEC-004）？

### SEC-003: 内置 read/write/edit 仍接受绝对路径，不经权限门、无 cwd 圈禁
**File(s):** `backend/crates/bm-server/src/builtin_tools.rs:198-207,212-256`；`backend/crates/bm-server/src/builtin_gate.rs:18-51`；`backend/crates/bm-server/src/compat_engine.rs:1126-1233`
**Severity:** High
**Observation:** BuiltinGate 只拦截 `bash`/`subagent`（上轮 SEC-002 已部分修）。`read`/`write`/`edit`/`grep`/`find`/`ls` 直放。`resolve()` 对绝对路径原样放行，注释写明“不复制 cwd 圈禁”。模型（或插件经 `pi.tool("read")`）可读写 `C:\Users\...\ .ssh`、config.toml（含 api_key）。插件侧 `pi.tool("read")` 走 host `check_capability("read")`，而 read 在 default_caps 里同样静默允许。
**Question:** 是否对内置文件工具强制 `safe_join(cwd)`，绝对路径一律拒绝或升为高权限询问？这是否仍是“本地个人助手全权”的有意设计？若是，系统提示/设置页是否应显式披露？

### SEC-004: 提供商/MCP SSRF 只检查字面 IP，DNS 到私网可绕过
**File(s):** `backend/crates/bm-core/src/providers.rs:97-133`；`backend/crates/bm-mcp/src/discover.rs:200-230`
**Severity:** Medium
**Observation:** `validate_base_url` / `validate_http_url`：localhost/回环放行；字面私网/链路本地拒绝；**主机名为非 localhost 的域名直接 Ok**。`http://metadata.google.internal/`、用户控制的 DNS 指到 `169.254.169.254` 或 `192.168.x.x` 可通过校验。测试只覆盖字面 IP（`providers.rs:467-489`）。
**Question:** 是否在连接前解析 DNS 并对解析结果再跑一遍私网检查（并禁止跟随到私网的 HTTP 重定向）？

### SEC-005: `GET /api/config` 明文返回全部 api_key
**File(s):** `backend/crates/bm-server/src/routes/config.rs:8-10`；`backend/crates/bm-core/src/config.rs:153-168`
**Severity:** Medium
**Observation:** `get_config` 直接 `Json(state.config.clone())`。前端设置页需要回显，但任何能打到本机 API 的调用方（本机其他源 + SEC-001、或 `BOENMIND_BIND=0.0.0.0` 且无 token）拿到全部云厂商密钥。插件设置有 masked 回显，主配置没有。
**Question:** GET 是否对 `api_key` 做与插件 secret 相同的掩码，仅在 PUT 时提交未改掩码则保留原值？

### SEC-006: `resolve_root` 让 workspace API 以任意绝对路径为根
**File(s):** `backend/crates/bm-server/src/routes/workspace.rs:15-20,31-36,59-67,122-131`；对比 `bm-core/src/workspace.rs:30-47`
**Severity:** Medium
**Observation:** `safe_join` 只保证 rel 不逃出 **给定 root**。`root` 查询/JSON 参数可为任意绝对路径（测试还锁定了 `D:\projects\my-app` 行为）。编程壳项目切换需要它，但缺少“允许的项目根”白名单。配合 SEC-001，本机网页可对任意磁盘路径 list/read/write。
**Question:** 是否维护已打开项目根白名单（前端 projects + 配置 working_dir），拒绝白名单外 root？

### SEC-007: 终端 API 任意 cwd、无权限询问、完整交互式 shell
**File(s):** `backend/crates/bm-server/src/routes/terminal.rs:38-55`；`backend/crates/bm-server/src/terminal.rs:1-6,75-77`
**Severity:** Medium
**Observation:** 模块头写明“用户自己开的终端不触发插件权限询问”。`cwd` 无校验。在 SEC-001 成立时，本机恶意页可 `POST /api/terminal` 再注入命令。即使用户操作，也无会话级确认。
**Question:** 创建终端是否校验 cwd 属于项目白名单，并要求与 chat 相同的 Origin + 可选一次性确认？

### SEC-008: 子代理 child 模式仍可仅凭环境变量进入
**File(s):** `backend/crates/bm-server/src/subagent_child.rs:42-50`
**Severity:** Low
**Observation:** `(has_json || has_parent) && (has_no_session || has_parent)`——只设 `PI_SUBAGENT_PARENT_PID` 即可进 child 模式，消耗 API 额度、读配置。上轮 SEC-004 未变。本地威胁模型有限。
**Question:** 是否要求 `--mode json` 与 parent pid 同时成立，并对 pid 做存活校验？

### SEC-009: Bearer token 比较非恒定时间
**File(s):** `backend/crates/bm-server/src/lib.rs:394-399`
**Severity:** Low
**Observation:** `token == expected`。服务器部署下远程攻击者可对时序侧信道（实际难度高）。
**Question:** 是否改用 `subtle` 恒定时间比较？

---

## Performance（PERF-xxx）

### PERF-001: 每步全量 `derive_messages` + schema JSON 估 token，长会话 O(n²)
**File(s):** `backend/crates/bm-loop/src/engine.rs:642-650,715-725`
**Severity:** Medium
**Observation:** 步循环在工具后 `flush` → 整分支重放投影 → `estimate_context` 对 system+tools JSON+全部消息 `estimate_tokens`。上轮 PERF-001 主体仍在。128K / 多步回合本地 CPU 可观。
**Question:** 是否按分支缓存投影并只折叠新增 seq？tools schema 的 token 估计是否可缓存到 registry 变更？

### PERF-002: 角色注入每步同步读盘解析 roles.json
**File(s):** `backend/crates/bm-server/src/roles.rs:21-38,59-62`
**Severity:** Low
**Observation:** `RoleInjector` 头注自称“无内部状态（每次请求读文件）”。与 PERF-001 叠加为每步固定 IO。上轮 PERF-003 残留。
**Question:** 是否按 mtime 做 1s 级缓存？

### PERF-003: 事件订阅仍 250ms SQLite 轮询，同会话多标签不合并
**File(s):** `backend/crates/bm-kernel/src/event_log.rs:347-398`
**Severity:** Low
**Observation:** 每订阅一条 tail 任务。总线未接线（ARCH-002）。单用户可接受。
**Question:** 同 `(session, branch)` 是否引用计数合并轮询，或改总线直推？

### PERF-004: 每次内置工具执行 `BuiltinTools::new`；每次插件 HTTP `Client::new`
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:296,1231`
**Severity:** Low
**Observation:** `QuickJsToolExecutor` 已有 `engine.working_dir` 与 `engine.builtin`，却在分派处重建 `BuiltinTools`。`http` hostcall 每次新建 reqwest Client（丢连接池、TLS 会话）。
**Question:** 执行路径是否复用 `BridgeServices.builtin` 与共享 `reqwest::Client`？

### PERF-005: 插件 HTTP 响应体无上限
**File(s):** `backend/crates/bm-server/src/compat_engine.rs:337-345`
**Severity:** Medium
**Observation:** `response.bytes().await` 读入全部 body。恶意/被劫持端点可把数 GB 推进进程内存，卡死回合。提供商路径有 64KB 上限（`providers.rs:150-153`），此处不对齐。
**Question:** 是否设硬上限（如 8MB）并截断/报错？

---

## Quality（QUAL-xxx）

### QUAL-001: 协议死变体仍在：StepEnd / SessionEndSeed
**File(s):** `backend/crates/bm-protocol/src/event.rs:141,192,229,242`
**Severity:** Medium
**Observation:** 全库仍无生产写者（与上轮 QUAL-001 相同，`HeaderReason::Resume` 未再穷举）。读协议者会以为有这些生命周期事件。
**Question:** 删除死变体，或在 loop/会话删除路径补写者？

### QUAL-002: `enqueue_turn` / `run` 队列 API 仍仅测试使用
**File(s):** `backend/crates/bm-loop/src/engine.rs:197-265`；`backend/crates/bm-loop/tests/engine_tests.rs:271-272`
**Severity:** Low
**Observation:** 生产走 `run_turn`。头注仍保留 next-turn 队列。上轮 QUAL-003 残留。
**Question:** 删除队列字段与 API，或标明为多源回合预留且加生产调用？

### QUAL-003: Memory 注入生产路径 `expect`
**File(s):** `backend/crates/bm-memory/src/lib.rs:146-154`
**Severity:** Medium
**Observation:** `inject_payload` 对 `payload.messages` / `content` 使用 `expect`。注释假设 engine 保证形状。若第二调用方（治理/测试/错误拼装）传入非数组，**进程 panic** 而非 fail-open。同文件 `RoleInjector` 用的是 `if let` 静默返回。
**Question:** 是否与 roles.rs 对齐为 fail-open（形状不对就不注入）？

### QUAL-004: 插件禁用不卸载，工具面活到进程重启
**File(s):** `backend/crates/bm-server/src/routes/plugins.rs:36-37`
**Severity:** Medium
**Observation:** 启用走 `compat.reload` + `invalidate_loop_agents`；禁用注释写明“无运行时卸载路径，工具面保留至服务重启”。设置页“关闭插件”立即失效的预期不成立。
**Question:** 是否实现按 extension_id 卸载 / 从 tools 快照剔除，或 UI 标注“需重启”？

### QUAL-005: `bm-compat` `extensions_js.rs` 仍是三万行级单文件
**File(s):** `backend/crates/bm-compat/src/extensions_js.rs`（体量与上轮一致量级）
**Severity:** Medium
**Observation:** 运行时、宿主桥、内嵌 JS、测试挤在同一文件。与 ARCH-003 叠加，权限/exec 路径难审。
**Question:** 是否按 runtime / hostcall / policy / tests 拆分？

### QUAL-006: Markdown 未过滤 `javascript:` 链接
**File(s):** `frontend/src/components/shared/Markdown.tsx:11-30`
**Severity:** Low
**Observation:** `react-markdown` 默认不渲染原始 HTML（无 `rehype-raw`），XSS 面小于 `dangerouslySetInnerHTML`。但 `a` 组件透传 `href`，模型/文件内容里的 `[x](javascript:...)` 可变成可点击伪协议。聊天场景相对本机，风险有限。
**Question:** 是否加 `rehype-sanitize` 或在 `a` 组件拒绝非 http(s)/# 协议？

### QUAL-007: `put_config` 不失效 agent、不重载插件策略
**File(s):** `backend/crates/bm-server/src/routes/config.rs:12-52`；对比 `routes/plugins.rs:43`、`routes/skills.rs:128`
**Severity:** Medium
**Observation:** 插件/技能变更会 `invalidate_loop_agents`。全量保存配置（含 default provider、custom_system_prompt、extension_policy、compaction、apps.expert）不会。与 ARCH-001/006、BUG-001/003 同源。
**Question:** `put_config` 成功后是否统一 invalidate + 同步 kernel config + 热更新 gate？

### QUAL-008: 权限档位注释/UI/实现三套语言不一致
**File(s):** `backend/crates/bm-core/src/config.rs:100-106`；`backend/crates/bm-server/src/compat_engine.rs:1003-1020`；`frontend/src/stores/app-store.ts:399-404`
**Severity:** Low
**Observation:** config 注释写 safe/balanced/permissive；compat 注释写 default/safe/balanced/permissive/yolo；前端还有 default。后端 `Some(other)` 一律 Prompt。读者无法从代码得出四档差异。
**Question:** 是否把档位收成一个枚举 + 单一映射表（含 default_caps/deny/ask_high_risk）？

---

## Bugs（BUG-xxx）

### BUG-001: 设置页保存 API 密钥/端点后，已有会话与 LlmPort 仍用启动时的旧凭证
**File(s):** `backend/crates/bm-server/src/lib.rs:575`；`backend/crates/bm-server/src/routes/config.rs:50`；`backend/crates/bm-server/src/bm_engine.rs:318-328,567-575`
**Severity:** High
**Observation:** 实证阅读：`shared_config` 只在启动 clone；`put_config` 只替换 `state.config`；`build_loop_agent` 成功走 LlmPort 时**忽略**传入的最新 `ProviderConfig`；agent 复用条件不含 api_key/base_url。复现路径：启动 → 设置页改 key → 同一会话继续聊 → 仍用旧 key；新会话同样走 LlmPort 旧快照。直调退化分支（无 kernel）才会用 `state.config` 里的新 key，而生产有 kernel。
**Question:** 修复是合并配置锁，还是 `put_config` 写回 `shared_config` 并 `invalidate_loop_agents`？是否补回归测试：“clone 后改 key，LlmPort.resolve_config 必须看到新值”？

### BUG-002: 工具执行中取消仍可能留下永久未闭合 ToolCall
**File(s):** `backend/crates/bm-loop/src/engine.rs:574-615,626-635`
**Severity:** Medium
**Observation:** `select!` 命中 `cancel.changed()` 时 `break`，不落 `ToolResult`。ToolCall 已 push。投影会滤掉未闭合调用，审计面残缺。上轮 BUG-002 **仍在**。若 `execute` 已在跑（bash），取消只停等待、不杀进程。
**Question:** 取消时是否补 `ToolResult { ok:false, output:"cancelled" }`？是否把 cancel 传到 executor/进程 kill？

### BUG-003: 运行时切换权限档位不改变 BuiltinGate / McpGate / ExtensionPolicy
**File(s):** `backend/crates/bm-server/src/lib.rs:678-682,788`；`backend/crates/bm-server/src/builtin_gate.rs:27-51`；`backend/crates/bm-server/src/routes/config.rs:50`
**Severity:** High
**Observation:** `ask_high_risk` 是 `BuiltinGate` 字段，启动后只读。用户 yolo → 点过“总是允许”后切回 safe：记忆仍放行；即使清记忆，`ask_high_risk==false` 时根本不问。这是安全开关失效，不是文档问题。
**Question:** gate 是否每次 `check` 读当前 config？或 `setPermissionMode`/`put_config` 必须重建 gate？

### BUG-004: `origin_middleware` 在无 Origin 且无 Referer 时放行全部非 GET
**File(s):** `backend/crates/bm-server/src/lib.rs:419-426`
**Severity:** Medium
**Observation:** 注释称兼容 curl/同源。浏览器现代跨站 POST 必带 Origin，此分支主要影响非浏览器与部分旧式客户端。结合默认无 token，本机任意进程可无头驱动 API（威胁模型若含“恶意本地进程”则成立）。
**Question:** 桌面是否应始终要求自定义头或 loopback token，而把“无 Origin 放行”留给显式 CLI 标志？

### BUG-005: `referer_allowed` 对任意 `tauri://` 前缀放行
**File(s):** `backend/crates/bm-server/src/lib.rs:441-444`
**Severity:** Low
**Observation:** CORS Origin 只允许 `tauri://localhost`；Referer 分支 `starts_with("tauri://")` 更宽。无 Origin、伪造 Referer 的非浏览器客户端可利用（浏览器难伪造 Referer）。
**Question:** Referer 是否与 Origin 使用同一 host 白名单函数？

---

## Improvements（IMP-xxx）

### IMP-001: 配置单一事实源
**File(s):** `backend/crates/bm-server/src/lib.rs:575-635`；`routes/config.rs`
**Severity:** High
**Observation:** 承接 ARCH-001 / BUG-001。Port 层应订阅 `AppState.config`，避免第二份 RwLock。
**Question:** 是否在本迭代收口，而不是再加同步函数？

### IMP-002: 权限档位运行时生效 + 拉开 safe/balanced
**File(s):** `compat_engine.rs:1008-1027`；`builtin_gate.rs`；`app-store.ts:395-418`
**Severity:** High
**Observation:** 承接 ARCH-006 / BUG-003。建议一张表：`safe`（http 询问、无 exec）、`balanced/default`（http 默认、exec 询问）、`permissive`（全自动）、`yolo`（+exec/env）。
**Question:** 产品上四档的真实语义是否就是这张表？

### IMP-003: 收紧本机 CSRF（自定义头或绑定端口白名单）
**File(s):** `backend/crates/bm-server/src/lib.rs:270-289,406-433`；`frontend/src/api/client.ts`
**Severity:** High
**Observation:** 承接 SEC-001。前端已有 `authHeaders()`，可强制 `X-BoenMind: 1`（简单请求无法带自定义头，可破 form CSRF）。
**Question:** 是否接受“无该头的浏览器请求一律 403”，CLI 用 token 或显式头豁免？

### IMP-004: 插件 HTTP 与提供商 SSRF 对齐，并做响应上限
**File(s):** `compat_engine.rs:283-345`；`providers.rs:97-133`
**Severity:** High
**Observation:** 承接 SEC-002 / SEC-004 / PERF-005。
**Question:** 插件 http 是否直接调用同一 `validate_http_url` + 8MB 上限？

### IMP-005: 内置文件工具 cwd 圈禁
**File(s):** `builtin_tools.rs:198-207`
**Severity:** Medium
**Observation:** 承接 SEC-003。`safe_join` 已存在且测试充分。
**Question:** 绝对路径拒绝，还是升为与 bash 同级询问？

### IMP-006: EventBus 替换 250ms 轮询
**File(s):** `bm-kernel/src/event_log.rs:339-400`
**Severity:** Low
**Observation:** 承接 ARCH-002 / PERF-003。上轮 IMP-003 未做。
**Question:** 是否仍排在 A6？

### IMP-007: 增量投影缓存
**File(s):** `bm-loop/src/engine.rs:644-650`
**Severity:** Low
**Observation:** 承接 PERF-001。loop 对会话串行写，缓存一致性简单。
**Question:** 是否随压缩事务一并做？

### IMP-008: 场景工具注册表，移出 wiki 字面量
**File(s):** `bm_engine.rs:366-380`；`compat_engine.rs:1215-1230`
**Severity:** Medium
**Observation:** 承接 ARCH-005。完善性：下一 APP 不应再改执行中枢。
**Question:** 是否接受 `SceneTools` trait（definitions + execute）由各场景 crate 注册？

---

## 维度评分 1-10

| 维度 | 分 | 说明 |
|------|----|------|
| 架构合理性 | 7 | 分层、Port、依赖守卫仍扎实；config 双源与场景硬编码是明显裂痕 |
| 精简 | 6 | 死变体/队列 API/双裁决/双轨 Port 仍在；wiki 字面量进通用路径 |
| 优美 | 7 | 模块头注释与 fail-closed 纪律仍好；档位注释≠实现损害可读性 |
| 复用 | 7 | SSE/usePolling/ask_capability 已收敛；HTTP SSRF 与 Client 未复用 |
| 完善 | 6 | 上轮接线大多落地；热更新与档位语义未完成 |
| 安全 | 6 | CSRF/bash 门/测试套件已补；本机任意端口 CSRF、插件 http、绝对路径读写仍高危 |
| 性能 | 7 | 单用户可接受；每步全量投影与无界 HTTP body 是下一刀 |
| 正确性 | 6 | 编译全绿；配置热更新与档位热更新在生产路径上不正确 |

**综合：6.5 / 10**（比 2026-08-16 的“骨架已立、接线未完”前进一步，进入“接线已完、运行时一致性未完”）。

---

## 与 2026-08-16 基线对比

| 上轮项 | 2026-08-17 状态 |
|--------|-----------------|
| BUG-001 event_log 测试无法编译 | **已修**（`cargo check -p bm-event-log-tests --tests` 绿） |
| SEC-001 无 CSRF | **部分修**：有 Origin/Referer 中间件；**本机任意端口仍过**（新 SEC-001） |
| SEC-002 内置 bash 无门 | **已修** BuiltinGate；**read/write/edit 仍无圈禁**（新 SEC-003） |
| ARCH-001 extension_policy 死配置 | **部分修**：启动期映射；**不热更新 + 四档坍缩**（ARCH-006 / BUG-003） |
| QUAL-005 三处 SSE 复制 | **已修** `readSSEStream` |
| 前端 tsc / 漏译 / theme / usePolling | **tsc 绿**；usePolling 在 |
| run_agent_turn / memory 单例 / Compactor port / context_window | **已接线**（memory 仍有每会话回落） |
| 双写冻结 | **已标注**，不当新缺陷，保留 ARCH-004 跟踪收口 |
| EventBus / 死变体 / enqueue_turn / 双 check_capability / 每步全量投影 / 取消无 ToolResult | **仍在** |

**新出现的最值钱问题（本轮应优先）：**

1. **BUG-001 + IMP-001**：config 双源 → 改 API key 不生效  
2. **BUG-003 + ARCH-006**：权限档位运行时无效  
3. **SEC-001 + IMP-003**：localhost 任意端口 CSRF  
4. **SEC-002 + IMP-004**：插件 http 静默 + 无 SSRF  
5. **SEC-003 + IMP-005**：内置文件工具绝对路径

编译：**后端 bm-server / bm-kernel / bm-loop / bm-compat / bm-core / bm-event-log-tests 绿；前端 tsc -b 绿。**
