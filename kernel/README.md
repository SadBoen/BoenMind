# BoenMind 微内核（kernel/）

Rust 微内核骨架（v2.1 计划 M1 产物）。**前端借 dsh 生态，后端全 Rust**：
事件日志 = 唯一事实源，sessions/messages/tool_calls 为投影；turn/step waterfall
事件语义对齐 dsh harness（model-visible-means-logged）。

## Crate 分层（依赖只许向下）

```
layer 5  kernel-contracts    端口 trait + 事件词汇 + DTO（LlmPort/FsPort/ShellPort/
                             SessionPersistPort/PluginRuntimePort/EventBus）
layer 4  kernel-session      append-only SessionEvent 日志 + 投影（derive_messages）
layer 4  kernel-llm          LlmPort 的 mock 实现（ScriptLlm/MockTurn，门禁 1）
layer 4  kernel-tools        工具注册表 + 门控（enabled 名单 + fail-closed）
layer 4  kernel-storage      sqlite 持久化（WAL + synchronous=FULL + 单事务原子发布）
layer 3  kernel-loop         ReactLoopAgent：turn/step 驱动 + logged-means-persisted
layer 3  kernel-supervisor   插件进程宿主雏形（拉起/健康检查/崩溃重启）
layer 2  kernel-assembly     组合根（Runtime）+ interrupted-turn 修复
layer 1  headless            headless 二进制（门禁 1 验收载体）
```

边界守卫 = `kernel-assembly/tests/crate_boundaries.rs`（Rust 集成测试，
`cargo test --workspace` 即门禁；无需外部 Python）。

## 存储模型（v2.1 拍板）

- **事件日志（append-only）= 唯一事实源**；sessions 表只是 header 索引。
- **原子性**：每次 append 是单个 sqlite 事务（WAL + synchronous=FULL）。
  kill -9 发生在事务提交前 → 批次整体丢失 → 日志永远没有半条事件。
- **logged-means-persisted**：loop 里每个事件 append 后立即落盘。
- **interrupted-turn 修复**：kill -9 恢复时，`restore_session` 把尾部未配对的
  Step/Turn Started 修剪掉并**写回磁盘**（`rewrite_events`），保证磁盘与内存一致。

## 命令

```bash
cargo test --workspace        # 全量测试（含 crate 边界守卫）
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p headless
bash scripts/verify-gate1.sh  # 门禁 1 验收（roundtrip + kill-9 恢复 + 尾部完整性）
```

## headless 二进制

```bash
headless roundtrip <db> <session>   # 建会话 + mock LLM 工具回合全链路
headless abort <db> <session> <1|2> # 自死在断点（模拟 kill -9）
headless resume <db> <session>      # 修复 torn 尾部 + 续跑
headless verify-tail <db> <session> # 断言日志尾部配对完整
headless dump <db> <session>        # 打印事件日志（调试）
```
