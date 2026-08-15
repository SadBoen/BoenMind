# BoenMind 2.0 核心实现方案 —— 最小内核 + 会话事件日志

> 状态：**T0-T13 全部完成**（2026-08-14 夜，commit 255b2ac + ae72750），80 测试全绿 + clippy 零警告。
> 实现期修正/partial 标注见文末 §7（实现记录）。

---

## 0. 范围界定

**本方案实现什么**：最小内核四件套（插件加载器 / 服务注册表 / 事件总线 / 会话事件日志原语）+ 会话日志 turso 落库（阶段 0 双写过渡）。

**本方案不实现什么**（后续阶段）：agent-loop 移植（阶段 1 后半）、pi-compat（阶段 1）、把关链/权限（阶段 2）、网络/RPC 插件化（阶段 3）、应用插件机制（阶段 4）、记忆/Steward（阶段 5）。

**成功判据**：`cargo test` 全绿；一个 30 轮对话的事件流重放两次字节一致；bm-server 聊天与事件日志双写运行（阶段 0 验收）。

## 1. 目标仓库结构（crate 布局）

```
backend/
├── Cargo.toml                     # workspace 成员加入 bm-protocol / bm-kernel
├── crates/
│   ├── bm-core/                   # 现有：领域层（不动，仅新增 db 双写扩展点）
│   ├── bm-server/                 # 现有：HTTP/SSE（阶段 0 加事件投影路由）
│   ├── bm-protocol/               # 【新】纯契约 crate——零运行时依赖（无 tokio/turso/axum）
│   │   ├── Cargo.toml             # 仅 serde + serde_json（序列化是契约的一部分）
│   │   └── src/
│   │       ├── lib.rs             # re-export
│   │       ├── ids.rs             # SessionId/BranchId/SeqNo/CallId（typed 包装）
│   │       ├── event.rs           # CoreEvent（强类型核心域）+ CustomEvent + SessionEvent 信封
│   │       ├── port.rs            # Port traits：EventStorePort/ModelProviderPort/…（14 个起步）
│   │       ├── plugin.rs          # Plugin trait + ServiceKey + Disposer
│   │       ├── policy.rs          # Capability 模式串 + PolicySet + PolicyEvaluation
│   │       ├── surface.rs         # SurfaceOp（Append/Replace）+ SurfaceEvent
│   │       └── error.rs           # 类型化错误码（capability_escalation_denied 等风格）
│   └── bm-kernel/                 # 【新】内核实现 crate
│       ├── Cargo.toml             # 依赖：bm-protocol + tokio + serde_json
│       └── src/
│           ├── lib.rs             # KernelBuilder：组装四件套的入口
│           ├── ctx.rs             # Ctx：plugin/service/on/emit/waterfall/parallel/serial
│           ├── registry.rs        # 服务注册表（拓扑排序 + 失败回滚）
│           ├── bus.rs             # 事件总线（四种分发模式）
│           ├── loader.rs          # 插件加载器（manifest + 依赖解析 + 可逆副作用）
│           ├── event_log.rs       # EventLog 原语（内存实现，trait 化）
│           ├── projection.rs      # Projection trait + 消息面投影（SurfaceProjection）
│           └── validation.rs      # 事件校验（seq 连续/JSON/ignorable 守卫/surface 语义）
├── storage/                       # 【新】存储后端 crate（阶段 0 落库）
│   └── bm-storage-turso/
│       ├── Cargo.toml             # 依赖：bm-protocol + turso
│       └── src/
│           ├── event_log.rs       # EventStorePort 的 turso 实现（单写者 Mutex）
│           ├── checkpoint.rs      # 请求边界 fsync + interrupted 恢复
│           └── dual_write.rs      # 阶段 0 双写过渡（现有表 + 事件流）
└── tests/                         # 【新】集成测试
    └── event_log/
        ├── replay_determinism.rs  # 重放两次字节一致（Life Agent OS 验证过的姿势）
        ├── ignorable_guard.rs     # 未知事件语义（跳过 vs 拒绝重建）
        └── fork_branch.rs         # branch_id 三维寻址
```

