//! B6 event dispatch integration tests: load a real TS plugin that subscribes
//! to events via `pi.on`, push events through the
//! `__pi_dispatch_extension_event` bridge, assert handler invocation,
//! payload delivery and return-value readback.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use bm_compat::events::dispatch_extension_event;
use bm_compat::extensions::PolicyProfile;
use bm_compat::load::{load_extension, JsExtensionLoadSpec};

use common::{mock_services, test_thread};

/// Plugin subscribing to `startup` / `tool_result` and recording what it sees.
const EVENT_PLUGIN: &str = r#"
export default function init(pi) {
  globalThis.__events_log = [];
  pi.on("startup", async (event, ctx) => {
    globalThis.__events_log.push("startup:" + String(ctx && ctx.cwd || ""));
  });
  pi.on("tool_result", async (event, ctx) => ({
    seen: event.toolName,
    cwd: String(ctx && ctx.cwd || ""),
  }));
  pi.on("session_before_compact", async (event, ctx) => {
    if (event && event.cancelProbe) return { cancel: true };
    return undefined;
  });
  pi.registerTool({
    name: "read_log",
    description: "read back the recorded event log",
    parameters: { type: "object" },
    execute: async () => ({ log: globalThis.__events_log.join(",") }),
  });
}
"#;

fn write_plugin_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bm-compat-events-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("package.json"),
        r#"{ "name": "events-plugin", "version": "1.0.0" }"#,
    )
    .expect("package.json");
    std::fs::write(dir.join("index.ts"), EVENT_PLUGIN).expect("index.ts");
    dir.join("index.ts")
}

#[tokio::test(flavor = "current_thread")]
async fn dispatch_delivers_payload_and_ctx() {
    let entry = write_plugin_dir("deliver");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let thread = common::test_thread(mock_services(), PolicyProfile::Permissive.to_policy()).await;
    load_extension(&thread, &spec).await.expect("load");

    // startup：handler 应收到 ctx.cwd（ctx payload 经 __pi_make_extension_ctx）
    let out = dispatch_extension_event(
        &thread,
        "startup",
        serde_json::json!({ "type": "startup", "version": "1.0.0" }),
        serde_json::json!({ "cwd": "C:/work/proj" }),
        Duration::from_secs(10),
    )
    .await
    .expect("dispatch startup");
    assert!(out.is_null(), "startup handler 无返回值 → Null: {out}");

    // tool_result：handler 返回 {seen, cwd}——链上 last 值应原样回读
    let out = dispatch_extension_event(
        &thread,
        "tool_result",
        serde_json::json!({ "type": "tool_result", "toolName": "web_search", "isError": false }),
        serde_json::json!({ "cwd": "C:/work/proj" }),
        Duration::from_secs(10),
    )
    .await
    .expect("dispatch tool_result");
    assert_eq!(out["seen"], "web_search", "handler 应收到 event.toolName: {out}");
    assert_eq!(out["cwd"], "C:/work/proj", "handler 应收到 ctx.cwd: {out}");

    // session_before_compact：cancel 语义回读（宿主据此可拦截压缩）
    let out = dispatch_extension_event(
        &thread,
        "session_before_compact",
        serde_json::json!({ "type": "session_before_compact", "cancelProbe": true }),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("dispatch session_before_compact");
    assert_eq!(out["cancel"], true, "handler 的 cancel 返回值应回读: {out}");

    // 未注册的事件：无 handler → Null，不报错不挂起
    let out = dispatch_extension_event(
        &thread,
        "no_such_event",
        serde_json::json!({}),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("dispatch unknown event");
    assert!(out.is_null(), "未注册事件应返回 Null: {out}");

    // 事件确实按序送达（经 read_log 工具读回 handler 侧的记录）
    let log = bm_compat::execute::execute_tool(
        &thread,
        "read_log",
        "call-1",
        serde_json::json!({}),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("read_log");
    assert_eq!(log["log"], "startup:C:/work/proj");

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn dispatch_empty_event_name_rejects() {
    let entry = write_plugin_dir("empty-name");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let thread = common::test_thread(mock_services(), PolicyProfile::Permissive.to_policy()).await;
    load_extension(&thread, &spec).await.expect("load");

    // 空事件名：JS 桥 throw → task rejection 浮出（不得挂起）
    let err = dispatch_extension_event(
        &thread,
        "",
        serde_json::json!({}),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("rejected") || err.to_string().contains("required"),
        "unexpected error: {err}"
    );

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

/// B6 补充：handler 内发起 hostcall（pi.tool）的泵循环——ctx-compactor 的
/// tool_result 修剪正是「事件 handler 里 read/write 落库」的组合场景。
#[tokio::test(flavor = "current_thread")]
async fn event_handler_hostcall_pumps() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bm-compat-evhc-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("package.json"),
        r#"{ "name": "evhc-plugin", "version": "1.0.0" }"#,
    )
    .expect("pkg");
    std::fs::write(
        dir.join("index.ts"),
        r#"
export default function init(pi) {
  globalThis.__hc_log = [];
  pi.on("tool_result", async (event, ctx) => {
    globalThis.__hc_log.push("handler-enter");
    const r = await pi.tool("write", { path: "hc-probe.txt", content: "probe-" + String(event.toolName) });
    globalThis.__hc_log.push("write-return:" + JSON.stringify(r));
    return { done: true };
  });
  pi.registerTool({
    name: "read_log",
    description: "read log",
    parameters: { type: "object" },
    execute: async () => ({ log: globalThis.__hc_log.join(",") }),
  });
}
"#,
    )
    .expect("index.ts");
    let spec = JsExtensionLoadSpec::from_entry_path(dir.join("index.ts")).expect("spec");

    let thread = test_thread(mock_services(), PolicyProfile::Permissive.to_policy()).await;
    load_extension(&thread, &spec).await.expect("load");

    let out = dispatch_extension_event(
        &thread,
        "tool_result",
        serde_json::json!({ "type": "tool_result", "toolName": "web_search", "content": [{ "type": "text", "text": "x".repeat(300) }], "isError": false }),
        serde_json::json!({ "cwd": dir.display().to_string() }),
        Duration::from_secs(20),
    )
    .await
    .expect("dispatch");
    assert_eq!(out["done"], true, "handler 返回值应回读: {out}");

    // handler 内 pi.tool hostcall 应经泵循环分发（MockServices 记录调用）
    let log = bm_compat::execute::execute_tool(
        &thread,
        "read_log",
        "call-1",
        serde_json::json!({}),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("read_log");
    assert_eq!(
        log["log"],
        "handler-enter,write-return:{\"ok\":true}",
        "handler 应进入且 hostcall 返回: {log}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
