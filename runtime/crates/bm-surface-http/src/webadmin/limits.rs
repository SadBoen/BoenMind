//! W10 运行时限制面(ADR-0024):逐键读 + 全量快照写(热生效)。

use super::{AdminConfig, internal, respond_or_fail};
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

/// GET /admin/limits(W10/ADR-0024):逐键返回 当前值/默认值/区间/来源。
pub async fn limits_get(State(cfg): State<AdminConfig>) -> Response {
    let limits = cfg.limits.get();
    let defaults = bm_core::limits::Limits::default();
    let source_of = |k: &str| cfg.limits_sources.lock().expect("锁未中毒").source_of(k);
    let all: Value = serde_json::to_value(&limits).unwrap_or(json!({}));
    let dfl: Value = serde_json::to_value(&defaults).unwrap_or(json!({}));
    let keys: Vec<Value> = bm_core::limits::KEY_META
        .iter()
        .map(|m| {
            json!({
                "key": m.key,
                "group": m.group,
                "label": m.label,
                "min": m.min,
                "max": m.max,
                "editable": m.editable,
                "value": all.get(m.key).cloned().unwrap_or(Value::Null),
                "default": dfl.get(m.key).cloned().unwrap_or(Value::Null),
                "source": source_of(m.key),
            })
        })
        .collect();
    Json(json!({ "ok": true, "keys": keys })).into_response()
}

/// PUT /admin/limits(W10/ADR-0024):全量快照写——body {values: {key: value}},
/// 仅接受已登记键;钳制→原子写 config/limits.json→更新 Cell(热生效)。
pub async fn limits_put(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let incoming = body
        .get("values")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let editable: std::collections::HashSet<&str> = bm_core::limits::KEY_META
        .iter()
        .filter(|m| m.editable)
        .map(|m| m.key)
        .collect();
    // 以默认值为底、只叠加登记键(整数化浮点,避免 u64 反序列化拒 5.0)
    let mut merged = match serde_json::to_value(bm_core::limits::Limits::default()) {
        Ok(Value::Object(m)) => m,
        _ => Default::default(),
    };
    for (k, v) in incoming {
        if !editable.contains(k.as_str()) {
            continue;
        }
        let v = match v.as_f64() {
            Some(f) if f.fract() == 0.0 => json!(f as i64),
            _ => v,
        };
        merged.insert(k, v);
    }
    let new_limits = bm_core::limits::Limits::from_file_value(&Value::Object(merged.clone()));
    // 原子写(与 fs.write/配置面同款语义:临时文件+rename,崩溃不留半截)
    let path = cfg.data_dir.join("config").join("limits.json");
    if let Some(parent) = path.parent() {
        respond_or_fail!(
            std::fs::create_dir_all(parent).map_err(|e| internal(format!("建配置目录失败: {e}")))
        );
    }
    let pretty = serde_json::to_string_pretty(&new_limits.to_file_value()).unwrap_or_default();
    respond_or_fail!(
        bm_core::ports::persist::atomic_write(&path, pretty.as_bytes())
            .map_err(|e| internal(format!("写 limits.json 失败: {e}")))
    );
    cfg.limits.set(new_limits);
    if let Ok(mut src) = cfg.limits_sources.lock() {
        src.file_raw = Some(Value::Object(merged));
    }
    Json(json!({
        "ok": true,
        "note": "已保存并热生效:命令/工具下一条、回合下一回合、流式下一条起生效",
    }))
    .into_response()
}
