# Architecture Audit（工具 C · ln-24）

**Verdict:** FAIL
**Checklist: 44/44 complete**
**Incomplete:** None

> 2026-08-17 只读。硬排除 WIKI 功能。未改文件。Git：`main` @ `f8066a6`；脏树仅 tauri lock 与 `dist-win/`，不计入架构。基线已修项（配置单锁 / CSRF 头 / workspace 白名单 / 文件圈禁 / fork 日志 / GET 掩码）本轮复核仍在。

材料性门后 Findings 只有代码所有权两条；文档权威冲突单列，不进 Findings 表。

---

## Actual architecture

单进程本地 Agent 运行时。`bm-protocol` → `bm-kernel` → storage/loop → 插件 crate → `bm-core` → `bm-server` 组合根。前端 ClassicShell 默认；桌面壳代码已退役。存储：`messages` = 消息当前真相；`event_log` = sidecar。

关键流：`POST /api/chat` → `run_agent_turn`；权限在 executor 的 BuiltinGate/McpGate（热读档位）；配置一把 `Arc<RwLock<AppConfig>>`。

**MCP：** `serve_inner` 建 `McpClientManager` 后若 `servers().is_empty()` 丢弃为 `(None, None)`。`connect_server` 在 `state.mcp` 为 None 时 503。

主导模型：插件边界 + 组装层编译内置。三轨并存（QuickJS / loop 契约 / 组装内置）。双写冻至 M3。

## Fitness summary

| Area | Status | Evidence |
|---|---|---|
| Pattern fitness and ownership | FAIL | MCP 组合根按启动数据缺席（F1）；插件禁用不收回（F2） |
| Contracts and boundaries | CONCERNS | Port JSON + 双路径可工作；TS 轨与 kernel 注册表隔离是能力边界 |
| Dependency topology | PASS | L9 六 crate 守卫；无禁边/环 |
| Physical structure and configuration | CONCERNS | 配置单锁成立；活文档落后 |

对用户四问：

1. **架构文件要不要大手术？不要。** 长文仍是有效系统-design 基线。要增量修订当前态。
2. **更合理方案？** 有界修补：MCP 始终持有空管理器；禁用走执行期收回。不换骨架。
3. **严重缺陷？** 一条 P1：默认空配置下设置页加第一个 MCP 永久 503。
4. **文档完善？** 结构丰富，权威不干净。

## Findings

| Priority | Problem | Evidence | Required resolution |
|---|---|---|---|
| P1 | MCP 服务所有权按启动时是否已有连接决定；空配置下 UI 连接第一个 server 503 | `lib.rs` 790–807 空则 None；`routes/mcp.rs` 59–63 | 组合根始终 `Some(manager)`（零 server 合法空集）。实践：[Composition Root](https://blog.ploeh.dk/2011/07/28/CompositionRoot/) |
| P2 | 插件禁用/卸载不收回工具面与执行权至重启 | `routes/plugins.rs` 仅 enable 时 reload；`CompatEngine::reload` 只增量 load | 按 enabled 过滤工具面 + executor fail-closed + invalidate。实践：[OWASP Authorization](https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html) |

无其他候选过材料性门。

**不要做：** 重写内核/换 dsh；M3 前双写对账；删 EventBus/LoopHooks/enqueue_turn；拆 `serve_inner` 当正确性前提。

## 文档权威冲突（不进 Findings）

| 工件 | 分类 | 冲突 |
|---|---|---|
| everything-is-plugin-architecture.md | 基线 current，混 target/历史 | 13 面接线；§6.9 现状过期；§15.1 未接线快照 |
| EXTENSION_POINTS_REGISTRY.md | 部分 stale | tools/notify/scheduler/credentials 状态；缺 mcp |
| HANDOFF_KERNEL_PHASE1.md | stale | 双 DE；M2 下一步自相矛盾 |
| README.md | stale | v0.24；缺 08-17 / design/ |
| SETTINGS_ARCHITECTURE | in-progress 文首过期 | 阶段表 ✅ 后又 ⬜；未写 MCP 组合根约束 |
| SERVICE_FACES archive | superseded | 止于 13 面 |

文档策略：保留长文为决策与铁律载体；短「执行态」表 + 刷新登记表/交接。重排全书收益低于误改铁律的风险。
