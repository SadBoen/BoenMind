//! 应答 pending 表（契约台账 §1 面 7 + §4 断连恢复语义）。
//!
//! 对齐 DSH `api-proxy.ts`：approval 与 question 两个登记表共享一个 rpcId
//! 命名空间（UUID）；`respond` 按回显 rpcId 路由（approval 先、question 后）。
//! 登记点留扩展：未来审批/提问工具调用 `PendingRegistry::register_*` 即接入。
//! mux 重开基线重放仍 pending 的 `approval/requested` 与 `question/requested`
//! （rpcId 原样复用）。

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::rpc::ServerRequestFrame;

/// 一个待应答审批（对齐 DSH `PendingApproval`）。
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub rpc_id: String,
    pub session_id: String,
    pub approval_id: String,
    pub tool_name: String,
    pub call_id: Option<String>,
    pub reason: Option<String>,
}

/// 一个待应答提问（对齐 DSH `PendingQuestion`）。
#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub rpc_id: String,
    pub session_id: String,
    pub questions: Vec<Value>,
}

/// pending 登记表（approval 先、question 后）。
#[derive(Default)]
pub struct PendingRegistry {
    pub approvals: HashMap<String, PendingApproval>,
    pub questions: HashMap<String, PendingQuestion>,
}

impl PendingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_approval(
        &mut self,
        rpc_id: String,
        session_id: String,
        approval_id: String,
        tool_name: String,
        call_id: Option<String>,
        reason: Option<String>,
    ) {
        self.approvals.insert(
            rpc_id.clone(),
            PendingApproval {
                rpc_id,
                session_id,
                approval_id,
                tool_name,
                call_id,
                reason,
            },
        );
    }

    pub fn register_question(
        &mut self,
        rpc_id: String,
        session_id: String,
        questions: Vec<Value>,
    ) {
        self.questions.insert(
            rpc_id.clone(),
            PendingQuestion {
                rpc_id,
                session_id,
                questions,
            },
        );
    }

    /// approval/requested 帧（初始推送与 mux 重开重放共用）。
    pub fn approval_frame(&self, pending: &PendingApproval) -> ServerRequestFrame {
        let mut payload = json!({
            "sessionId": pending.session_id,
            "approvalId": pending.approval_id,
            "toolName": pending.tool_name,
        });
        if let Some(c) = &pending.call_id {
            payload["callId"] = json!(c);
        }
        if let Some(r) = &pending.reason {
            payload["reason"] = json!(r);
        }
        ServerRequestFrame::new(pending.rpc_id.clone(), "approval/requested", payload)
    }

    /// question/requested 帧（rpcId = 问题稳定逻辑 id）。
    pub fn question_frame(&self, pending: &PendingQuestion) -> ServerRequestFrame {
        ServerRequestFrame::new(
            pending.rpc_id.clone(),
            "question/requested",
            json!({
                "sessionId": pending.session_id,
                "questions": pending.questions,
            }),
        )
    }

    /// 整批匹配校验（对齐 DSH `matchesQuestions`）：
    /// sessionId 一致、答案数 = 问题数、逐项 id 一致、selected 唯一、
    /// multiSelect 约束、option label 集合、custom trim 非空。
    pub fn matches_questions(
        &self,
        pending: &PendingQuestion,
        session_id: &str,
        answers: &Value,
    ) -> bool {
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
            // 逐项 id 一致。
            if answer.get("id").and_then(Value::as_str)
                != question.get("id").and_then(Value::as_str)
            {
                return false;
            }
            let selected = match answer.get("selected").and_then(Value::as_array) {
                Some(s) => s,
                None => return false,
            };
            // selected 唯一性。
            let mut seen = std::collections::HashSet::new();
            for sel in selected {
                if let Some(s) = sel.as_str() {
                    if !seen.insert(s.to_string()) {
                        return false;
                    }
                }
            }
            // custom trim 非空。
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
                // 单选 + custom → 不可同时选；selected 最多一个。
                if has_custom && !selected.is_empty() {
                    return false;
                }
                if selected.len() > 1 {
                    return false;
                }
            }
            // option label 集合。
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
            selected.iter().all(|sel| {
                sel.as_str()
                    .map(|s| labels.contains(s))
                    .unwrap_or(false)
            })
        })
    }
}

/// AppState 持有的 pending 登记（内部锁）。
#[derive(Default)]
pub struct PendingState {
    inner: Mutex<PendingRegistry>,
}

impl PendingState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, PendingRegistry> {
        self.inner.lock().unwrap()
    }
}
