//! # plugin-approval —— 工具审批中心插件（功能分类）。
//!
//! 万物皆插件②（2026-08-22 从 web-server approval.rs/pending.rs 下沉）：
//! - [`ApprovalCenter`] 实现 [`bm_ports::ToolApprovalPort`]（loop 消费面）：
//!   危险工具执行前登记 pending → 广播 approval/requested（前端弹窗）→
//!   等用户裁定（600s 超时拒绝）。
//! - 同时实现 [`bm_ports::ApprovalFacePort`]（宿主委托面）：`POST /api/respond`
//!   路由（approval 先、question 后）、mux 断连重放、测试钩子登记。
//! - 状态自带（pending 表 + 等待表）；广播经 [`bm_ports::BroadcastPort`] 消费。
//!
//! fail-loud：登记/广播失败 → Err（loop 按拒绝处理，不静默放行危险工具）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bm_ports::{
    ApprovalFacePort, ApprovalVerdict, BroadcastPort, MuxFrameOut, ToolApprovalPort,
    APPROVAL_TIMEOUT,
};
use serde_json::{json, Value};

pub mod plugin;

pub use plugin::manifest;

/// 一个待应答审批。
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub rpc_id: String,
    pub session_id: String,
    pub approval_id: String,
    pub tool_name: String,
    pub call_id: Option<String>,
    pub reason: Option<String>,
}

/// 一个待应答提问。
#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub rpc_id: String,
    pub session_id: String,
    pub questions: Vec<Value>,
}

/// approval/requested 帧（初始推送与 mux 重开重放共用）。
fn approval_frame(p: &PendingApproval) -> MuxFrameOut {
    let mut payload = json!({
        "type": "approval/requested",
        "sessionId": p.session_id,
        "approvalId": p.approval_id,
        "toolName": p.tool_name,
    });
    if let Some(c) = &p.call_id {
        payload["callId"] = json!(c);
    }
    if let Some(r) = &p.reason {
        payload["reason"] = json!(r);
    }
    MuxFrameOut {
        rpc_id: p.rpc_id.clone(),
        method: "approval/requested".to_string(),
        payload,
    }
}

/// question/requested 帧（rpcId = 问题稳定逻辑 id）。
fn question_frame(p: &PendingQuestion) -> MuxFrameOut {
    MuxFrameOut {
        rpc_id: p.rpc_id.clone(),
        method: "question/requested".to_string(),
        payload: json!({
            "type": "question/requested",
            "sessionId": p.session_id,
            "questions": p.questions,
        }),
    }
}

