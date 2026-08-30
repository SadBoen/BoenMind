//! Execution Log 条目镜像(logs/execution-log-entry.v0_1.schema.json)。
//! Event Log 记事实,Execution Log 记过程(基线 8.4);log_seq 由日志写者
//! 单调分配,仅在本日志内排序。

use crate::BmTimestamp;
use crate::ids::BmId;
use serde::{Deserialize, Serialize};

wire_str_enum!(LogKind {
    AgentTurn => "agent.turn",
    ModelInvocation => "model.invocation",
    BudgetCheck => "budget.check",
    Error => "error",
});

// 脱敏管线检查标记;schema const "passed"——未通过扫描的条目禁止落盘(INV-5)。
wire_str_enum!(SecretScan {
    Passed => "passed",
    // P0(第四轮评审)增发:复扫失败的条目以 failed 态降格落盘占位
    //(release 级 fail-closed;原文禁止落盘),不再依赖 debug_assert。
    Failed => "failed",
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub log_seq: u64,
    pub ts: BmTimestamp,
    pub kind: LogKind,
    pub session_id: BmId,
    pub agent_id: BmId,
    pub operation_id: BmId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<BmId>,
    /// 写入该条目时的 Agent 状态。
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_scan: Option<SecretScan>,
    pub detail: serde_json::Value,
}
