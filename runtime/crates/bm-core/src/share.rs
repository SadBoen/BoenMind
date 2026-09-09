//! M11/ADR-0031 批次 1:Task 作用域共享面(公告栏)。
//!
//! 消息信封语义的最小先行面:成员把发现贴上本 Task 公告栏——share.published
//! 事件即事实(写穿持久,事件溯源),同 Task 成员按发布序读取。跨 Task 结构性
//! 隔离:task_id 一律自 principal 推导(agent:worker|coord:<task_id>),调用
//! 参数不可指定(防伪逃逸)。执行在内核内联(runtime::turn::share 拦截,占位
//! Provider 不触达),单写者纪律与 Broker 裁决/审计全覆盖;点名消息与成员级
//! 身份随批次 2(ADR-0031 决策 2),远程网格留阶段二。

use bm_contract::capability::CapabilityManifest;
use bm_contract::events::{EventEnvelope, EventType};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const SHARE_PUBLISH: &str = "task.share.publish";
pub const SHARE_LIST: &str = "task.share.list";
/// 能力名前缀(dispatch 层拦截位;新增 task.share.* 族能力自动走内核内联)。
pub const CAPABILITY_PREFIX: &str = "task.share.";

/// 单条共享发现(投影条目;seq = share.published 事件序)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareEntry {
    pub seq: u64,
    pub principal: String,
    pub title: String,
    pub content: String,
    pub occurred_at: String,
}

/// Task 公告栏投影:task_id → 发布序条目;单调 seq 折叠,同一事件前缀两次
/// 重建逐字节一致(ADR-0004 条件 1 同款;可随时丢弃自事件日志全量重建)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskShareBoard {
    shares: BTreeMap<String, Vec<ShareEntry>>,
    applied_to_seq: u64,
}

impl TaskShareBoard {
    /// 折叠单条事件;非 share.* 事件为 no-op。
    pub fn apply(&mut self, event: &EventEnvelope) {
        if !matches!(event.event_type, EventType::SharePublished) {
            return;
        }
        let p = &event.payload;
        let Some(task_id) = p["task_id"].as_str().map(str::to_string) else {
            return;
        };
        self.shares.entry(task_id).or_default().push(ShareEntry {
            seq: event.event_seq,
            principal: p["principal"].as_str().unwrap_or_default().to_string(),
            title: p["title"].as_str().unwrap_or_default().to_string(),
            content: p["content"].as_str().unwrap_or_default().to_string(),
            occurred_at: event.occurred_at.clone(),
        });
        self.applied_to_seq = self.applied_to_seq.max(event.event_seq);
    }

    /// 全量重建:自事件流折叠(与增量 apply 等价,有测试锁死)。
    pub fn rebuild(events: &[EventEnvelope]) -> Self {
        let mut board = Self::default();
        for e in events {
            board.apply(e);
        }
        board
    }

    /// 按 task 读公告栏;since_seq 只取该事件序之后(增量拉取)。
    pub fn list(&self, task_id: &str, since_seq: u64) -> Vec<&ShareEntry> {
        self.shares
            .get(task_id)
            .map(|v| v.iter().filter(|s| s.seq > since_seq).collect())
            .unwrap_or_default()
    }

    /// 投影位点(已折叠到的 event_seq;可观测丢弃/重建的一致性)。
    pub fn applied_to_seq(&self) -> u64 {
        self.applied_to_seq
    }
}

/// 主体归属 Task:仅 Task 域主体(worker/coord)有公告栏;task_id 一律自
/// principal 结构推导,调用参数不可指定(防伪逃逸)。
pub fn task_id_of_principal(principal: &str) -> Option<String> {
    let segs: Vec<&str> = principal.split(':').collect();
    match segs.as_slice() {
        ["agent", "worker" | "coord", task_id] if !task_id.is_empty() => {
            Some((*task_id).to_string())
        }
        _ => None,
    }
}

fn share_entry(
    capability: &str,
    effect: &str,
    idempotent: bool,
    description: &str,
    input_schema: Value,
) -> (
    CapabilityManifest,
    Arc<dyn crate::registry::CapabilityProvider>,
) {
    let manifest: CapabilityManifest = serde_json::from_value(json!({
        "capability": capability,
        "provider": "builtin.core",
        "version": "0.1.0",
        "description": description,
        "input_schema": input_schema,
        "output_schema": {"type": "object"},
        "effect": effect,
        "idempotent": idempotent,
        "cancellable": false,
        "timeout_ms": 5_000,
        "approval": "not-required",
        "scopes": ["domain:task"]
    }))
    .expect("task.share manifest 合法");
    (manifest, Arc::new(SharePlaceholder))
}

