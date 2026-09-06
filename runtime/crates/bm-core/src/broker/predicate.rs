//! 资源谓词与 Provider 装配辅助(自 broker.rs 机械移入)。
use bm_contract::capability::GrantResource;
use std::sync::Arc;

pub(super) fn resource_matches(resource: &GrantResource, args: &serde_json::Value) -> bool {
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
