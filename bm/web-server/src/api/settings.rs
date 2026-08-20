//! settings.* handler 领域子模块（api.rs 拆分）。
//! 白名单命名空间视图 + update/replace/mutate 写面（secrets 永不出域）＋ provider 覆盖同步。

use serde_json::{json, Value};
use std::path::Path;

use crate::api::AppState;
use host_fs;
use crate::rpc::{err, err_with_details, ok};

pub(super) const WEB_SETTINGS_NAMESPACES: &[&str] = &[
    "agent-loop",
    "shell",
    "locale",
    "permission",
    "ui-conversation",
    "ui-theme",
    "ui-onboarding",
    "host", // 工作目录等宿主设置（host.workdir 为文件管理器的唯一事实源）
];

/// 构造单个 SettingsNamespaceView（台账 §2：{ns, schema, value, base?, user?, applies, secrets, revision}）。
/// 脱敏铁律：secret 字段永不随响应出域（M2.5 无 secret 字段，secrets 槽恒空）。
pub(super) fn settings_view(
    ns: &str,
    value: &serde_json::Map<String, Value>,
    revision: u64,
) -> Value {
    json!({
        "ns": ns,
        "schema": {},
        "value": value,
        "applies": "restart",
        "secrets": [],
        "revision": revision,
    })
}

/// settings.describe（特权）：返回白名单命名空间视图。
pub(super) fn settings_describe(state: &AppState) -> Value {
    let settings = state.settings.lock().unwrap();
    let revisions = state.settings_revisions.lock().unwrap();
    let mut namespaces = Vec::new();
    // 静态白名单 + 真实 provider 的 settings ns（llm.<id>，对齐 DSH 每个插件一个 ns）。
    let mut ns_list: Vec<&str> = WEB_SETTINGS_NAMESPACES.to_vec();
    for p in &state.providers {
        if !ns_list.contains(&p.settings_ns.as_str()) {
            ns_list.push(&p.settings_ns);
        }
    }
    for ns in ns_list {
        let value = settings.get(ns).cloned().unwrap_or_default();
        let revision = revisions.get(ns).copied().unwrap_or(0);
        namespaces.push(settings_view(ns, &value, revision));
    }
    ok(json!({ "writable": true, "hasDocument": false, "namespaces": namespaces }))
}

/// settings.update（特权）：整 ns patch 合并（对象深合并，JSON patch 语义近似）。
pub(super) fn settings_update(state: &AppState, payload: Value) -> Value {
    settings_write(state, payload, |cur, payload| {
        if let Some(patch) = payload.get("patch").and_then(Value::as_object) {
            for (k, v) in patch {
                cur.insert(k.clone(), v.clone());
            }
        }
    })
}

/// settings.replace（特权）：整段替换。
pub(super) fn settings_replace(state: &AppState, payload: Value) -> Value {
    settings_write(state, payload, |cur, payload| {
        if let Some(section) = payload.get("section").and_then(Value::as_object) {
            *cur = section.clone();
        }
    })
}

/// settings.mutate（特权）：{op:'set',path,value} / {op:'unset',path} 点路径写。
pub(super) fn settings_mutate(state: &AppState, payload: Value) -> Value {
    settings_write(state, payload, |cur, payload| {
        let Some(ops) = payload.get("ops").and_then(Value::as_array) else {
            return;
        };
        for op in ops {
            let Some(op_kind) = op.get("op").and_then(Value::as_str) else {
                continue;
            };
            let path: Vec<String> = op
                .get("path")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            match op_kind {
                "set" => {
                    if let Some(value) = op.get("value").cloned() {
                        if path.is_empty() {
                            continue;
                        }
                        let last = path.len() - 1;
                        let mut node = &mut *cur;
                        for (i, seg) in path.iter().enumerate() {
                            if i == last {
                                node.insert(seg.clone(), value.clone());
                            } else {
                                // 中间段：缺失或非对象 → 以 {} 垫底后下钻。
                                let missing = match node.get(seg) {
                                    Some(v) => !v.is_object(),
                                    None => true,
                                };
                                if missing {
                                    node.insert(seg.clone(), json!({}));
                                }
                                node = node.get_mut(seg).unwrap().as_object_mut().unwrap();
                            }
                        }
                    }
                }
                "unset" => {
                    if let Some(last) = path.last() {
                        cur.remove(last);
                    }
                }
                _ => {}
            }
        }
    })
}

