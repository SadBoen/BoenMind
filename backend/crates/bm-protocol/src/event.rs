//! 核心域事件 + 会话事件信封（事件日志落盘形态）。
//!
//! 两层分治（实现方案 §5-8）：核心域 = [`CoreEvent`] 强类型 enum，
//! 插件域 = [`CustomEvent`]（`事件类型: "命名空间.事件"`），**禁止**
//! 往 CoreEvent 加插件专属变体。
//!
//! 未知事件语义（D2，实现方案 §5-5）：`ignorable=true` 跳过、
//! 缺省（false）拒绝重建——防旧版本静默读坏新日志。

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::ids::{BranchId, CallId, SeqNo, SessionId};
use crate::surface::SurfaceOp;

// ---------------------------------------------------------------------------
// 消息体小结构
// ---------------------------------------------------------------------------

/// 用户消息来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserMsgSource {
    /// 用户亲自输入
    Human,
    /// 系统/插件注入（goal 展开、指令注入）
    Inject,
    /// 目标驱动的自主触发
    Goal,
}

/// 用户消息（内容原样，附件后续阶段补充）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMsg {
    pub content: String,
}

/// 流式块（AssistantChunk 携带，逐块落日志 → 消息面投影合并）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamChunk {
    pub text: String,
}

/// 助手完整消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMsg {
    pub content: String,
}

/// 工具调用结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultMsg {
    pub ok: bool,
    pub output: String,
}

/// Token 用量（可选，仅 AssistantMessage 携带）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// 回合结束原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEndReason {
    Completed,
    Failed,
    Cancelled,
    /// 进程崩溃/断电遗留的未闭合回合（启动恢复补写，dsh 语义）
    Interrupted,
}

/// 请求头原因（一次模型调用的头事件）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaderReason {
    /// 回合首个请求
    Initial,
    /// 断线续跑/恢复
    Resume,
    /// 请求参数变更（切模型/换提供商）
    Change,
}

/// 回合请求头（epoch 标识一次模型调用链）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochHeader {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub created_at: i64,
    /// 模型可见输入的审计锚点：system prompt + 工具 schema 的 sha256（hex）。
    /// 阶段 1（pi 引擎）覆盖 BoenMind 注入面（自定义系统提示词/skills 注入/
    /// 扩展路径=已注册工具代理）；A6 自研 loop 后覆盖完整输入。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_hash: Option<String>,
}

/// 活任务清单条目（编程应用核心，TodoWrite 携带）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    /// pending | in_progress | completed
    pub priority: Option<String>,
}

/// 压缩摘要（CompactionSummary 携带）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionSummaryMsg {
    /// 被压缩遮蔽的 seq 区间起点
    pub removed_start: u64,
    pub removed_end: u64,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// 核心域事件
// ---------------------------------------------------------------------------

/// 核心域事件。`type` 字段即日志表 type 列（"turn/start" 风格）。
///
/// **不变量（模型可见即已记录）**：任何影响模型可见状态的事件必须
/// 在这里有变体或走 Custom，禁止"只改内存不落日志"。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    /// 回合开始
    #[serde(rename = "turn/start")]
    TurnStart { turn: u32 },
    /// 回合结束（reason: completed/failed/cancelled/interrupted）
    #[serde(rename = "turn/end")]
    TurnEnd { turn: u32, reason: TurnEndReason },
    /// 步骤开始（回合内一个模型-工具循环）
    #[serde(rename = "step/start")]
    StepStart { turn: u32, step: u32 },
    /// 步骤结束
    #[serde(rename = "step/end")]
    StepEnd { turn: u32, step: u32 },
    /// 用户消息入日志（source: human/inject/goal）
    #[serde(rename = "user/message")]
    UserMessage { msg: UserMsg, source: UserMsgSource },
    /// 助手流式块（逐块记录，投影合并）
    #[serde(rename = "assistant/chunk")]
    AssistantChunk { turn: u32, step: u32, chunk: StreamChunk },
    /// 助手完整消息（附 token 用量）
    #[serde(rename = "assistant/message")]
    AssistantMessage {
        turn: u32,
        step: u32,
        msg: AssistantMsg,
        usage: Option<TokenUsage>,
    },
    /// 工具调用（args 原样 JSON 字符串）
    #[serde(rename = "tool/call")]
    ToolCall { turn: u32, step: u32, call_id: CallId, name: String, args: String },
    /// 工具结果（call_id ↔ ToolCall 关联）
    #[serde(rename = "tool/result")]
    ToolResult {
        turn: u32,
        step: u32,
        call_id: CallId,
        result: ToolResultMsg,
        meta: Option<JsonValue>,
    },
    /// 请求头（initial/resume/change，模型调用链标识）
    #[serde(rename = "request/header")]
    RequestHeader { header: EpochHeader, reason: HeaderReason },
    /// 压缩开始
    #[serde(rename = "compaction/start")]
    CompactionStart { turn: u32 },
    /// 压缩摘要（removed 区间 + 摘要文本）
    #[serde(rename = "compaction/summary")]
    CompactionSummary { msg: CompactionSummaryMsg },
    /// 压缩结束
    #[serde(rename = "compaction/end")]
    CompactionEnd { turn: u32 },
    /// 记忆写入
    #[serde(rename = "memory/write")]
    MemoryWrite { key: String, data: JsonValue },
    /// 活任务清单快照
    #[serde(rename = "todo/write")]
    TodoWrite { todos: Vec<TodoItem> },
    /// 会话结束种子（日志闭合标记）
    #[serde(rename = "session/end")]
    SessionEndSeed,
}

