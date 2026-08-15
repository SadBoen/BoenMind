//! Port traits：内核依赖 Port 而非实现（A2）。
//!
//! 首版只定义 [`EventStorePort`]（阶段 0 必需）；其余 Port
//! （ModelProviderPort/FileSystemPort/…）留待对应阶段按 S9
//! "只注册正在使用的类型"逐个添加——**不建空 trait 占位**
//! （诚实标注 partial，避免 kernel.chat 的宣称与交付脱节）。
//!
//! 签名用 `BoxFuture`（手写）而非 async-trait：保持契约 crate
//! 零额外依赖。

use std::future::Future;
use std::pin::Pin;

use crate::error::ProtocolError;
use crate::event::SessionEvent;
use crate::ids::{BranchId, SeqNo, SessionId};

/// 手写 async fn 签名（等价 async-trait 展开，零依赖）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 事件读取查询（按 (session, branch, seq 范围 / 事件类型)）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventQuery {
    pub session_id: SessionId,
    pub branch_id: BranchId,
    /// 只返回 seq > seq_gt 的事件
    pub seq_gt: Option<u64>,
    /// 只返回 seq <= seq_lte 的事件
    pub seq_lte: Option<u64>,
    /// 只返回该事件类型的事件（type 列 = [`EventKind::name`]，如 "todo/write"；
    /// None = 不过滤）。长会话投影（活任务清单等）用它替代全量重放。
    pub event_type: Option<String>,
    /// 返回条数上限（默认不限）
    pub limit: Option<u64>,
}

impl EventQuery {
    pub fn new(session_id: SessionId, branch_id: BranchId) -> Self {
        Self {
            session_id,
            branch_id,
            seq_gt: None,
            seq_lte: None,
            event_type: None,
            limit: None,
        }
    }

    /// 只读某类事件的便捷构造（如 `EventQuery::of_type(sid, bid, "todo/write")`）。
    pub fn of_type(session_id: SessionId, branch_id: BranchId, event_type: &str) -> Self {
        let mut q = Self::new(session_id, branch_id);
        q.event_type = Some(event_type.to_string());
        q
    }
}

/// 分支头（fork/merge 语义，branch_heads 表行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchHead {
    pub session_id: SessionId,
    pub branch_id: BranchId,
    /// fork 来源分支（main 为 None）
    pub parent_branch: Option<BranchId>,
    pub head_seq: SeqNo,
    /// fork 时父分支的 head 快照（A3 父前缀折叠的分叉点；main 为 None）。
    /// 父分支 seq <= forked_at 的事件对子分支可见，分叉后父分支新增不可见。
    pub forked_at: Option<u64>,
}

/// 事件存储端口。实现：内存（bm-kernel InMemoryEventStore）与
/// turso（bm-storage-turso）。**单写者约定**：跨进程不直写日志
/// （走 RPC 代理，首版不承诺多进程写，实现方案 §5-4）。
///
/// 能力矩阵（shipped/partial 诚实标注）：
/// - append / append_batch / read / head_seq：shipped
/// - 事件流订阅：kernel 级 `subscribe_events`（replay-prefix + tail 轮询，
///   A5 已落地；SSE 路由 /api/sessions/{id}/events 消费）——非本端口方法
pub trait EventStorePort: Send + Sync {
    /// 原子 append 单条事件，返回分配的 seq（存储层覆写信封 seq）。
    fn append(&self, ev: SessionEvent) -> BoxFuture<'_, Result<SeqNo, ProtocolError>>;

    /// 原子批量 append（seq 连续分配，失败整体不落）。
    fn append_batch(&self, evs: Vec<SessionEvent>) -> BoxFuture<'_, Result<Vec<SeqNo>, ProtocolError>>;

    /// 按查询读取事件（seq 升序）。
    fn read(&self, q: EventQuery) -> BoxFuture<'_, Result<Vec<SessionEvent>, ProtocolError>>;

    /// 分支当前头 seq（无事件为 None）。
    fn head_seq(&self, sid: &SessionId, bid: &BranchId) -> BoxFuture<'_, Result<Option<SeqNo>, ProtocolError>>;

    /// 按事件类型计数（event_type=None 计全量）。
    /// turn 计数等场景用，避免全量重放 O(n) 读。
    fn count(
        &self,
        sid: &SessionId,
        bid: &BranchId,
        event_type: Option<&str>,
    ) -> BoxFuture<'_, Result<u64, ProtocolError>>;

    /// fork 新分支（记录 parent，超头/重复拒绝）。`new` 由上层生成。
    fn fork_branch(
        &self,
        sid: &SessionId,
        from: &BranchId,
        new: &BranchId,
    ) -> BoxFuture<'_, Result<(), ProtocolError>>;

    /// 列出会话全部分支头。
    fn branch_heads(&self, sid: &SessionId) -> BoxFuture<'_, Result<Vec<BranchHead>, ProtocolError>>;

    /// 清空会话全部事件与分支头（回收站 C2 用户主动清除）。
    /// 返回删除的事件行数；分支头随之重置（下次 append 从 seq 1 重新起）。
    fn clear_session(&self, sid: &SessionId) -> BoxFuture<'_, Result<u64, ProtocolError>>;
}

