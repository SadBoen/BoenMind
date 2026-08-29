//! Schema 校验工具:把冻结 schema 编译成校验器,供建仓/测试/strict 模式使用。
//!
//! session/agent/connector/exec-log schema 通过 `$id` 引用 envelope 的
//! definitions(如 `boenmind:wire:envelope:v0.1#/definitions/id`)。为不引入
//! 远程解析,这里把被引 definitions 合并进主文档并把跨文档引用改写为本地
//! `#/definitions/...`,得到等价的单文档 schema 再编译。

use serde_json::{Value, json};

const ENVELOPE_ID: &str = "boenmind:wire:envelope:v0.1#";
const BUDGET_ID: &str = "boenmind:budget:v0.1#";
const TASK_ID: &str = "boenmind:task:task:v0.1#";
const WIRE_CAPABILITY_ID: &str = "boenmind:wire:capability:v0.1#";

/// 合并跨文档引用,返回可独立编译的 schema 文档。
/// 已知被引文档:envelope / budget / task / wire-capability(M5 起增后两者)。
/// 合并是传递闭包:task 引用 wire-capability 的 definitions,合并进主文档后
/// 会继续触发下一轮扫描,直到不再出现新的被引 id(不动点)。
pub fn combine(schema_text: &str) -> Value {
    let mut doc: Value = serde_json::from_str(schema_text).expect("schema 必须是合法 JSON");

    const ENVELOPE: &str = "envelope";
    const BUDGET: &str = "budget";
    const TASK: &str = "task";
    const WIRE_CAPABILITY: &str = "wire-capability";

    fn source_of(name: &str) -> &'static str {
        match name {
            ENVELOPE => crate::registries::ENVELOPE_SCHEMA,
            BUDGET => crate::registries::BUDGET_SCHEMA,
            TASK => crate::registries::TASK_SCHEMA,
            WIRE_CAPABILITY => crate::registries::WIRE_CAPABILITY_SCHEMA,
            _ => unreachable!("未知被引文档"),
        }
    }

    fn id_of(name: &str) -> &'static str {
        match name {
            ENVELOPE => ENVELOPE_ID,
            BUDGET => BUDGET_ID,
            TASK => TASK_ID,
            WIRE_CAPABILITY => WIRE_CAPABILITY_ID,
            _ => unreachable!("未知被引文档"),
        }
    }

    let mut merged: Vec<&'static str> = Vec::new();
    loop {
        let text = serde_json::to_string(&doc).expect("序列化不会失败");
        let mut progressed = false;
        for name in [ENVELOPE, BUDGET, TASK, WIRE_CAPABILITY] {
            if text.contains(id_of(name)) && !merged.contains(&name) {
                let obj = doc.as_object_mut().expect("schema 顶层必须是对象");
                let defs = obj
                    .entry("definitions")
                    .or_insert_with(|| Value::Object(Default::default()));
                let defs_obj = defs.as_object_mut().expect("definitions 必须是对象");
                if name == ENVELOPE {
                    let envelope: Value =
                        serde_json::from_str(source_of(name)).expect("envelope schema 合法");
                    for (k, v) in envelope
                        .get("definitions")
                        .and_then(|d| d.as_object())
                        .expect("envelope 必须有 definitions")
                    {
                        defs_obj.entry(k.clone()).or_insert(v.clone());
                    }
                    // envelope 根层的命名子 schema(request/response/event_envelope)
                    // 也被跨文档引用,一并并入 definitions,使 "#/definitions/<name>"
                    // 可解析。
                    for sub in ["request", "response", "event_envelope"] {
                        if let Some(s) = envelope.get(sub) {
                            defs_obj.entry(sub.to_string()).or_insert(s.clone());
                        }
                    }
                } else {
                    let src: Value =
                        serde_json::from_str(source_of(name)).expect("被引 schema 合法");
                    for (k, v) in src
                        .get("definitions")
                        .and_then(|d| d.as_object())
                        .expect("被引 schema 必须有 definitions")
                    {
                        defs_obj.entry(k.clone()).or_insert(v.clone());
                    }
                }
                merged.push(name);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    rewrite_refs(&mut doc);
    doc
}

fn rewrite_refs(v: &mut Value) {
    const PREFIX: &str = "boenmind:";
    match v {
        Value::String(s) => {
            if let Some(rest) = s.strip_prefix(PREFIX) {
                // "wire:envelope:v0.1#/definitions/id" → "#/definitions/id"
                // "wire:envelope:v0.1#/event_envelope"    → "#/definitions/event_envelope"
                if let Some(frag) = rest.split_once('#').map(|(_, f)| f) {
                    if frag.starts_with("/definitions/") {
                        *s = format!("#{frag}");
                    } else {
                        *s = format!("#/definitions{frag}");
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_refs(item);
            }
        }
        Value::Object(map) => {
            for (_k, val) in map.iter_mut() {
                rewrite_refs(val);
            }
        }
        _ => {}
    }
}

/// 编译 + 校验。返回全部校验错误的拼接文本(测试断言用)。
pub fn validate(schema_text: &str, instance: &Value) -> Result<(), String> {
    let doc = combine(schema_text);
    let validator = jsonschema::validator_for(&doc).map_err(|e| format!("schema 编译失败: {e}"))?;
    validator
        .validate(instance)
        .map_err(|error| format!("schema 校验失败: {error}"))
}

/// 按 JSON Pointer 取 schema 子树后校验(session/agent schema 顶层不是实例
/// schema)。pointer 可带 `#` 前缀(如 `#/event_envelope`)。子树以 `allOf`
/// 包裹并继承根 definitions,使其内部 `$ref` 仍可解析。
pub fn validate_by_pointer(
    schema_text: &str,
    pointer: &str,
    instance: &Value,
) -> Result<(), String> {
    let doc = combine(schema_text);
    let pointer = pointer.strip_prefix('#').unwrap_or(pointer);
    let sub = doc
        .pointer(pointer)
        .ok_or_else(|| format!("schema 无此指针: {pointer}"))?
        .clone();
    let defs = doc.get("definitions").cloned().unwrap_or(json!({}));
    let wrapper = json!({ "definitions": defs, "allOf": [sub] });
    let validator =
        jsonschema::validator_for(&wrapper).map_err(|e| format!("schema 编译失败: {e}"))?;
    validator
        .validate(instance)
        .map_err(|e| format!("schema 校验失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_schema_compiles_and_validates() {
        let event = json!({
            "event_seq": 1,
            "type": "runtime.started",
            "occurred_at": "2026-08-29T09:30:00.100Z",
            "payload": {"pid": 1, "version": "0.1.0-m1", "started_at": "2026-08-29T09:30:00.098Z"}
        });
        validate_by_pointer(
            crate::registries::ENVELOPE_SCHEMA,
            "#/event_envelope",
            &event,
        )
        .expect("合法事件");

        let bad = json!({
            "event_seq": 0,
            "type": "runtime.started",
            "occurred_at": "nope",
            "payload": {}
        });
        assert!(
            validate_by_pointer(crate::registries::ENVELOPE_SCHEMA, "#/event_envelope", &bad)
                .is_err(),
            "非法事件必须报错"
        );
    }

    #[test]
    fn cross_document_refs_are_combined() {
        let create = json!({"agent": {"name": "assistant", "model_chain": ["zhipu.glm-4-flash"]}});
        validate_by_pointer(
            crate::registries::SESSION_SCHEMA,
            "#/session.create/params",
            &create,
        )
        .expect("session.create params 合法");
    }
}