/// 插件域事件（事件类型: "app.wiki.indexed" / "infra.net.health" …）。
/// 内核透传不解释；注册/订阅语义见架构 §6.2。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomEvent {
    /// "命名空间.事件"，如 "app.wiki.indexed"
    pub event_type: String,
    pub data: JsonValue,
}

/// 日志里的统一事件体：核心事件或插件自定义事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Core(CoreEvent),
    Custom(CustomEvent),
}

impl EventKind {
    /// 事件类型名（日志 type 列 / 订阅匹配键）：
    /// Core → "turn/start" 等；Custom → "app.wiki.indexed" 等。
    pub fn name(&self) -> String {
        match self {
            EventKind::Core(ev) => core_type_name(ev).to_string(),
            EventKind::Custom(c) => c.event_type.clone(),
        }
    }
}

/// 核心事件的 type 名（serde rename 的单点映射，避免手写字符串漂移）。
pub fn core_type_name(ev: &CoreEvent) -> &'static str {
    match ev {
        CoreEvent::TurnStart { .. } => "turn/start",
        CoreEvent::TurnEnd { .. } => "turn/end",
        CoreEvent::StepStart { .. } => "step/start",
        CoreEvent::StepEnd { .. } => "step/end",
        CoreEvent::UserMessage { .. } => "user/message",
        CoreEvent::AssistantChunk { .. } => "assistant/chunk",
        CoreEvent::AssistantMessage { .. } => "assistant/message",
        CoreEvent::ToolCall { .. } => "tool/call",
        CoreEvent::ToolResult { .. } => "tool/result",
        CoreEvent::RequestHeader { .. } => "request/header",
        CoreEvent::CompactionStart { .. } => "compaction/start",
        CoreEvent::CompactionSummary { .. } => "compaction/summary",
        CoreEvent::CompactionEnd { .. } => "compaction/end",
        CoreEvent::MemoryWrite { .. } => "memory/write",
        CoreEvent::TodoWrite { .. } => "todo/write",
        CoreEvent::SessionEndSeed => "session/end",
    }
}

// ---------------------------------------------------------------------------
// 会话事件信封（日志落盘形态）
// ---------------------------------------------------------------------------

/// 会话事件格式版本（dsh SESSION_FORMAT_VERSION 语义）：
/// 信封结构演进时递增；**写者决定 bump**（"能解析 ≠ 语义正确"）。
/// 读者发现 version != 当前值 → 拒绝重建（FormatVersionMismatch）。
pub const SESSION_FORMAT_VERSION: u32 = 1;