/// 占位执行体:真实执行在内核内联(dispatch_share 拦截),Provider 通道
/// 直调一律拒绝——防绕过 turn 语义(fs.* 同款口径)。
struct SharePlaceholder;
impl crate::registry::CapabilityProvider for SharePlaceholder {
    fn invoke(&self, _args: Value) -> Result<Value, String> {
        Err("task.share.* 在内核内联执行,不经 Provider 通道".into())
    }
}

/// task.share.* 能力装配集(公告栏一对;进 boenmind-server 生产装配)。
/// 权限语义(ADR-0031 决策 + 既有 Broker 三层,无需新裁决步):
/// trusted 主体步 6 直通;worker/coord 走 Task 授权 Grant(步 4 批量预授权,
/// ADR-0002 裁决 4);无 Grant 的 untrusted 调用 publish 升级审批
/// (Reversible 生效)、list 默认拒绝(ADR-0006)。
pub fn share_capability_entries() -> Vec<(
    CapabilityManifest,
    Arc<dyn crate::registry::CapabilityProvider>,
)> {
    vec![
        share_entry(
            SHARE_PUBLISH,
            "low-risk-command",
            false,
            "把一条发现/结论贴到当前 Task 的共享公告栏,同 Task 成员立即可见。只写本任务公告栏,不触外部世界。参数:title(一句话标题,必填)、content(发现正文,必填)。所属 Task 由调用主体自动归属,不可也无法指定。",
            json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "一句话标题"},
                    "content": {"type": "string", "description": "发现/结论正文"}
                },
                "required": ["title", "content"]
            }),
        ),
        share_entry(
            SHARE_LIST,
            "read-only",
            true,
            "读取当前 Task 的共享公告栏(同 Task 成员发布的发现,按发布序返回)。可选 since_seq 只取该事件序之后的条目(增量拉取)。所属 Task 由调用主体自动归属。",
            json!({
                "type": "object",
                "properties": {
                    "since_seq": {"type": "integer", "description": "只返回该事件序之后的条目(可选,默认 0 = 全量)"}
                }
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(seq: u64, task: &str, title: &str) -> EventEnvelope {
        EventEnvelope::new(
            seq,
            EventType::SharePublished,
            "2026-09-10T00:00:00.000Z".into(),
            None,
            None,
            None,
            json!({"task_id": task, "principal": "agent:worker:t1", "title": title, "content": "c"}),
        )
    }

    #[test]
    fn principal_derivation_only_for_task_scope() {
        assert_eq!(
            task_id_of_principal("agent:worker:task_01X").as_deref(),
            Some("task_01X")
        );
        assert_eq!(
            task_id_of_principal("agent:coord:task_01X").as_deref(),
            Some("task_01X")
        );
        // 空 task 段 / 非 Task 域主体 / 多余段一律无归属
        assert_eq!(task_id_of_principal("agent:worker:"), None);
        assert_eq!(task_id_of_principal("surface:user"), None);
        assert_eq!(task_id_of_principal("agent:worker:a:b"), None);
        assert_eq!(task_id_of_principal(""), None);
    }

    #[test]
    fn board_folds_incremental_and_rebuild_equally() {
        let events = vec![env(5, "tA", "一"), env(7, "tB", "二"), env(9, "tA", "三")];
        let mut live = TaskShareBoard::default();
        for e in &events {
            live.apply(e);
        }
        // 增量 == 全量重建(逐字节一致语义)
        assert_eq!(TaskShareBoard::rebuild(&events), live);
        assert_eq!(live.applied_to_seq(), 9);
        // Task 隔离 + 增量位
        assert_eq!(live.list("tA", 0).len(), 2);
        assert_eq!(live.list("tA", 5).len(), 1);
        assert_eq!(live.list("tB", 0).len(), 1);
        assert_eq!(live.list("tC", 0).len(), 0);
        assert_eq!(live.list("tA", 0)[0].title, "一");
        assert_eq!(live.list("tA", 0)[1].seq, 9);
    }

    #[test]
    fn non_share_events_are_noop() {
        let mut board = TaskShareBoard::default();
        let other = EventEnvelope::new(
            1,
            EventType::RuntimeStarted,
            "2026-09-10T00:00:00.000Z".into(),
            None,
            None,
            None,
            json!({"pid": 1, "version": "t", "started_at": "2026-09-10T00:00:00.000Z"}),
        );
        board.apply(&other);
        assert_eq!(board, TaskShareBoard::default());
    }
}
