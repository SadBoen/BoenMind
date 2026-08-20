//! llm.* handler 领域子模块（api.rs 拆分）。
//! 模型 provider 列表/模型清单/探测器（mock 与真 provider 双模式）。

use serde_json::{json, Value};

use crate::api::AppState;
use crate::rpc::{err_with_details, ok};
use super::{mock_model_group, model_groups};

pub(super) async fn llm_providers(state: &AppState) -> Value {
    if state.providers.is_empty() {
        // M1：单一 mock provider。
        return ok(json!({
            "providers": [{
                "provider": state.runtime.provider,
                "displayName": "Mock",
                "settingsNs": "llm.mock",
                "settingsPath": ["llm", "mock"],
                "active": true,
            }]
        }));
    }
    let providers: Vec<Value> = state
        .providers
        .iter()
        .map(|p| {
            json!({
                "provider": p.id,
                "displayName": p.display_name,
                "settingsNs": p.settings_ns,
                "settingsPath": p.settings_path(),
                "active": true,
            })
        })
        .collect();
    ok(json!({ "providers": providers }))
}

pub(super) async fn llm_models(state: &AppState) -> Value {
    if state.providers.is_empty() {
        // mock 模式：单 mock 组。
        return ok(json!({
            "groups": [mock_model_group()],
            "failures": []
        }));
    }
    ok(json!({
        "groups": model_groups(state),
        "failures": []
    }))
}

/// llm.discoverModels（特权）：真实探测。settingsNs 匹配到已装配 provider →
/// 用其 API 请求模型列表端点；不匹配/失败 → `model-discovery-failed`。
pub(super) async fn llm_discover_models(state: &AppState, payload: Value) -> Value {
    let settings_ns = payload
        .get("settingsNs")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(provider) = state.providers.iter().find(|p| p.settings_ns == settings_ns) else {
        // 无配置（mock 模式）或未知 ns：回退 mock 已知 ns。
        if state.providers.is_empty() && (settings_ns == "llm.mock" || settings_ns.is_empty()) {
            return ok(json!({
                "models": [{ "id": "mock-1", "name": "Mock 1" }]
            }));
        }
        return err_with_details(
            "model-discovery-failed",
            "no provider for this settings namespace",
            json!({ "settingsNs": settings_ns }),
        );
    };
    let Some(adapter) = &provider.adapter else {
        return err_with_details(
            "model-discovery-failed",
            "provider has no discovery endpoint",
            json!({ "settingsNs": settings_ns }),
        );
    };
    match adapter.list_models_remote().await {
        Ok(models) => ok(json!({
            "models": models.iter().map(|m| {
                let mut v = json!({
                    "id": m.id,
                    "name": m.label.clone().unwrap_or_else(|| m.id.clone()),
                });
                // contextWindow/maxTokens 仅已知时带（schema `.optional()`，未知省略）。
                if let Some(c) = m.context_window {
                    v["contextWindow"] = json!(c);
                }
                if let Some(t) = m.max_tokens {
                    v["maxTokens"] = json!(t);
                }
                v
            }).collect::<Vec<_>>()
        })),
        Err(e) => err_with_details(
            "model-discovery-failed",
            e.message,
            json!({ "settingsNs": settings_ns, "baseURL": provider.base_url }),
        ),
    }
}
