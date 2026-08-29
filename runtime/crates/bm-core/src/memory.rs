//! memory.* 能力(M5.7,基线 §4.1;ADR-0002 条件 5 余项)。
//!
//! Memory 是一等合同对象:作用域即权限边界(memory:app:<app> / task:<id> /
//! agent:<id> / user)。读写检索删除都以普通 Capability Provider 身份注册
//! (memory.write / memory.search / memory.delete),全部经 Broker 统一裁决
//! ——无特权通道。阶段一检索 FTS5(libsqlite3-sys bundled 缺 FTS5 编译特性
//! 时自动 LIKE 兜底,接口可替换);默认不自动写长期记忆;用户纠正覆盖而非
//! 追加(correction_of 即时墓碑化);来源被删除时记忆级联失效(墓碑)。
//!
//! memory:user 的「显式授权」执行面(区分 Surface 直写与 Agent 写)随 M7
//! principal-aware Provider;M5 面:scope 形态校验 + Broker 信任分级适用。

use bm_contract::capability::CapabilityManifest;
use serde_json::json;
use std::sync::Arc;

/// scope 形态校验(与 memory/memory-entry.v0.1 合同 pattern 同源):
/// memory:app:<name> | memory:task:<id> | memory:agent:<ulid26> | memory:user。
/// 作用域即权限边界——形态外的域一律拒绝(测试 t88)。
pub fn scope_ok(scope: &str) -> bool {
    let Some(rest) = scope.strip_prefix("memory:") else {
        return false;
    };
    if rest == "user" {
        return true;
    }
    if let Some(app) = rest.strip_prefix("app:") {
        return !app.is_empty()
            && app.len() <= 32
            && app.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && app
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    }
    if let Some(t) = rest.strip_prefix("task:") {
        return !t.is_empty() && t.len() <= 64;
    }
    if let Some(a) = rest.strip_prefix("agent:") {
        return a.len() == 26 && a.chars().all(|c| c.is_ascii_alphanumeric());
    }
    false
}

/// 注册 memory.* 三能力(普通 Provider 身份;调用方把返回值并入
/// RuntimeConfig.capabilities,store 与 Runtime 共享同一 EventStore)。
pub fn memory_capabilities(
    store: Arc<dyn bm_persist::EventStore>,
    ids: Arc<dyn bm_contract::ids::IdGen>,
) -> Vec<(
    CapabilityManifest,
    Arc<dyn crate::registry::CapabilityProvider>,
)> {
    fn manifest(name: &str, effect: &str) -> CapabilityManifest {
        serde_json::from_value(json!({
            "capability": name, "provider": "memory", "version": "0.1.0",
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "effect": effect, "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .expect("memory manifest 合法")
    }

    let write_store = store.clone();
    let write = crate::broker::provider_fn(move |args: serde_json::Value| {
        let scope = args["scope"].as_str().unwrap_or_default().to_string();
        let content_ref = args["content_ref"].as_str().unwrap_or_default().to_string();
        if !scope_ok(&scope) {
            return Err(format!("非法记忆作用域(作用域即权限边界): {scope}"));
        }
        if content_ref.is_empty() {
            return Err("content_ref 必填".into());
        }
        let entry_id = ids.next_id("mem").to_string();
        let preview = args["content_preview"].as_str().map(|s| s.to_string());
        let trust = args["source_trust"]
            .as_str()
            .unwrap_or("trusted")
            .to_string();
        let source_ref = args["source_ref"].as_str().map(|s| s.to_string());
        let correction_of = args["correction_of"].as_str().map(|s| s.to_string());
        write_store
            .memory_put(
                &entry_id,
                &scope,
                &content_ref,
                preview.as_deref(),
                &trust,
                source_ref.as_deref(),
                correction_of.as_deref(),
                &json!({"entry_id": entry_id, "scope": scope}).to_string(),
                &bm_contract::timestamp::now(),
            )
            .map_err(|e| format!("记忆写入失败: {e}"))?;
        Ok(json!({"entry_id": entry_id, "scope": scope}))
    });

    let search_store = store.clone();
    let search = crate::broker::provider_fn(move |args: serde_json::Value| {
        let scope = args["scope"].as_str().unwrap_or_default().to_string();
        if !scope_ok(&scope) {
            return Err(format!("非法记忆作用域: {scope}"));
        }
        let query = args["query"].as_str().unwrap_or_default().to_string();
        let entries = search_store
            .memory_search(&scope, &query)
            .map_err(|e| format!("记忆检索失败: {e}"))?;
        Ok(json!({"entries": entries, "count": entries.len()}))
    });

    let delete_store = store;
    let delete = crate::broker::provider_fn(move |args: serde_json::Value| {
        let entry_id = args["entry_id"].as_str().unwrap_or_default().to_string();
        if entry_id.is_empty() {
            return Err("entry_id 必填".into());
        }
        let cascaded = delete_store
            .memory_delete(&entry_id)
            .map_err(|e| format!("记忆删除失败: {e}"))?;
        Ok(json!({"entry_id": entry_id, "tombstoned": true, "cascaded": cascaded}))
    });

    vec![
        (manifest("memory.write", "low-risk-command"), write),
        (manifest("memory.search", "read-only"), search),
        (manifest("memory.delete", "reversible-command"), delete),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_register_under_memory_namespace() {
        let dir = tempfile::tempdir().expect("临时目录");
        let store: Arc<dyn bm_persist::EventStore> =
            Arc::new(bm_persist::PersistStore::open(dir.path()).expect("打开"));
        let ids = Arc::new(bm_contract::ids::SeqIdGen::new());
        let caps = memory_capabilities(store, ids);
        let names: Vec<&str> = caps.iter().map(|(m, _)| m.capability.as_str()).collect();
        assert_eq!(names, ["memory.write", "memory.search", "memory.delete"]);
    }
}