## 2. 接口定义（Rust 签名）

### 2.1 bm-protocol：契约层

```rust
// ---- ids.rs：typed 包装（Life Agent OS 风格）----
pub struct SessionId(pub String);      // "sess_<hex16>"
pub struct BranchId(pub String);       // "main" | "br_<hex16>"
pub struct SeqNo(pub u64);

// ---- event.rs：核心域事件（强类型 enum，变体即契约）----
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    TurnStart { turn: u32 },
    TurnEnd { turn: u32, reason: TurnEndReason },
    StepStart { turn: u32, step: u32 },
    StepEnd { turn: u32, step: u32 },
    UserMessage { msg: UserMsg, source: UserMsgSource },   // source: human/inject/goal
    AssistantChunk { turn: u32, step: u32, chunk: StreamChunk },
    AssistantMessage { turn: u32, step: u32, msg: AssistantMsg, usage: Option<TokenUsage> },
    ToolCall { turn: u32, step: u32, call_id: CallId, name: String, args: String }, // args 原样 JSON
    ToolResult { turn: u32, step: u32, call_id: CallId, result: ToolResultMsg, meta: Option<JsonValue> },
    RequestHeader { header: EpochHeader, reason: HeaderReason },  // initial/resume/change
    CompactionStart { .. }, CompactionSummary { .. }, CompactionEnd { .. },
    MemoryWrite { key: String, data: JsonValue },
    TodoWrite { todos: Vec<TodoItem> },                    // 活任务清单（编程应用核心）
    SessionEndSeed,
    // 扩展口：插件域事件统一走 Custom（Spec D4 原则）
    Custom { event_type: String, data: JsonValue },        // "app.wiki.indexed" / "infra.net.health" …
}

// ---- event.rs：信封（日志落盘形态）----
#[derive(Serialize, Deserialize)]
pub struct SessionEvent {
    pub seq: SeqNo,
    pub session_id: SessionId,
    pub branch_id: BranchId,           // 首版落字段，分支 UI 二期（A1）
    pub time: i64,                     // epoch ms
    #[serde(flatten)]
    pub kind: EventKind,               // CoreEvent | 插件注册事件（EventId + 版本）
    pub ignorable: bool,               // 未认识可跳过；缺省=必需（不认识须拒绝重建）
    pub surface_op: Option<SurfaceOp>, // 仅消息面事件
    pub source_seqs: Option<Vec<u64>>, // 引用链（压缩遮蔽、chunk→message）
}

// ---- port.rs：Port traits（内核依赖 Port 而非实现，A2）----
#[async_trait]
pub trait EventStorePort: Send + Sync {
    async fn append(&self, ev: SessionEvent) -> Result<SeqNo>;      // 原子 append，返回分配的 seq
    async fn append_batch(&self, evs: Vec<SessionEvent>) -> Result<Vec<SeqNo>>; // 原子批量
    async fn read(&self, q: EventQuery) -> Result<Vec<SessionEvent>>; // 按 (session, branch, seq 范围)
    async fn head_seq(&self, sid: &SessionId, bid: &BranchId) -> Result<Option<SeqNo>>;
    async fn subscribe(&self, sid: &SessionId) -> EventStream;       // replay-prefix + tail
}

// ---- plugin.rs ----
pub trait Plugin: Send + Sync {
    fn deps(&self) -> &[ServiceKey];                       // 依赖声明 → 拓扑排序
    fn apply(&mut self, ctx: &mut Ctx) -> Result<Vec<Disposer>>;    // 注册一切，返回可逆副作用
}
pub type ServiceKey = &'static str;                        // "event_store" / "model_provider" / …
pub struct Disposer(Box<dyn FnOnce() + Send>);             // RAII：卸载 = 逆序执行

// ---- policy.rs：能力模式串（A4）----
pub struct Capability(pub String);  // "fs:write:/session/**"  glob 模式
pub enum PolicyEvaluation { Allowed, RequiresApproval { justification: String }, Denied(ErrorCode) }

// ---- surface.rs ----
pub enum SurfaceOp { Append, Replace { start: u64, end: u64 } }  // 压缩遮蔽（D3/D9）
```