/// 共用写面：定位 ns → expectedRevision 冲突校验 → 应用闭包变更 → 同步
/// provider 动态覆盖 → revision+1 → 返回新视图。
/// 未知 ns → `settings-rejected {ns}`（台账：schema 校验/未知 ns/只读 provider → settings-rejected）。
/// 写带 `expectedRevision` 且与当前 revision 不匹配 → `settings-conflict{ns, expected, actual}`
/// （M4 P1-4，对齐 api-proxy-config.spec.ts）。
/// provider 的 `llm.<id>` ns 是动态写面（M3 收尾）：`baseURL` 变更同步到适配器，
/// 下一请求即生效（对齐 DSH `llm-deepseek` settings section 每请求解析）。
pub(super) fn settings_write<F>(state: &AppState, payload: Value, apply: F) -> Value
where
    F: FnOnce(&mut serde_json::Map<String, Value>, &Value),
{
    let Some(ns) = payload.get("ns").and_then(Value::as_str) else {
        return err("bad-request", "missing ns");
    };
    let dynamic_ns = state.providers.iter().any(|p| p.settings_ns == ns);
    if !WEB_SETTINGS_NAMESPACES.contains(&ns) && !dynamic_ns {
        return err_with_details("settings-rejected", "namespace not writable", json!({ "ns": ns }));
    }
    let mut settings = state.settings.lock().unwrap();
    let mut revisions = state.settings_revisions.lock().unwrap();
    let current = revisions.get(ns).copied().unwrap_or(0);
    if let Some(expected) = payload.get("expectedRevision").and_then(Value::as_u64) {
        if expected != current {
            return err_with_details(
                "settings-conflict",
                "settings revision conflict",
                json!({ "ns": ns, "expected": expected, "actual": current }),
            );
        }
    }
    let mut value = settings.get(ns).cloned().unwrap_or_default();
    apply(&mut value, &payload);
    // host 命名空间写面校验：workdir 必须是存在且可读的绝对目录（设置保存即校验，
    // 防把工作目录指向 / 或不存在路径后文件面全量失效/全盘暴露）。
    if ns == "host" {
        if let Some(wd) = value.get("workdir").and_then(Value::as_str) {
            let wd = wd.trim();
            if !wd.is_empty() {
                let p = Path::new(wd);
                if !p.is_absolute() {
                    return err_with_details(
                        "settings-rejected",
                        "host.workdir must be an absolute directory path",
                        json!({ "ns": ns, "field": "workdir" }),
                    );
                }
                if let Err(e) = host_fs::validate_workdir(p) {
                    return err_with_details(
                        "settings-rejected",
                        format!("host.workdir invalid: {e}"),
                        json!({ "ns": ns, "field": "workdir" }),
                    );
                }
            }
        }
    }
    settings.insert(ns.to_string(), value.clone());
    revisions.insert(ns.to_string(), current + 1);
    drop(revisions);
    drop(settings); // persist_settings 需重取 settings 锁，先释放本写者锁
    if dynamic_ns {
        sync_provider_overrides(state, ns, &value);
    }
    state.persist_settings();
    ok(settings_view(ns, &value, current + 1))
}

/// 把 `llm.<id>` 命名空间写面的 baseURL 同步到适配器（覆盖优先、恢复装配值用 null/缺省）。
pub(super) fn sync_provider_overrides(state: &AppState, ns: &str, value: &serde_json::Map<String, Value>) {
    let Some(provider) = state.providers.iter().find(|p| p.settings_ns == ns) else {
        return;
    };
    if let Some(adapter) = &provider.adapter {
        match value.get("baseURL") {
            Some(Value::String(u)) if !u.is_empty() => adapter.set_base_url_override(Some(u.clone())),
            _ => adapter.set_base_url_override(None),
        }
    }
}