/// 记忆面（服务面铺开第一批，SERVICE_FACES 图纸 #3）。
///
/// 实现：bm-memory 对 `Mutex<MemoryFilePlugin>` 的适配（bm-server 组装层
/// 注册 "memory" 服务）。可替换：未来记忆子系统（向量/淡化）以第二实现
/// 接管同 key——"服务面 = 承诺 API，实现面等第二实现"的第一批样板。
pub trait MemoryPort: Send + Sync {
    /// 记住一条事实（去重；失败静默——记忆是增强不是正确性依赖）。
    fn remember(&self, fact: String);

    /// 当前事实（最旧在前）。
    fn facts(&self) -> Vec<String>;

    /// 把记忆注入块追加进模型请求 payload 的 system 段（无则插入首条）。
    fn inject_into_payload(&self, payload: &mut serde_json::Value);
}

/// 设置面（插件设置存取 + secret 掩码语义，SERVICE_FACES 图纸 #6）。
///
/// `schema` = 插件 manifest settings 声明的 JSON 序列化（`Vec<SettingField>`，
/// bm-core 定义；契约层用 Value 传递保持零依赖——实现侧反序列化）。
pub trait SettingsPort: Send + Sync {
    /// 读设置（明文，仅服务端内部使用）。
    fn read(&self, plugin_id: &str, schema: &serde_json::Value) -> serde_json::Value;

    /// 读设置（secret 字段掩码回显——前端安全）。
    fn read_masked(&self, plugin_id: &str, schema: &serde_json::Value) -> serde_json::Value;

    /// 保存设置（类型校验 + 密钥掩码保留），返回掩码版供前端刷新。
    fn save(
        &self,
        plugin_id: &str,
        schema: &serde_json::Value,
        values: &serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError>;
}

/// 一次会话的 token 用量统计（assistant/message 事件聚合）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub messages: u64,
}

/// 统计面（用量/计数查询，SERVICE_FACES 图纸 #12）。
///
/// 实现：bm-server 对事件日志的聚合（StatsPortImpl）。消费方
/// （/api/sessions/{id}/usage）从 kernel 取服务，不再内联计算。
pub trait StatsPort: Send + Sync {
    /// 会话累计用量（读事件日志聚合；无事件 = 全零）。
    fn session_usage(
        &self,
        session_id: &SessionId,
    ) -> BoxFuture<'_, Result<SessionUsage, ProtocolError>>;
}

/// LLM 能力面（SERVICE_FACES 图纸 #4）：客户端配置解析 + 提供商清单。
///
/// 契约层不依赖 bm-loop 的 [`LlmConfig`]（类型在 loop 层），边界走 JSON——
/// 实现侧 serde 往返（bm-server resolve_llm_config 桥接语义保留在实现内）。
pub trait LlmPort: Send + Sync {
    /// 解析 provider+model 的客户端配置（base_url/api_key/model/
    /// reasoning_effort 的 JSON 视图）；未知提供商/未配置端点 → Err。
    fn resolve_config(
        &self,
        provider_id: &str,
        model: &str,
        thinking: Option<&str>,
    ) -> Result<serde_json::Value, ProtocolError>;

    /// 可用提供商清单（JSON 数组视图：id/name/kind/models/defaultModel）。
    fn providers(&self) -> serde_json::Value;
}

/// 凭证面（SERVICE_FACES 图纸 #7）：提供商密钥读取（明文，仅服务端内部）。
///
/// 实现：bm-server 读 AppConfig.providers（CredentialsPortImpl）。
/// 插件不得经此面取密钥——插件凭证走 settings 面（掩码语义），
/// 本面只服务宿主内部（LlmPort 解析、Steward 等）。
pub trait CredentialsPort: Send + Sync {
    /// 提供商 API 密钥；未知提供商/未配置 → None。
    fn api_key(&self, provider_id: &str) -> Option<String>;
}

/// 厂商面（SERVICE_FACES 图纸 #15，LLM provider 插件化方案 A）：厂商插件
/// 注册表查询。内置厂商 = 出厂注册（minimax/deepseek/custom，官方端点/
/// 协议形状单源 bm-core providers 表——/api/providers/presets 与 ProviderPort
/// 同源）；第三方厂商经 Custom（用户填端点 + 形状）或未来插件注册接入。
///
/// 实现：bm-server 对 AppConfig.providers 的适配（ProviderPortImpl）。
/// 消费方：LlmPort 解析（LlmPortImpl 经此面取官方端点/协议形状）。
/// 契约层 JSON 边界（bm-protocol 零依赖纪律——不引用 bm-core/loop 类型）。
pub trait ProviderPort: Send + Sync {
    /// 全部已注册厂商（JSON 数组视图：stableId/name/officialBaseUrl/shape/models）。
    fn providers(&self) -> serde_json::Value;

    /// 按稳定标识查厂商描述（None = 未注册）。stable_id = 内置厂商名
    /// （minimax/deepseek）或 custom-{id}（自定义类，取代 pi_name）。
    fn provider(&self, stable_id: &str) -> Option<serde_json::Value>;
}

