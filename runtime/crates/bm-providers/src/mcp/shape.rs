//! MCP 数据形状(自 mcp.rs 机械移出;ADR-0048)。纯数据/纯工具,无传输依赖。

use super::*;
use bm_contract::capability::{
    ApprovalRequirement, ExecutionMode, ManifestSpec, RiskClass,
};

// ---- 数据形状 --------------------------------------------------------------

/// tools/list 条目(发现面)。
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    /// 工具功能描述(ADR-0022:进 manifest.description 面向模型展示)。
    pub description: Option<String>,
    /// 工具 inputSchema(MCP JSON Schema;直通 manifest.input_schema)。
    pub input_schema: Value,
    /// MCP annotations(readOnlyHint / destructiveHint → effect 映射)。
    pub annotations: Value,
}

/// 服务端进度通知(notifications/progress 解析结果)。
#[derive(Debug, Clone)]
pub struct McpProgressNote {
    pub progress_token: String,
    pub progress: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// 工具名规范化:仅 `.` 分段;段内小写、连字符归一为下划线;
/// 任一段不匹配能力名段字符集 `^[a-z][a-z0-9_]*$` → None(拒注册)。
pub fn normalize_tool_name(tool: &str) -> Option<String> {
    let mut out = String::new();
    for raw in tool.split('.') {
        let seg = raw.to_ascii_lowercase().replace('-', "_");
        let ok = !seg.is_empty()
            && seg.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && seg
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !ok {
            return None;
        }
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&seg);
    }
    Some(out)
}

/// server 名归一(冻结能力名段字符集):「mcp.<server>」前缀组合的唯一真源,
/// capability/provider/路由前缀三处同用——连字符等非法字符原样拼进能力名,
/// 会撞冻结合同的 capability pattern(注册期门禁会拒)。不合法 → None,
/// 与工具名非法同口径(跳过/未连接)。
pub fn normalize_server_name(server: &str) -> Option<String> {
    normalize_tool_name(server)
}

/// annotations → effect/approval 映射(M7 规格 S3;GT-05 形态):
/// readOnlyHint → read-only + not-required;destructiveHint →
/// external-side-effect + required;缺省 reversible-command + required
/// (未知风险首调审批,M7.7)。
pub fn tool_manifest(
    server: &str,
    tool: &McpToolDef,
    timeout_ms: u64,
) -> Option<CapabilityManifest> {
    let server_norm = normalize_server_name(server)?;
    let tool_norm = normalize_tool_name(&tool.name)?;
    let read_only = tool
        .annotations
        .get("readOnlyHint")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let destructive = tool
        .annotations
        .get("destructiveHint")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // 外部审计 X-05(P2):冲突标注裁决——destructiveHint 优先(第三方
    // 元数据只能提高风险、不能降低)。readOnly+destructive 并存 → 按
    // external-side-effect + required 注册,绝不降级为免审批只读。
    let read_only = read_only && !destructive;
    let effect = if destructive {
        RiskClass::ExternalSideEffect
    } else if read_only {
        RiskClass::ReadOnly
    } else {
        RiskClass::ReversibleCommand
    };
    let approval = if read_only {
        ApprovalRequirement::NotRequired
    } else {
        ApprovalRequirement::Required
    };
    let input_schema = if tool.input_schema.is_null() || tool.input_schema == json!({}) {
        json!({"type": "object"})
    } else {
        tool.input_schema.clone()
    };
    // ADR-0051:走全仓单一 manifest 合成路径(缺省单源)。
    let mut spec =
        ManifestSpec::new(format!("mcp.{server_norm}.{tool_norm}"), format!("mcp.{server_norm}"), effect)
            .input_schema(input_schema)
            .cancellable(true)
            .timeout_ms(timeout_ms)
            .approval(approval)
            .scopes(vec![format!("domain:mcp.{server_norm}")])
            .execution_mode(ExecutionMode::Async);
    // ADR-0022:工具自描述进 manifest,对话工具清单不再丢描述。
    if let Some(d) = tool
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        spec = spec.description(d);
    }
    spec.build().ok()
}

#[cfg(test)]
mod x05_tests {
    use super::*;
    use serde_json::json;

    fn def(name: &str, annotations: serde_json::Value) -> McpToolDef {
        McpToolDef {
            name: name.into(),
            description: None,
            input_schema: json!({"type": "object"}),
            annotations,
        }
    }

    /// X-05:readOnly+destructive 并存 → external-side-effect + required
    /// (元数据只能提高风险,不能降级为免审批只读)。
    #[test]
    fn conflicting_annotations_escalate() {
        let m = tool_manifest(
            "srv",
            &def("t", json!({"readOnlyHint": true, "destructiveHint": true})),
            1000,
        )
        .expect("manifest");
        assert_eq!(m.effect.as_str(), "external-side-effect");
        assert_eq!(
            m.approval,
            bm_contract::capability::ApprovalRequirement::Required
        );
    }

    #[test]
    fn read_only_only_stays_passthrough() {
        let m =
            tool_manifest("srv", &def("t", json!({"readOnlyHint": true})), 1000).expect("manifest");
        assert_eq!(m.effect.as_str(), "read-only");
        assert_eq!(
            m.approval,
            bm_contract::capability::ApprovalRequirement::NotRequired
        );
    }
}
