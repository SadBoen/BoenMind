# 服务面铺开图纸（SERVICE FACES，2026-08-15 定稿）

> 背景：用户对标 dsh 的 20+ 挂点（`inject` 服务面），指出我们"5 个挂点（LoopHooks）"与"万物皆插件"差距远，心里有道槛。
> 事实核查：bm-kernel 的 `Plugin::deps()` + `Ctx`（register_service/service/port/on/emit/around/waterfall/parallel/serial）**已是 Cordis 同构体**，缺的是生产路径上的"官方全家桶铺基础面"——bm-server 组装层（`AppState`）把能力内置了，没有注册成 kernel 服务。
> 定调（用户拍板"全做"）：**服务面 = 平台对插件的承诺 API，主动铺；实现面才等第二实现**（原"第一个第二实现出现时"判据只适用于换实现类挂点，已修正）。
> 纪律：遵守 `bm-protocol/src/port.rs`"不建空 trait 占位"原则——每个服务面必须有真实实现可包；真正的预留口（sandbox/subprocess）标注待立项。

## 一、目标服务面清单（~17 个）

| # | key | Port（bm-protocol） | 现有实现 | 现状 | 可替换性 |
|---|---|---|---|---|---|
| 1 | `event_store` | `EventStorePort` | TursoEventStore / InMemory | ✅ 已注册（kernel 内置） | 可替换（存储层已 Port 化） |
| 2 | `compactor` | CompactorPort（新） | bm-compactor `CompactorPlugin` | ✅ 已注册（第一根接线） | 可替换（压缩策略插件） |
| 3 | `memory` | MemoryPort（新） | `bm_memory::MemoryFilePlugin` | ⏳ 有实现未注册（挂在 StreamHooks.on_request） | 可替换（§6.1 插件化） |
| 4 | `llm` | LlmPort（新） | bm-core providers/chat 客户端 | ⛔ 内置（bm_engine 直接构造） | 可替换（多提供商适配） |
| 5 | `tools` | ToolsPort（新） | compat_engine ToolRegistry | ⛔ 内置（插件工具贡献已活） | 插件贡献 + 宿主可换执行器 |
| 6 | `settings` | SettingsPort（新） | bm-core plugin_settings | ⛔ 内置 | 可替换（插件设置面） |
| 7 | `credentials` | CredentialsPort（新） | config/密钥管理 | ⛔ 内置 | 可替换 |
| 8 | `http` | HttpRoutePort（新） | axum Router（routes 硬编码） | ⛔ 内置 | 插件注册路由（dsh ctx.webServer 语义） |
| 9 | `session` | SessionStorePort（新） | Db（sessions/messages/tasks） | ⛔ 内置 | 可替换（存储已 turso 化） |
| 10 | `scheduler` | SchedulerPort（新） | steward 调度器（next_wake_at） | ⛔ 内置（steward 专用） | 可替换（唤醒策略） |
| 11 | `skill` | SkillPort（新） | routes/skills.rs + skill 注入 | ⛔ 内置 | 可替换（skill 目录面） |
| 12 | `stats` | StatsPort（新） | usage 统计（/api/sessions/{id}/usage） | ⛔ 内置 | 可替换 |
| 13 | `notify` | NotifyPort（新） | session_streams（SSE 通道） | ⛔ 内置 | 可替换（通知策略） |
| 14 | `gate` | GatePort（新） | PermissionBridge（B5 权限裁决） | ⛔ 内置 | 可替换（裁决策略） |
| 15 | `sandbox` | SandboxPort | 无（exec 政策拒绝） | ❌ 预留（政策口） | 政策口——股票插件计算路径（WASM/sidecar）立项时实现 |
| 16 | `subprocess` | SubprocessPort | 无 | ❌ 预留（政策口） | 政策口——dsh-ffmpeg 类重活出进程路径 |
| 17 | `event_log` | EventLogPort | DualWriter/EventLog | ⛔ 内置（权威核心） | **可注入不可夺权**（审计承诺） |

> 17 个里：2 个已注册（event_store/compactor）、2 个预留政策口（sandbox/subprocess）、其余 13 个 = "把现有实现包成服务面"（非空占位）。