/// 整批匹配校验（对齐 DSH `matchesQuestions`）：sessionId 一致、答案数 = 问题数、
/// 逐项 id 一致、selected 唯一、multiSelect 约束、option label 集合、custom 非空。
fn matches_questions(pending: &PendingQuestion, session_id: &str, answers: &Value) -> bool {
    if session_id != pending.session_id {
        return false;
    }
    let answers = match answers.as_array() {
        Some(a) => a,
        None => return false,
    };
    if answers.len() != pending.questions.len() {
        return false;
    }
    answers.iter().enumerate().all(|(i, answer)| {
        let question = &pending.questions[i];
        if answer.get("id").and_then(Value::as_str) != question.get("id").and_then(Value::as_str) {
            return false;
        }
        let selected = match answer.get("selected").and_then(Value::as_array) {
            Some(s) => s,
            None => return false,
        };
        let mut seen = std::collections::HashSet::new();
        for sel in selected {
            if let Some(s) = sel.as_str() {
                if !seen.insert(s.to_string()) {
                    return false;
                }
            }
        }
        if let Some(custom) = answer.get("custom").and_then(Value::as_str) {
            if custom.trim().is_empty() {
                return false;
            }
        }
        let multi_select = question
            .get("multiSelect")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let has_custom = answer.get("custom").is_some();
        if !multi_select {
            if has_custom && !selected.is_empty() {
                return false;
            }
            if selected.len() > 1 {
                return false;
            }
        }
        let labels: std::collections::HashSet<String> = question
            .get("options")
            .and_then(Value::as_array)
            .map(|opts| {
                opts.iter()
                    .filter_map(|o| o.get("label").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        selected
            .iter()
            .all(|sel| sel.as_str().map(|s| labels.contains(s)).unwrap_or(false))
    })
}

/// 审批中心（pending 表 + 等待表自带；广播经端口；每实例独立）。
pub struct ApprovalCenter {
    broadcast: Arc<dyn BroadcastPort>,
    /// approval 先、question 后（共享 rpcId 命名空间）。
    approvals: Mutex<HashMap<String, PendingApproval>>,
    questions: Mutex<HashMap<String, PendingQuestion>>,
    /// approval_id → oneshot 发送端：respond 唤醒等待中的 loop 调用。
    waiters: Mutex<HashMap<String, tokio::sync::oneshot::Sender<ApprovalVerdict>>>,
}

impl std::fmt::Debug for ApprovalCenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalCenter").finish_non_exhaustive()
    }
}

impl ApprovalCenter {
    pub fn new(broadcast: Arc<dyn BroadcastPort>) -> Self {
        Self {
            broadcast,
            approvals: Mutex::new(HashMap::new()),
            questions: Mutex::new(HashMap::new()),
            waiters: Mutex::new(HashMap::new()),
        }
    }

    /// 由 respond 路由（allowed-once/rejected）解析审批：把裁定交还等待中的 loop。
    /// 找不到等待者（超时已清理/重复应答）→ 忽略。
    fn resolve_waiter(&self, approval_id: &str, verdict: ApprovalVerdict) {
        if let Some(tx) = self.waiters.lock().unwrap().remove(approval_id) {
            let _ = tx.send(verdict);
        }
    }

    /// 广播一帧 mux（rpcId 透传）。
    fn send_frame(&self, frame: MuxFrameOut) {
        self.broadcast
            .broadcast_mux(frame.rpc_id, &frame.method, frame.payload);
    }
}

#[async_trait]
impl ToolApprovalPort for ApprovalCenter {
    async fn request_approval(
        &self,
        session_id: &str,
        tool_name: &str,
        call_id: &str,
        reason: Option<String>,
    ) -> Result<ApprovalVerdict, kernel_contracts::ToolError> {
        let approval_id = uuid::Uuid::new_v4().to_string();
        let rpc_id = uuid::Uuid::new_v4().to_string();
        // 审批帧带发起会话 id：前端豁免表按 (sessionId, toolName) 区分会话。
        let pending = PendingApproval {
            rpc_id: rpc_id.clone(),
            session_id: session_id.to_string(),
            approval_id: approval_id.clone(),
            tool_name: tool_name.to_string(),
            call_id: Some(call_id.to_string()),
            reason,
        };

        // 1. 登记进审批表（approval/requested 帧数据源；mux 断连重放）。
        self.approvals.lock().unwrap().insert(rpc_id.clone(), pending);

        // 2. 建 oneshot 等待通道。
        let (tx, rx) = tokio::sync::oneshot::channel::<ApprovalVerdict>();
        self.waiters.lock().unwrap().insert(approval_id.clone(), tx);

        // 3. 广播 approval/requested（前端弹窗触发）。
        if let Some(p) = self.approvals.lock().unwrap().get(&rpc_id).cloned() {
            self.send_frame(approval_frame(&p));
        }

        // 4. 等待裁定（超时 → 拒绝；通道被 drop/异常 → 拒绝）。
        let verdict: ApprovalVerdict = match tokio::time::timeout(APPROVAL_TIMEOUT, rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => ApprovalVerdict::Rejected,
            Err(_) => {
                // 超时：从审批表移除（不再可应答）。
                self.approvals.lock().unwrap().remove(&rpc_id);
                ApprovalVerdict::Rejected
            }
        };

        // 5. 无论何种裁定，清理等待表（防泄漏）。
        self.waiters.lock().unwrap().remove(&approval_id);
        Ok(verdict)
    }
}

impl ApprovalFacePort for ApprovalCenter {
    fn respond(&self, rpc_id: &str, result: &serde_json::Value) -> (bool, Option<&'static str>) {
        // ---- approval 表先查（先取快照再进 if：scrutinee 的锁 guard 会活到
        //      if-let 块尾，块内再锁同表 = 自死锁）----
        let approval_pending = self.approvals.lock().unwrap().get(rpc_id).cloned();
        if let Some(pending) = approval_pending {
            if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                return (false, Some("bad-response"));
            }
            let value = result.get("value").cloned().unwrap_or(Value::Null);
            // 应答负载须 {sessionId, approvalId, outcome:'allowed-once'|'rejected'}
            // 且 approvalId/sessionId 与登记一致（对齐 approvals.schema.ts）。
            let outcome = value.get("outcome").and_then(Value::as_str);
            let ok_outcome = matches!(outcome, Some("allowed-once") | Some("rejected"));
            let matches = value.get("sessionId").and_then(Value::as_str)
                == Some(pending.session_id.as_str())
                && value.get("approvalId").and_then(Value::as_str) == Some(pending.approval_id.as_str())
                && ok_outcome;
            if !matches {
                return (false, Some("bad-response"));
            }
            self.approvals.lock().unwrap().remove(rpc_id);
            let session_id = pending.session_id.clone();
            let approval_id = pending.approval_id.clone();
            let outcome = outcome.unwrap_or("rejected").to_string();
            // 唤醒等待中的 loop 审批调用（allowed-once ↔ Allowed / rejected ↔ Rejected）。
            let verdict = match outcome.as_str() {
                "allowed-once" => ApprovalVerdict::Allowed,
                _ => ApprovalVerdict::Rejected,
            };
            self.resolve_waiter(&approval_id, verdict);
            // 纯推送：approval/resolved。
            self.broadcast.broadcast_mux(
                uuid::Uuid::new_v4().to_string(),
                "approval/resolved",
                json!({
                    "type": "approval/resolved",
                    "sessionId": session_id,
                    "approvalId": approval_id,
                    "outcome": outcome,
                }),
            );
            return (true, None);
        }
        // ---- question 表后查 ----
        let Some(pending) = self.questions.lock().unwrap().get(rpc_id).cloned() else {
            return (false, Some("not-pending"));
        };
        let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if !ok {
            // result.ok:false && error.code==='cancelled' → accepted（用户取消）。
            let code = result
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if code != "cancelled" {
                return (false, Some("bad-response"));
            }
            self.questions.lock().unwrap().remove(rpc_id);
            self.broadcast.broadcast_mux(
                uuid::Uuid::new_v4().to_string(),
                "question/resolved",
                json!({
                    "type": "question/resolved",
                    "sessionId": pending.session_id,
                    "questionRpcId": pending.rpc_id,
                    "outcome": "cancelled",
                }),
            );
            return (true, None);
        }
        let value = result.get("value").cloned().unwrap_or(Value::Null);
        let session_id = value.get("sessionId").and_then(Value::as_str).unwrap_or("");
        let answers = value
            .get("answer")
            .and_then(|a| a.get("answers"))
            .cloned()
            .unwrap_or(Value::Null);
        if !matches_questions(&pending, session_id, &answers) {
            return (false, Some("bad-response"));
        }
        self.questions.lock().unwrap().remove(rpc_id);
        self.broadcast.broadcast_mux(
            uuid::Uuid::new_v4().to_string(),
            "question/resolved",
            json!({
                "type": "question/resolved",
                "sessionId": pending.session_id,
                "questionRpcId": pending.rpc_id,
                "outcome": "answered",
            }),
        );
        (true, None)
    }

    fn pending_frames(&self) -> Vec<MuxFrameOut> {
        let mut frames: Vec<MuxFrameOut> = self
            .approvals
            .lock()
            .unwrap()
            .values()
            .map(approval_frame)
            .collect();
        frames.extend(self.questions.lock().unwrap().values().map(question_frame));
        frames
    }

    fn register_test_approval(
        &self,
        rpc_id: String,
        session_id: String,
        approval_id: String,
        tool_name: String,
        call_id: Option<String>,
        reason: Option<String>,
    ) {
        let pending = PendingApproval {
            rpc_id,
            session_id,
            approval_id,
            tool_name,
            call_id,
            reason,
        };
        // 对齐 request_approval 的推送语义：登记 + 广播 requested 帧。
        self.send_frame(approval_frame(&pending));
        self.approvals.lock().unwrap().insert(pending.rpc_id.clone(), pending);
    }

    fn register_test_question(&self, rpc_id: String, session_id: String, questions: Value) {
        let pending = PendingQuestion {
            rpc_id: rpc_id.clone(),
            session_id,
            questions: questions
                .as_array()
                .cloned()
                .unwrap_or_default(),
        };
        self.questions.lock().unwrap().insert(rpc_id, pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// 桩广播：记录 mux 帧。
    #[derive(Debug, Default)]
    struct StubBroadcast {
        frames: StdMutex<Vec<(String, String, Value)>>,
    }
    impl BroadcastPort for StubBroadcast {
        fn broadcast_host(&self, _method: &str, _payload: Value) {}
        fn broadcast_mux(&self, rpc_id: String, method: &str, payload: Value) {
            self.frames.lock().unwrap().push((rpc_id, method.to_string(), payload));
        }
        fn write_projection(&self, _session_id: &str, _key: &str, _value: Value) {}
    }

    fn center() -> (Arc<ApprovalCenter>, Arc<StubBroadcast>) {
        let b = Arc::new(StubBroadcast::default());
        let c = Arc::new(ApprovalCenter::new(
            Arc::clone(&b) as Arc<dyn BroadcastPort>,
        ));
        (c, b)
    }

    #[test]
    fn manifest_category_feature() {
        use kernel_contracts::plugin::PluginCategory;
        assert_eq!(manifest().category, PluginCategory::Feature);
    }

    /// approval 全链路：登记 → requested 帧 → respond 校验（sessionId/approvalId
    /// 不匹配拒绝）→ resolved 帧 → 等待者被唤醒。
    #[tokio::test(flavor = "current_thread")]
    async fn approval_respond_roundtrip() {
        let (c, b) = center();
        // 先登记等待者（模拟 loop 在等）。
        let (tx, rx) = tokio::sync::oneshot::channel::<ApprovalVerdict>();
        c.waiters.lock().unwrap().insert("a1".to_string(), tx);
        c.register_test_approval(
            "rpc-1".to_string(),
            "s1".to_string(),
            "a1".to_string(),
            "host.run_command".to_string(),
            Some("c9".to_string()),
            Some("危险命令".to_string()),
        );
        // requested 帧已广播（payload 带 type 判别字段）。
        assert_eq!(b.frames.lock().unwrap().len(), 1);
        assert_eq!(b.frames.lock().unwrap()[0].1, "approval/requested");

        // 不匹配的应答 → bad-response，pending 保留。
        let bad = json!({ "ok": true, "value": { "sessionId": "sX", "approvalId": "a1", "outcome": "allowed-once" } });
        assert_eq!(c.respond("rpc-1", &bad), (false, Some("bad-response")));

        // 正确应答 → accepted + resolved 帧 + 等待者拿到 Allowed。
        let good = json!({ "ok": true, "value": { "sessionId": "s1", "approvalId": "a1", "outcome": "allowed-once" } });
        assert_eq!(c.respond("rpc-1", &good), (true, None));
        assert_eq!(rx.await.unwrap(), ApprovalVerdict::Allowed);
        let frames = b.frames.lock().unwrap();
        assert_eq!(frames[1].1, "approval/resolved");
        assert_eq!(frames[1].2["outcome"], "allowed-once");
        // 二次应答 → not-pending。
        drop(frames);
        assert_eq!(c.respond("rpc-1", &good), (false, Some("not-pending")));
    }

    /// question 校验：answers 形状不对 → bad-response；正确 → answered。
    #[test]
    fn question_respond_validation() {
        let (c, _b) = center();
        let questions = json!([
            { "id": "q1", "options": [ { "label": "A" }, { "label": "B" } ], "multiSelect": false }
        ]);
        c.register_test_question("rpc-q".to_string(), "s1".to_string(), questions);
        // 答案数不对。
        let bad = json!({ "ok": true, "value": { "sessionId": "s1", "answer": { "answers": [] } } });
        assert_eq!(c.respond("rpc-q", &bad), (false, Some("bad-response")));
        // label 不在选项集。
        let bad2 = json!({ "ok": true, "value": { "sessionId": "s1", "answer": { "answers": [ { "id": "q1", "selected": ["Z"] } ] } } });
        assert_eq!(c.respond("rpc-q", &bad2), (false, Some("bad-response")));
        // 正确单选。
        let good = json!({ "ok": true, "value": { "sessionId": "s1", "answer": { "answers": [ { "id": "q1", "selected": ["A"] } ] } } });
        assert_eq!(c.respond("rpc-q", &good), (true, None));
        // 取消路径。
        c.register_test_question("rpc-q2".to_string(), "s1".to_string(), json!([]));
        let cancelled = json!({ "ok": false, "error": { "code": "cancelled" } });
        assert_eq!(c.respond("rpc-q2", &cancelled), (true, None));
    }

    /// 重放帧：approval 与 question 都在列（rpcId 复用）。
    #[test]
    fn pending_frames_replay() {
        let (c, _b) = center();
        c.register_test_approval("rpc-a".into(), "s1".into(), "a1".into(), "t".into(), None, None);
        c.register_test_question("rpc-q".into(), "s1".into(), json!([]));
        let frames = c.pending_frames();
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().any(|f| f.rpc_id == "rpc-a" && f.method == "approval/requested"));
        assert!(frames.iter().any(|f| f.rpc_id == "rpc-q" && f.method == "question/requested"));
    }
}
