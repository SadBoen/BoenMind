---
status: accepted
date: 2026-09-12
summary: surface"服务层边界"核实——AppState 持 EventStore 是端口依赖非越层、SSE 直读日志是刻意性能设计;真实债仅 chat_completions 巨型 handler,已抽取会话寻址与派发(462→316 行)
supersedes: []
superseded_by: []
---

# ADR-0044: surface「服务层边界」核实与巨型 handler 拆分 
- 关联: ADR-0042(核实轮方法论)、ADR-0009(部署与 Surface)、基线 §14(Surface 与核心解耦) - 背景: 早先架构报告提出「`bm-surface-http` 兼服务层:直接持 `EventStore`、直读文件做编排,应引入服务层」。按 ADR-0042 的方法先核实,结果**大部为假阳性**,仅一处真实债。 
## 核实结论 
1. **`AppState` 持 `Arc<dyn EventStore>` 不是越层**(假阳性)。它依赖的是 core 的**端口 trait**,不是具体适配器——符合六边形依赖倒置。surface 使用它的 4 处全是 `replay_since`/`last_log_seq`(读事件日志做 SSE 推送)。 2. **SSE 直读日志是刻意的性能设计**(非缺陷)。`sse.rs` 150ms 增量轮询 `store.replay_since`;若改走 `handle.events_poll`(actor 命令),会把每次轮询压到**单写者热路径**上——对 150ms 频率的流式推送是**倒退**。故保留直读。 3. **真实债仅一处**:`chat_completions` 巨型 handler,**462 行**——把 HTTP 校验 + 模型路由校验 + 会话寻址 + 消息派发 + 流式回放混在一个函数里。 
## 决策 
**抽取 `chat_completions` 内的「会话寻址 + 消息派发」块为独立函数 `resolve_and_dispatch`**(纯机械移动,零行为变更): - 该块原本已是 `tokio::spawn` 的独立任务单元(客户端掉线只丢响应、回合照常完成); - 参数显式化(10 个:`handle`/`v1_sessions`/`store`/`data_dir`/`default_model`/`requested_model`/`requested_workspace`/`target_role_id`/`text`/`headers`),`Prepared` 枚举随之外置; - handler 侧改为 `tokio::spawn(resolve_and_dispatch(...))` 调用。 
## 后果 
- `chat_completions` **462 → 316 行**(拆出的 `resolve_and_dispatch` 157 行);职责从「5 合 1」降为「HTTP 校验 + 派发 + 流式回放」。 - **零行为变更**:全仓 501 测试全绿,`clippy -D warnings` 零警告。 - **不做**:不引入独立「服务层 crate」——surface 已是薄传输层(依赖端口、编排大多已下沉 RuntimeHandle),为剩余 handler 体量新建一层属过度设计;不把 SSE 直读改走 actor(会退化)。 - **方法论再次验证**:报告的三条「surface 越层」中,两条假阳性(端口依赖被误读为越层、性能设计被误读为缺陷),一条为真但体积远小于报告描述(462 行而非"整个 surface 兼服务层")。**核实优先于动手**。 