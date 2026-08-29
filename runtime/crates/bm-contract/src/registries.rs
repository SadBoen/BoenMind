//! 冻结合同文本的内嵌与解析访问。
//!
//! 路径相对本文件:src → bm-contract → crates → runtime → 仓库根。
//! CI/同步测试保证「内嵌副本 == 合同库文件」;合同库任何变更都会让同步测试变红。

use serde::Deserialize;

pub const ENVELOPE_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/wire/envelope.v0_1.schema.json");
pub const SESSION_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/wire/session.v0_1.schema.json");
pub const AGENT_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/wire/agent.v0_1.schema.json");
pub const CONNECTOR_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/model/connector.v0_1.schema.json");
pub const BUDGET_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/budget.v0_1.schema.json");
pub const EXEC_LOG_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/logs/execution-log-entry.v0_1.schema.json");
pub const ERROR_CODES_REGISTRY: &str =
    include_str!("../../../../boenmind-contracts/registry/error-codes.v0_1.json");
pub const RUNTIME_EVENTS_REGISTRY: &str =
    include_str!("../../../../boenmind-contracts/registry/runtime-events.v0_1.json");
pub const CORE_TRANSITIONS: &str =
    include_str!("../../../../boenmind-contracts/state-machines/core-transitions.v0_1.json");
// M4 增发(2026-08-29,Minor):capability.* 四合同 + wire/capability。
pub const CAPABILITY_MANIFEST_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/capability/manifest.v0_1.schema.json");
pub const CAPABILITY_GRANT_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/capability/grant.v0_1.schema.json");
pub const CAPABILITY_APPROVAL_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/capability/approval.v0_1.schema.json");
pub const CAPABILITY_LEASE_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/capability/lease.v0_1.schema.json");
pub const WIRE_CAPABILITY_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/wire/capability.v0_1.schema.json");
// M5 增发(2026-08-29,Minor):task 对象 + wire/task + memory 条目 + 观察日志条目。
pub const TASK_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/task/task.v0_1.schema.json");
pub const WIRE_TASK_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/wire/task.v0_1.schema.json");
pub const MEMORY_ENTRY_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/memory/memory-entry.v0_1.schema.json");
pub const OBSERVATION_LOG_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/logs/observation-log-entry.v0_1.schema.json");
// M7 增发(2026-08-30,Minor):MCP server 接入配置合同。
pub const MCP_SERVER_SCHEMA: &str =
    include_str!("../../../../boenmind-contracts/mcp/mcp-server.v0_1.schema.json");

#[derive(Debug, Deserialize)]
pub struct RegistryCode {
    pub code: String,
    #[serde(rename = "cli_exit")]
    pub cli_exit: i32,
    pub default_retryable: bool,
    pub available_since: String,
}

#[derive(Debug, Deserialize)]
pub struct RegistryEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct RegistryTransition {
    pub from: String,
    pub to: String,
    pub guard: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegistryMachine {
    pub states: Vec<String>,
    pub terminal: Vec<String>,
    pub transitions: Vec<RegistryTransition>,
}

#[derive(Debug, Deserialize)]
pub struct RegistryMachines {
    pub machines: std::collections::BTreeMap<String, RegistryMachine>,
}

/// 解析错误码注册表(保持文件顺序)。
pub fn error_codes() -> Vec<RegistryCode> {
    #[derive(Deserialize)]
    struct Root {
        codes: Vec<RegistryCode>,
    }
    serde_json::from_str::<Root>(ERROR_CODES_REGISTRY)
        .expect("error-codes 注册表必须是合法 JSON")
        .codes
}

/// 解析事件注册表(保持文件顺序)。
pub fn runtime_events() -> Vec<RegistryEvent> {
    #[derive(Deserialize)]
    struct Root {
        events: Vec<RegistryEvent>,
    }
    serde_json::from_str::<Root>(RUNTIME_EVENTS_REGISTRY)
        .expect("runtime-events 注册表必须是合法 JSON")
        .events
}

/// 解析状态迁移表。
pub fn core_transitions() -> RegistryMachines {
    serde_json::from_str(CORE_TRANSITIONS).expect("core-transitions 必须是合法 JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_contract_texts_are_valid_json() {
        for (name, text) in [
            ("envelope", ENVELOPE_SCHEMA),
            ("session", SESSION_SCHEMA),
            ("agent", AGENT_SCHEMA),
            ("connector", CONNECTOR_SCHEMA),
            ("budget", BUDGET_SCHEMA),
            ("exec-log", EXEC_LOG_SCHEMA),
            ("error-codes", ERROR_CODES_REGISTRY),
            ("runtime-events", RUNTIME_EVENTS_REGISTRY),
            ("core-transitions", CORE_TRANSITIONS),
            ("capability-manifest", CAPABILITY_MANIFEST_SCHEMA),
            ("capability-grant", CAPABILITY_GRANT_SCHEMA),
            ("capability-approval", CAPABILITY_APPROVAL_SCHEMA),
            ("capability-lease", CAPABILITY_LEASE_SCHEMA),
            ("wire-capability", WIRE_CAPABILITY_SCHEMA),
            ("task", TASK_SCHEMA),
            ("wire-task", WIRE_TASK_SCHEMA),
            ("memory-entry", MEMORY_ENTRY_SCHEMA),
            ("observation-log", OBSERVATION_LOG_SCHEMA),
        ] {
            let v: serde_json::Value =
                serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(v.is_object());
        }
    }
}
