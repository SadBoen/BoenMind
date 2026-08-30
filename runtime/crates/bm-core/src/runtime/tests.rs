//! runtime 内嵌测试(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

#[cfg(test)]
mod t7_event_shape_tests {
    use super::super::*;

    /// T7 硬约束 3:命令语义形状在持久化前拒绝(G1 Bus 不得当 RPC)。
    #[test]
    fn command_semantic_payloads_are_rejected_before_persist() {
        let ty = EventType::SessionCreated;
        for bad_key in [
            "requested_action",
            "instruction",
            "command",
            "please_execute",
        ] {
            let payload = serde_json::json!({ bad_key: {"op": "mail.send"} });
            assert!(
                validate_event_shape(&ty, &payload).is_err(),
                "{bad_key} 形状必须被拒"
            );
        }
        // 正常事实载荷照常通过
        assert!(validate_event_shape(&ty, &serde_json::json!({"session_id": "x"})).is_ok());
    }
}