### 2.2 bm-kernel：内核实现

```rust
// ---- ctx.rs：Ctx 是插件的唯一视角 ----
pub struct Ctx { /* 内部：registry + bus 引用 */ }
impl Ctx {
    pub fn plugin(&mut self, p: Box<dyn Plugin>) -> Result<()>;   // 挂载（依赖就绪才启动）
    pub fn service<T: ?Sized>(&self, key: ServiceKey) -> Result<Arc<T>>; // 按 key 找服务
    pub fn on<E: EventName>(&mut self, ev: E, handler: Handler) -> Disposer;  // 观察
    pub fn emit(&self, ev: impl Into<EventKind>);                 // 不 await，按注册序观察
    pub fn waterfall<A, R>(&self, ev: &str, args: A, default: impl FnOnce() -> R) -> R; // 环绕中间件
    pub fn parallel(&self, ev: &str, args: ...) -> impl Future;   // 并行扇出
    pub fn serial(&self, ev: &str, args: ...) -> impl Future;     // 按序执行
}

// ---- event_log.rs：日志原语（内核只承诺语义，不承诺存储）----
pub struct EventLog { store: Arc<dyn EventStorePort> }
impl EventLog {
    pub fn append(&self, session_id: &SessionId, kind: EventKind, surface: SurfaceIntent) -> Result<SeqNo>;
    pub fn derive_messages(&self, session_id: &SessionId, branch: &BranchId) -> Result<Vec<Message>>;
    pub fn fork(&self, sid: &SessionId, from_branch: &BranchId) -> Result<BranchId>;  // 三维寻址
    pub fn replay(&self, sid: &SessionId, branch: &BranchId) -> Result<Vec<SessionEvent>>;
}

// ---- projection.rs：状态恢复的 canonical 姿势（A10）----
pub trait Projection: Send + Sync {
    fn on_event(&mut self, ev: &SessionEvent);      // 逐个折叠
    fn checkpoint(&self) -> JsonValue;              // 可序列化快照（加速恢复）
}
pub struct SurfaceProjection { /* 消息面：user/assistant/tool 排序视图 */ }
```

## 3. Schema（turso 事件日志表）

```sql
-- 事件日志（阶段 0 双写过渡的核心表，boenmind.db 新增）
CREATE TABLE IF NOT EXISTS event_log (
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,   -- 全局单调（单写者保证）
  session_id   TEXT NOT NULL,
  branch_id    TEXT NOT NULL DEFAULT 'main',
  time         INTEGER NOT NULL,                    -- epoch ms
  type         TEXT NOT NULL,                       -- "turn/start" | "app.wiki.indexed" | …
  data         TEXT NOT NULL,                       -- lossless JSON（data 字段）
  ignorable    INTEGER NOT NULL DEFAULT 0,
  surface_op   TEXT,                                -- NULL | 'append' | 'replace:start:end'
  source_seqs  TEXT,                                -- NULL | JSON array
  UNIQUE (session_id, branch_id, seq)               -- 三维寻址（A1）
);
CREATE INDEX IF NOT EXISTS idx_event_log_lookup
  ON event_log (session_id, branch_id, seq DESC);

-- 分支头（fork/merge 语义，首版落字段 + 事件，UI 二期）
CREATE TABLE IF NOT EXISTS branch_heads (
  session_id TEXT NOT NULL,
  branch_id  TEXT NOT NULL,
  parent_branch TEXT,                               -- fork 来源
  merged_into TEXT,                                 -- merge 后转只读
  head_seq   INTEGER NOT NULL,
  PRIMARY KEY (session_id, branch_id)
);
```

