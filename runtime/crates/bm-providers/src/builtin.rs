//! 内置演示能力 Provider(M4-T5,规格 §5.6):覆盖五风险等级 × 幂等性 ×
//! 审批矩阵,纯内存/本地状态,无真实外部副作用。真实 Provider(M7)替换
//! 实现,manifest 合同与 Broker 审计形态不变(调用方只依赖 Capability 名)。
//!
//! 能力集与裁决语义:
//! - system.echo           read-only            幂等,直通
//! - system.counter.bump   low-risk-command     幂等递增,直通
//! - system.notes.write    reversible-command   undo=system.notes.delete
//! - system.notes.delete   reversible-command   undo 的逆操作本体
//! - system.mail.mock_send external-side-effect mock 外部发送(返回收据,留档)
//! - system.danger.purge   high-risk-command    恒审批(Broker 双保险兜住)

use bm_contract::capability::CapabilityManifest;
use bm_core::broker::provider_fn;
use bm_core::registry::CapabilityProvider;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 共享演示状态(notes 域 + mock 邮件发件箱 + 计数器)。
#[derive(Default)]
pub struct DemoState {
    notes: Mutex<HashMap<String, String>>,
    outbox: Mutex<Vec<Value>>,
    counters: Mutex<HashMap<String, u64>>,
}

fn manifest(name: &str, effect: &str, extra: Value) -> CapabilityManifest {
    let mut base = json!({
        "capability": name, "provider": name, "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": effect, "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    });
    if let (Some(base_obj), Some(extra_obj)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_obj {
            base_obj.insert(k.clone(), v.clone());
        }
    }
    serde_json::from_value(base).expect("内置 manifest 合法")
}

/// 内置能力装配集(RuntimeConfig.capabilities)。
pub fn builtin_capability_set() -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> {
    let state = Arc::new(DemoState::default());
    let mut out: Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> = Vec::new();

    // system.echo:回显(read-only)
    out.push((
        manifest(
            "system.echo",
            "read-only",
            json!({"scopes": ["system.echo"]}),
        ),
        provider_fn(Ok),
    ));

    // system.counter.bump:计数递增(low-risk-command;幂等键可)
    let st = state.clone();
    out.push((
        manifest(
            "system.counter.bump",
            "low-risk-command",
            json!({"scopes": ["system.counter"]}),
        ),
        provider_fn(move |args| {
            let key = args["key"]
                .as_str()
                .ok_or("缺必填参数 key(字符串)")?
                .to_string();
            let mut counters = st.counters.lock().expect("锁未中毒");
            let count = counters.entry(key.clone()).or_insert(0);
            *count += 1;
            Ok(json!({"key": key, "count": *count}))
        }),
    ));

    // system.notes.write:笔记写入(reversible-command;undo = delete)
    // W4b:input_schema 带 properties,模型据此传参(此前空 schema 导致
    // 模型无法得知必填参数)
    let st = state.clone();
    out.push((
        manifest(
            "system.notes.write",
            "reversible-command",
            json!({
                "scopes": ["system.notes"],
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "笔记路径/标题(唯一键)"},
                        "content": {"type": "string", "description": "笔记正文"}
                    },
                    "required": ["path", "content"]
                },
                "undo": {"capability": "system.notes.delete",
                         "args_map": {"path": "path"}}
            }),
        ),
        provider_fn(move |args| {
            let path = args["path"]
                .as_str()
                .ok_or("缺必填参数 path(字符串)")?
                .to_string();
            let content = args["content"]
                .as_str()
                .ok_or("缺必填参数 content(字符串)")?
                .to_string();
            st.notes
                .lock()
                .expect("锁未中毒")
                .insert(path.clone(), content);
            Ok(json!({"written": true, "path": path}))
        }),
    ));

    // system.notes.delete:逆操作本体(reversible-command)
    let st = state.clone();
    out.push((
        manifest(
            "system.notes.delete",
            "reversible-command",
            json!({
                "scopes": ["system.notes"],
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "要删除的笔记路径/标题"}
                    },
                    "required": ["path"]
                }
            }),
        ),
        provider_fn(move |args| {
            let path = args["path"]
                .as_str()
                .ok_or("缺必填参数 path(字符串)")?
                .to_string();
            let removed = st.notes.lock().expect("锁未中毒").remove(&path).is_some();
            Ok(json!({"deleted": removed, "path": path}))
        }),
    ));

    // system.mail.mock_send:mock 外部发送(external-side-effect;收据 = 本地
    // mock 收件号;真实外部系统与收据核验随 M7/T6 outbox)
    let st = state.clone();
    out.push((
        manifest(
            "system.mail.mock_send",
            "external-side-effect",
            json!({
                "scopes": ["system.mail"],
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "to": {"type": "string", "description": "收件人地址"},
                        "subject": {"type": "string", "description": "邮件主题"},
                        "body": {"type": "string", "description": "邮件正文"}
                    },
                    "required": ["to"]
                },
                "verification": null,
                "timeout_ms": 2000
            }),
        ),
        provider_fn(move |args| {
            let to = args["to"]
                .as_str()
                .ok_or("缺必填参数 to(字符串)")?
                .to_string();
            let subject = args["subject"].as_str().unwrap_or("").to_string();
            let receipt_no = st.outbox.lock().expect("锁未中毒").len() + 1;
            let receipt = json!({
                "message_id": format!("mock-{receipt_no:06}"),
                "queued": true,
            });
            st.outbox
                .lock()
                .expect("锁未中毒")
                .push(json!({"to": to, "subject": subject, "receipt": receipt}));
            Ok(receipt)
        }),
    ));

    // system.danger.purge:清除演示状态(high-risk-command;恒审批兜底)
    let st = state;
    out.push((
        manifest(
            "system.danger.purge",
            "high-risk-command",
            json!({"scopes": ["system.admin"], "cancellable": false}),
        ),
        provider_fn(move |args| {
            let target = args["target"].as_str().unwrap_or("all").to_string();
            let notes = st.notes.lock().expect("锁未中毒").len();
            if target == "all" || target == "notes" {
                st.notes.lock().expect("锁未中毒").clear();
            }
            Ok(json!({"purged": target, "notes_removed": notes}))
        }),
    ));

    out.push(model_invoke_cap());

    out
}