/// 事件日志的落盘形态：信封 + kind（flatten 展开）。
///
/// - `version`：信封格式版本（缺省 0 = 版本化之前的旧数据，读者据此拒绝）
/// - `seq`：分支内单调连续，由存储层分配（append 原子性保证）
/// - `time`：epoch ms
/// - `ignorable`：未认识可跳过；缺省=false（必需，不认识须拒绝重建）
/// - `surface_op`：仅消息面事件携带
/// - `source_seqs`：引用链（压缩遮蔽 / chunk→message 归并依据）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    #[serde(default)]
    pub version: u32,
    pub seq: SeqNo,
    pub session_id: SessionId,
    pub branch_id: BranchId,
    pub time: i64,
    #[serde(flatten)]
    pub kind: EventKind,
    #[serde(default)]
    pub ignorable: bool,
    pub surface_op: Option<SurfaceOp>,
    pub source_seqs: Option<Vec<SeqNo>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(seq: u64) -> SessionEvent {
        SessionEvent {
            version: SESSION_FORMAT_VERSION,
            seq: SeqNo::new(seq),
            session_id: SessionId::new("sess_abc"),
            branch_id: BranchId::new("main"),
            time: 1_750_000_000_000,
            kind: EventKind::Core(CoreEvent::TurnStart { turn: 1 }),
            ignorable: false,
            surface_op: None,
            source_seqs: None,
        }
    }

    #[test]
    fn envelope_serde_roundtrip_lossless() {
        let ev = sess(1);
        let json = serde_json::to_string(&ev).unwrap();
        let back: SessionEvent = serde_json::from_str(&json).unwrap();
        // 字节级一致（默认 serde_json 映射排序稳定）
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
        assert_eq!(back, ev);
    }

    #[test]
    fn old_data_without_version_parses_as_zero() {
        // 版本化之前落盘的旧数据没有 version 字段 → 解析为 0，
        // 由读者按 version != 当前值拒绝（写者决定 bump 语义）
        let json = r#"{"seq":1,"session_id":"sess_abc","branch_id":"main","time":1,
                       "kind":"core","type":"turn/start","turn":1,"ignorable":false}"#;
        let ev: SessionEvent = serde_json::from_str(json).unwrap();
        assert_eq!(ev.version, 0);
        assert_ne!(ev.version, SESSION_FORMAT_VERSION);
    }

    #[test]
    fn envelope_flatten_shape() {
        // 信封 flatten 后顶层即 kind 字段（log 表可直接照此落列）
        let json = serde_json::to_string(&sess(7)).unwrap();
        assert!(json.contains(r#""type":"turn/start""#));
        assert!(json.contains(r#""seq":7"#));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "turn/start");
        assert_eq!(parsed["session_id"], "sess_abc");
    }

    #[test]
    fn kind_name_mapping() {
        assert_eq!(
            EventKind::Core(CoreEvent::TurnStart { turn: 1 }).name(),
            "turn/start"
        );
        assert_eq!(
            EventKind::Custom(CustomEvent {
                event_type: "app.wiki.indexed".into(),
                data: JsonValue::Null,
            })
            .name(),
            "app.wiki.indexed"
        );
    }

    #[test]
    fn unknown_type_fails_parse() {
        // 未知核心类型 → 反序列化失败（ignorable 守卫的判定基础）
        let json = r#"{"seq":1,"session_id":"sess_abc","branch_id":"main","time":1,
                       "kind":"core","type":"future/event","ignorable":false}"#;
        assert!(serde_json::from_str::<SessionEvent>(json).is_err());

        // Custom 永远"已知"（内核透传）
        let json = r#"{"seq":1,"session_id":"sess_abc","branch_id":"main","time":1,
                       "kind":"custom","event_type":"app.anything","data":{"ok":true}}"#;
        assert!(serde_json::from_str::<SessionEvent>(json).is_ok());
    }

    #[test]
    fn ignorable_defaults_false() {
        // 缺省 ignorable = false（必需），防旧版本静默读坏新日志
        let json = r#"{"seq":1,"session_id":"sess_abc","branch_id":"main","time":1,
                       "kind":"core","type":"turn/start","turn":1}"#;
        let ev: SessionEvent = serde_json::from_str(json).unwrap();
        assert!(!ev.ignorable);
    }

    #[test]
    fn epoch_header_prompt_hash_optional_and_defaults() {
        // 旧数据无 prompt_hash → 解析为 None（A2 后向兼容）
        let old = r#"{"provider":"openai","model":"gpt-5.6","created_at":1750000000000}"#;
        let h: EpochHeader = serde_json::from_str(old).unwrap();
        assert_eq!(h.prompt_hash, None);

        // 新数据带 hash 往返无损
        let h2 = EpochHeader {
            provider: Some("openai".into()),
            model: Some("gpt-5.6".into()),
            created_at: 1_750_000_000_000,
            prompt_hash: Some("abc123".into()),
        };
        let json = serde_json::to_string(&h2).unwrap();
        assert!(json.contains("prompt_hash"));
        let back: EpochHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h2);
    }

    #[test]
    fn event_kind_serde_roundtrip() {        let kinds = [
            EventKind::Core(CoreEvent::TurnEnd { turn: 2, reason: TurnEndReason::Failed }),
            EventKind::Core(CoreEvent::ToolCall {
                turn: 1,
                step: 2,
                call_id: CallId::new("call_1"),
                name: "web_search".into(),
                args: r#"{"q":"rust"}"#.into(),
            }),
            EventKind::Core(CoreEvent::TodoWrite {
                todos: vec![TodoItem {
                    content: "实现 T13".into(),
                    status: "in_progress".into(),
                    priority: Some("high".into()),
                }],
            }),
            EventKind::Custom(CustomEvent {
                event_type: "infra.net.health".into(),
                data: serde_json::json!({"latency_ms": 12}),
            }),
        ];
        for k in kinds {
            let json = serde_json::to_string(&k).unwrap();
            let back: EventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, k);
        }
    }
}