## 4. 实现任务清单（有序，含依赖）

| # | 任务 | 依赖 | 说明 |
|---|---|---|---|
| T0 | workspace 骨架：`bm-protocol`/`bm-kernel`/`bm-storage-turso` crate 加入 | — | 空 crate + 测试目录就位 |
| T1 | `bm-protocol` 类型层：ids/event/surface/policy/error | T0 | 纯类型 + serde，**零运行时依赖**（铁律：契约 crate 纯净） |
| T2 | `bm-protocol` Port traits：EventStorePort 先做（其余 Port 留接口骨架） | T1 | `#[async_trait]` + mock 测试 |
| T3 | `bm-kernel` Ctx + Registry：service 按 key 注册/查找 + deps 拓扑排序 + 失败回滚 | T1 | 每个 Port 有独立 mock 测试 |
| T4 | `bm-kernel` EventBus：emit/waterfall/parallel/serial 四种分发 | T3 | waterfall 短路语义（不调 next = 短路） |
| T5 | `bm-kernel` Loader：manifest（极简 plugin.json，Z1）+ 可逆副作用（Disposer 逆序） | T4 | 卸载 = 撤销一切注册 |
| T6 | `bm-kernel` EventLog 内存实现 + append 原子性 + seq 分配 | T4 | 单写者 Mutex |
| T7 | `bm-kernel` 事件校验器：seq 连续/JSON 无损/ignorable 守卫/surface 语义 | T6 | 未知必需事件 → 拒绝重建（D2） |
| T8 | `bm-kernel` 投影：Projection trait + SurfaceProjection（消息面） | T7 | 重放两次字节一致测试（A10） |
| T9 | `bm-storage-turso`：EventStorePort 的 turso 实现 + 表迁移 | T2, T6 | 单写者 + 原子批量 append_batch |
| T10 | checkpoint 策略：请求边界 fsync + 崩溃 `interrupted` 标记恢复 | T9 | dsh checkpoint-policy 语义 |
| T11 | 双写过渡：bm-server chat 路由在现有落库的同时 append 事件流 | T9, T10 | 阶段 0 验收：双写运行 + 重放一致 |
| T12 | 分支字段：branch_id 落库 + fork 工具（首版仅内核级） | T9 | 三维寻址 + fork 超头拒绝 |
| T13 | 集成测试套件：replay_determinism / ignorable_guard / fork_branch / checkpoint_recovery | T8-T12 | invariant + property-based |

**依赖图**：T0→T1→T2→T3→T4→T5（Loader 可与 T3/T4 并行后半段）；T6→T7→T8；T2+T6→T9→T10→T11；T9→T12；T8-T12→T13。

## 5. 约束（非显而易见规则，实现时必须遵守）

1. **`bm-protocol` 零运行时依赖**——不许出现 tokio/turso/axum/redb（契约 crate 纯净性是内核"最小"的物理锁，A2/Life 验证过的姿势）；
2. **吸收不进核心**（铁律 3）：任何"顺手就能做"的功能放进 bm-kernel 即违规——放插件/存储/应用层；
3. **模型可见即已记录**（不变量 1）：新模型可见输入必须新增事件类型，禁止"只改内存不落日志"；
4. **事件 append 原子、seq 连续**：重复/跳号拒绝（校验器强制）；跨进程不直写日志（走 RPC 代理，首版不承诺多进程写）；
5. **未知事件**：`ignorable=true` 跳过、缺省拒绝重建（D2 语义，防旧版本静默读坏新日志）；
6. **压缩/记忆/一切投影从日志可重放复现**（不变量 3）——任何投影必须能通过 replay 重建，禁止依赖投影外状态；
7. **首版只注册正在使用的类型**（S9）——`app/*`、`goal/*` 等扩展域先用后注册；
8. **Rust 两层分治**（5.2）：核心域 = `CoreEvent` 强类型 enum；插件域 = `Custom { event_type: "命名空间.事件" }`，禁止往 CoreEvent 里加插件专属变体；
9. **错误全部类型化**（kernel.chat 风格）：`capability_escalation_denied` / `budget_exceeded` / `seq_gap` / `unknown_required_event` / `surface_violation`…，禁止裸 string 错误；
10. **能力矩阵按 shipped/partial 诚实标注**（避免 kernel.chat 的"宣称与交付脱节"教训）——每个声明的能力有可执行路径，否则标注 partial。