/// 生产环境纯净内置能力装配集：仅包含内核核心调度必须的能力（model.invoke），
/// 杜绝早期单元测试桩（mock 邮件、计数器、purge 等）污染生产环境模型工具集。
pub fn production_builtin_capability_set() -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)>
{
    vec![model_invoke_cap()]
}

/// M7 model.invoke:模型调用收编进 Capability 面(M7 规格 S1;M4 §5.8 豁免撤销)。
/// 执行体 = turn 循环内的连接器(spawn 前 Broker 查表放行);Wire 直调在此拒绝,
/// 防止绕过 turn 语义(预算记账/取消/审计)直接触模型。独立导出供最小装配
/// (TestRig::standard 只带 model.invoke,不携带演示能力)。
pub fn model_invoke_cap() -> (CapabilityManifest, Arc<dyn CapabilityProvider>) {
    (
        manifest(
            "model.invoke",
            "read-only",
            json!({
                "scopes": ["domain:model"],
                "idempotent": false,
                "cancellable": true,
                "timeout_ms": 120000,
                "input_schema": {
                    "type": "object",
                    "properties": {"model_id": {"type": "string"}}
                }
            }),
        ),
        provider_fn(|_args| Err("model.invoke 仅限运行时 turn 循环调用".to_string())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_set_covers_five_risk_classes() {
        let set = builtin_capability_set();
        assert_eq!(
            set.len(),
            7,
            "echo/counter/notes.write/notes.delete/mail/purge + model.invoke(M7)"
        );
        let mut effects: Vec<String> = set
            .iter()
            .map(|(m, _)| m.effect.as_str().to_string())
            .collect();
        effects.sort();
        assert!(effects.contains(&"read-only".to_string()));
        assert!(effects.contains(&"high-risk-command".to_string()));
        // undo 声明在场(notes.write)
        let write = set
            .iter()
            .find(|(m, _)| m.capability == "system.notes.write")
            .unwrap();
        assert!(write.0.undo.is_some(), "reversible 应声明 undo(基线 §5.2)");
    }

    #[test]
    fn notes_write_then_delete_roundtrip() {
        let set = builtin_capability_set();
        let write = set
            .iter()
            .find(|(m, _)| m.capability == "system.notes.write")
            .unwrap();
        let delete = set
            .iter()
            .find(|(m, _)| m.capability == "system.notes.delete")
            .unwrap();
        let r = write
            .1
            .invoke(json!({"path": "a.md", "content": "hi"}))
            .unwrap();
        assert_eq!(r["written"], json!(true));
        let r = delete.1.invoke(json!({"path": "a.md"})).unwrap();
        assert_eq!(r["deleted"], json!(true));
    }

    #[test]
    fn counter_increments_and_mail_returns_receipt() {
        let set = builtin_capability_set();
        let counter = set
            .iter()
            .find(|(m, _)| m.capability == "system.counter.bump")
            .unwrap();
        let r1 = counter.1.invoke(json!({"key": "k"})).unwrap();
        let r2 = counter.1.invoke(json!({"key": "k"})).unwrap();
        assert_eq!(r1["count"], json!(1));
        assert_eq!(r2["count"], json!(2));

        let mail = set
            .iter()
            .find(|(m, _)| m.capability == "system.mail.mock_send")
            .unwrap();
        let r = mail.1.invoke(json!({"to": "a@x", "subject": "s"})).unwrap();
        assert_eq!(r["queued"], json!(true));
        assert!(r["message_id"].as_str().unwrap().starts_with("mock-"));
    }
}
