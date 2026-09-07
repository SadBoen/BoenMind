//! 资源谓词与 Provider 装配辅助(自 broker.rs 机械移入)。
use bm_contract::capability::GrantResource;
use std::sync::Arc;

pub(super) fn resource_matches(resource: &GrantResource, args: &serde_json::Value) -> bool {
    // 谓词语义 = 子集匹配(已列键相等即命中,未列键不受约束)。
    // 2026-09-07 架构评审 P1-5 复核:曾尝试改「精确键集匹配」,被 m9 审批
    // 流测试否决——审批抽屉刻意只把抽屉谓词(如 scope)写进 Grant,真实
    // 调用携带全量参数,精确匹配会打断审批主路径。「委派全参快照是否收紧
    // 为精确匹配」登记 BACKLOG 待设计裁决(需区分授权来源,涉 Grant 字段)。
    for (k, want) in &resource.args_predicates {
        match args.get(k) {
            Some(have) if json_scalar_eq(have, want) => {}
            _ => return false,
        }
    }
    true
}

fn json_scalar_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// 内置 Provider 测试/装配辅助:把闭包包装成 CapabilityProvider。
/// (正式内置能力集随 T5;此处供 Broker 测试与早期装配。)
pub fn provider_fn(
    f: impl Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync + 'static,
) -> Arc<dyn crate::registry::CapabilityProvider> {
    struct F(Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>);
    impl crate::registry::CapabilityProvider for F {
        fn invoke(&self, args: serde_json::Value) -> Result<serde_json::Value, String> {
            (self.0)(args)
        }
    }
    Arc::new(F(Box::new(f)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(preds: serde_json::Value) -> GrantResource {
        GrantResource {
            capability: "fs.write".into(),
            args_predicates: preds.as_object().cloned().unwrap_or_default(),
        }
    }

    // 子集匹配语义(见 resource_matches 注释:P1-5 精确匹配已被审批流否决)
    #[test]
    fn subset_semantics_listed_keys_must_match() {
        let g = grant(serde_json::json!({"path": "a"}));
        assert!(resource_matches(&g, &serde_json::json!({"path": "a"})));
        assert!(!resource_matches(&g, &serde_json::json!({"path": "b"})));
        assert!(!resource_matches(&g, &serde_json::json!({})));
    }

    #[test]
    fn empty_predicates_remain_tool_wide_grant() {
        let g = grant(serde_json::json!({}));
        assert!(resource_matches(
            &g,
            &serde_json::json!({"any": ["thing", 1]})
        ));
    }
}