## 一·补充：实施状态（2026-08-16 六批全部落地）

**13 个服务面全部注册完毕**（commit 链 d6e95cf → 9dbe9fd → e15952c → ff03160 → a4a7542 → 3f5bde9）：

| 批次 | 服务面 | 消费方（经 kernel.port） |
|---|---|---|
| 已有 | event_store / compactor | 事件日志 / bm-compactor |
| 第一批 | memory / settings / stats | 设置路由×2、用量路由 |
| 第二批 | llm / credentials | 聊天主链路 build_loop_agent |
| 第三批 | skill / tools | skills 路由×3（list/set/uninstall） |
| 第四批 | notify / scheduler | （运行期注册；notify 测试闭环） |
| 第五批 | session | sessions 路由×3（create/list/get） |
| 第六批 | gate | chat respond_permission |

基建：`KernelBuilder.with_service/with_port`（预注册）+ `Ctx::register_port`（运行期注册，tools/notify/scheduler/gate 用）；LoopHooks 扩到 10 挂点（on_turn_end 已接线）。测试：bm-server 121 / bm-kernel 60 全绿。

**http 面判 YAGNI（图纸修订）**：无现有实现可包（路由硬编码非可替换能力）、无消费方；统一分发器（fallback 分派）成本高收益零——随"首个插件 HTTP 路由需求"落地（接线判据：第一个第二实现出现时）。sandbox/subprocess 两个政策口随股票插件计算路径立项。

## 二、LoopHooks 扩面（bm-loop/src/points.rs，现状 10 个方法）

现有：`on_request` / `on_request_error` / `on_tool_pre` / `on_tool_post` / `on_turn_stopping` / `on_stream_chunk`。

新增（每加一个 = 一个点名需求，不等"第二实现"）：
- `on_context_build`：请求上下文组装可改写（D8 提示词插件；比 on_request 覆盖更全，含 header 构造）
- `on_compact_begin` / `on_compact_end`：压缩前后（评估插件/审计/压缩统计）
- `on_turn_end`：回合收尾（统计、记忆沉淀、评估）
- `on_provider_select`：提供商/模型选择前（路由策略、成本策略）

## 三、实施步骤（等并行会话落盘后执行）

1. **bm-protocol**：新增 Port trait（memory/llm/tools/settings/credentials/http/session/scheduler/skill/stats/notify/gate/compactor），签名沿用 BoxFuture 手写风格，零新依赖
2. **bm-kernel**：`KernelBuilder` 加 `with_service(key, Arc<T>)` / `with_port(key, PortBox)`；验证 `contains` 已就绪
3. **bm-server 组装层**（serve_inner）：把 AppState 各字段/全局构造包成注册——优先低风险面（memory/settings/stats/compactor 对齐），随后 llm/tools/session/http；**消费方改为 `kernel.port` 取用**（渐进替换，先 memory + settings + stats）
4. **LoopHooks 扩面**：points.rs 加 5 个方法（默认空实现，零破坏）+ StreamHooks 接线（on_turn_end → 记忆沉淀候选）
5. **测试**：kernel 装配测试（每面注册/取用/替换）+ bm-server 冒烟；`cargo test -p bm-protocol -p bm-kernel -p bm-server`
6. **文档**：架构文档 §六 挂点清单更新 + 本图纸归档

## 四、冲突纪律（本次执行前提）

- 并行会话未提交改动：`bm-core/src/plugins.rs`、`bm-server/src/bm_engine.rs`、`bm-server/src/governance.rs`、`bm-server/src/lib.rs`、i18n×4、plugins 删除/新增（roles.rs、coding-memory/、role/）——**代码改造必须等其落盘后启动**
- 启动前 `git pull`；每次 commit 前 `git status` 确认无他人未提交改动混入
- 小步提交：每完成一个服务面一个 commit

## 五、验收判据

- 生产路径 ServiceKey 注册数 ≥ 13（2 已注册 + 13 新注册，不含 2 预留口）
- 至少 3 个消费方从 `kernel.port` 取服务（不再直接 new/取 AppState 字段）
- 新增 LoopHooks 全部有默认空实现 + 至少 2 个有真实消费方
- 全部测试绿 + 桌面壳/API 冒烟通过