## 6. 测试策略

- **确定性重放测试**（核心）：同一事件流重放两次，投影结果字节一致（Life Agent OS 验证过的姿势）；
- **invariant 测试**：每个不变量一个测试文件（seq 连续/ignorable/surface 语义/模型可见即记录）；
- **property-based**（proptest）：随机事件序列 → 重放一致性 + 校验器拒绝率；
- **集成**：真实 turso 文件 + 30 轮模拟对话双写 → 重放重建 → 与现有 DB 数据比对；
- **验收门槛**：`cargo test -p bm-protocol -p bm-kernel -p bm-storage-turso` 全绿 + clippy 零警告（对齐现有质量门）。

---
*（实现方案 v1.0 完。对应架构 v0.10 阶段 0 + 阶段 1 内核部分。待用户拍板后从 T0 开工。）*

---

## 7. 实现记录（2026-08-14 夜，T0-T13 全部落地）

### 7.1 已交付（commit 255b2ac + ae72750）

| 任务 | 落点 | 验证 |
|---|---|---|
| T0-T2 | `crates/bm-protocol`（ids/event/surface/policy/error/port，零运行时依赖；BoxFuture 手写签名替代 async-trait） | 13 测试 |
| T3-T5 | `crates/bm-kernel`：KernelBuilder/Ctx/Registry（重复拒绝）/EventBus（emit/waterfall 短路/parallel/serial）/Loader（manifest+deps 拓扑+Disposer 逆序回滚） | 38 测试 |
| T6-T8 | EventLog 内存实现（单写者原子 append）+ EventValidator（seq 连续/JSON 无损/ignorable/surface）+ SurfaceProjection（chunk 合并/工具配对/压缩遮蔽/占位填充） | 单测 + 集成 |
| T9-T10 | `crates/bm-storage-turso`：TursoEventStore（WAL + synchronous=FULL）+ CheckpointStore（interrupted 恢复）+ DualWriter（best-effort + 计数） | 集成测试 |
| T11 | bm-server chat 双写（AppState.dual_writer: Option<Arc<DualWriter>>；UserMessage→TurnStart→工具/助手/TurnEnd batch） | dual_write_30rounds 验收 |
| T12 | branch_heads 表 + fork 工具（三维寻址、超头/重复拒绝、持久化） | fork_branch 集成测试 |
| T13 | 集成测试四件套 + dual_write_30rounds（重放两次字节一致、内存与 turso 消息面一致） | 全绿 |

### 7.2 实现期修正（相对 §3 Schema）

- **seq 不用 AUTOINCREMENT**：全局计数与"分支内 seq 连续"矛盾（跨分支事件打洞），且事务回滚后 AUTOINCREMENT 不回用号码；改为应用层分配（读分支 head → +1 → 显式 INSERT），UNIQUE (session_id, branch_id, seq) 兜底，单写者锁内完成保证原子。

### 7.3 partial 标注（能力矩阵诚实）

- `ToolResult.output` 暂不落日志（chat.rs 双写处 output 为空串）——agent-loop 移植（阶段 1）时补；
- `EventStorePort::subscribe`（replay-prefix + tail）未实现——阶段 1 事件流推送；当前 read + head_seq 轮询等效；
- 删除会话时事件日志不联动清理——阶段 1 接 delete 事件流；
- 阶段 0 双写容错：事件日志写失败仅告警计数，主链路不受影响。