/// 技能面（SERVICE_FACES 图纸 #11）：skill 目录 CRUD。
///
/// 实现：bm-server 包 bm-core skills 模块（SkillPortImpl，config 读写锁）。
/// 消费方：routes/skills.rs（kernel 可用时经服务，退化直调）。
pub trait SkillPort: Send + Sync {
    /// 已安装技能清单（SkillInfo JSON 视图）。
    fn list(&self) -> Result<serde_json::Value, ProtocolError>;

    /// 从本地路径安装（目录或 .md 文件）。
    fn install_path(&self, path: &str) -> Result<(), ProtocolError>;

    /// 从 GitHub 安装（owner/repo 内 skill_id）。
    fn install_github(&self, owner: &str, repo: &str, skill_id: &str) -> Result<(), ProtocolError>;

    /// 启停（改变注入面；调用方负责失效重建会话 agent）。
    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), ProtocolError>;

    /// 卸载。
    fn uninstall(&self, id: &str) -> Result<(), ProtocolError>;
}

/// 工具面（SERVICE_FACES 图纸 #5）：已装配工具清单查询（JSON 视图）。
///
/// 工具注册仍走 ToolRegistry（会话级）；本面 = 宿主工具快照查询
/// （审计/插件工具面）。实现：compat 引擎快照（运行期注册，见 serve_inner）。
pub trait ToolsPort: Send + Sync {
    /// 全部已装配工具（name/description/inputSchema JSON 视图）。
    fn list(&self) -> Vec<serde_json::Value>;

    /// 工具是否存在。
    fn has(&self, name: &str) -> bool;
}

/// 调度面（SERVICE_FACES 图纸 #10）：唤醒调度（Steward 轮的可替换策略面）。
///
/// 实现：StewardStore（治理夹区间在 store 内部）；set_wake(0) = 清除唤醒。
/// 消费方：管家 set_wake 工具执行侧、未来唤醒策略插件。
pub trait SchedulerPort: Send + Sync {
    /// 登记/更新唤醒（after_seconds 被治理层夹进 [pacing-min, pacing-max]）；
    /// 非管家会话 → Err。
    fn set_wake(
        &self,
        session_id: &str,
        after_seconds: i64,
        reason: Option<&str>,
    ) -> BoxFuture<'_, Result<(), ProtocolError>>;

    /// 清除唤醒（静默，直到下次 set_wake/inject）。
    fn clear_wake(&self, session_id: &str) -> BoxFuture<'_, Result<(), ProtocolError>>;
}

/// 通知面（SERVICE_FACES 图纸 #13）：会话级 SSE 推送（前端通道）。
///
/// 实现：AppState.session_streams（运行期注册）；事件 = AgentStreamEvent
/// 的 JSON 视图（契约层不依赖 bm-core 类型，实现侧 serde 往返）。
pub trait NotifyPort: Send + Sync {
    /// 推送事件到会话通道；通道不存在/已关闭（前端断开）→ false。
    fn push(&self, session_id: &str, event: serde_json::Value) -> bool;
}

/// 会话面（SERVICE_FACES 图纸 #9）：会话存储 CRUD（JSON 视图）。
///
/// 实现：bm-server 包 bm-core Db（SessionPortImpl）。消费方：
/// routes/sessions.rs（kernel 可用时经服务，退化直调）。
pub trait SessionPort: Send + Sync {
    /// 会话列表（Session JSON 数组视图）。
    fn list(&self) -> BoxFuture<'_, Result<serde_json::Value, ProtocolError>>;

    /// 创建会话（返回 Session JSON；id 由调用方生成）。
    fn create(
        &self,
        id: &str,
        provider_id: Option<&str>,
        model: Option<&str>,
        app: &str,
    ) -> BoxFuture<'_, Result<serde_json::Value, ProtocolError>>;

    /// 会话详情（None = 不存在）。
    fn get(&self, id: &str) -> BoxFuture<'_, Result<Option<serde_json::Value>, ProtocolError>>;

    /// 重命名。
    fn rename(&self, id: &str, title: &str) -> BoxFuture<'_, Result<(), ProtocolError>>;

    /// 删除会话（返回删除行数）。
    fn delete(&self, id: &str) -> BoxFuture<'_, Result<usize, ProtocolError>>;

    /// 会话消息列表（Message JSON 数组视图）。
    fn messages(&self, id: &str) -> BoxFuture<'_, Result<serde_json::Value, ProtocolError>>;
}

/// 权限面（SERVICE_FACES 图纸 #14）：权限询问决策回传（前端落点）。
///
/// 实现：permission_pending 询问表（GatePortImpl，运行期注册）。
/// 消费方：POST /api/chat/permission-response（chat.rs）。
pub trait GatePort: Send + Sync {
    /// 回应挂起的权限询问（允许/拒绝/总是允许）；未知询问 id → NotFound。
    fn respond(
        &self,
        request_id: &str,
        allow: bool,
        always: bool,
    ) -> BoxFuture<'_, Result<(), ProtocolError>>;
}
