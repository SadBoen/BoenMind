//! 规范状态行 DTO(自 sqlite_state.rs 机械移入;借用字段纯数据结构)。

/// approvals 表行(载荷列 = Approval 合同 JSON 文本)。
pub struct ApprovalRow<'a> {
    pub id: &'a str,
    pub operation_id: &'a str,
    pub capability: &'a str,
    pub principal: &'a str,
    pub state: &'a str,
    pub payload: &'a str,
    pub created_at: &'a str,
    pub resolved_at: Option<&'a str>,
}

/// grants 表行(载荷列 = Grant 合同 JSON 文本)。
pub struct GrantRow<'a> {
    pub id: &'a str,
    pub audience: &'a str,
    pub action: &'a str,
    pub revocation_version: u64,
    pub revoked: bool,
    /// T6c 收紧(M5-T1):count 类 Grant 消费余量持久化,重启不再回满。
    pub used_count: u64,
    pub payload: &'a str,
    pub created_at: &'a str,
}

/// tasks 表行(载荷列 = task/task.v0.1 合同 JSON 文本)。
pub struct TaskRow<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub created_by: &'a str,
    pub task_epoch: u64,
    pub payload: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub parent_task_id: Option<&'a str>,
    pub delegation_depth: u64,
}

/// capabilities 表行(manifest 列 = Capability Manifest 合同 JSON 文本)。
pub struct CapabilityRow<'a> {
    pub capability: &'a str,
    pub provider_instance_id: &'a str,
    pub epoch: u64,
    pub status: &'a str,
    pub manifest: &'a str,
    pub updated_at: &'a str,
}
