//! credentials.* handler（领域子模块，api.rs 拆分）。
//! ref 名校验、credentials set/unset、`{ID}_API_KEY` 同步 provider key 覆盖。

use serde_json::{json, Value};

use crate::api::AppState;
use crate::rpc::{err, ok};

pub(super) fn credentials_describe(state: &AppState, payload: Value) -> Value {
    let refs = payload
        .get("refs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let store = state.credentials.lock().unwrap();
    let mut credentials = serde_json::Map::new();
    for r in refs {
        if let Some(name) = r.as_str() {
            credentials.insert(
                name.to_string(),
                json!({ "configured": store.contains_key(name), "writable": true }),
            );
        }
    }
    ok(json!({ "credentials": credentials }))
}

/// ref 名校验：`/^[A-Za-z_][A-Za-z0-9_]*$/`（台账 §2 credentials.*）。
pub(super) fn valid_credential_ref(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// credentials.set（特权）：ref + value(≥1) → {}。内存存储（持久化后置）。
/// ref 非法 → bad-request；value 空 → bad-request。
/// ref 形如 `{ID}_API_KEY` 时同步到同名 provider 适配器（对齐 DSH：key 经 credentials
/// 服务每请求解析，写后下一请求生效，无需重启）。
pub(super) fn credentials_set(state: &AppState, payload: Value) -> Value {
    let Some(name) = payload.get("ref").and_then(Value::as_str) else {
        return err("bad-request", "missing ref");
    };
    if !valid_credential_ref(name) {
        return err(
            "bad-request",
            "ref must match /^[A-Za-z_][A-Za-z0-9_]*$/",
        );
    }
    let Some(value) = payload.get("value").and_then(Value::as_str) else {
        return err("bad-request", "missing value");
    };
    if value.is_empty() {
        return err("bad-request", "value must be at least 1 character");
    }
    state.credentials.lock().unwrap().insert(name.to_string(), value.to_string());
    sync_provider_key_override(state, name, Some(value.to_string()));
    state.persist_settings();
    ok(json!({}))
}

/// credentials.unset（特权）：ref → {}（无引用也成功）。
pub(super) fn credentials_unset(state: &AppState, payload: Value) -> Value {
    let Some(name) = payload.get("ref").and_then(Value::as_str) else {
        return err("bad-request", "missing ref");
    };
    if !valid_credential_ref(name) {
        return err(
            "bad-request",
            "ref must match /^[A-Za-z_][A-Za-z0-9_]*$/",
        );
    }
    state.credentials.lock().unwrap().remove(name);
    sync_provider_key_override(state, name, None);
    state.persist_settings();
    ok(json!({}))
}

/// ref `{ID}_API_KEY`（大写 env 形态）命中同名 provider → 同步 key 覆盖（None = 恢复装配值/env）。
pub(super) fn sync_provider_key_override(state: &AppState, ref_name: &str, value: Option<String>) {
    let Some(suffix) = ref_name.strip_suffix("_API_KEY") else {
        return;
    };
    for p in &state.providers {
        if p.id.to_uppercase() == suffix {
            if let Some(adapter) = &p.adapter {
                adapter.set_api_key_override(value.clone());
            }
            return;
        }
    }
}
