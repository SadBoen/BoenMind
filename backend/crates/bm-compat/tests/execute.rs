//! B4 tool execution integration tests: load a real TS plugin, execute its
//! tool through the `__pi_execute_tool` bridge, assert the result value.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use bm_compat::extensions::PolicyProfile;
use bm_compat::execute::execute_tool;
use bm_compat::load::{load_extension, JsExtensionLoadSpec};

use common::mock_services;

/// Plugin whose `execute` echoes the input back with a computed value —
/// no imports, no hostcalls (hostcall wiring is B5/B6 scope).
const ECHO_PLUGIN: &str = r#"
export default function init(pi) {
  pi.registerTool({
    name: "echo",
    description: "echo back the text",
    parameters: { type: "object", properties: { text: { type: "string" } } },
    execute: async (_callId, input) => ({
      content: [{ type: "text", text: "echo:" + input.text }],
      details: { echoed: input.text },
    }),
  });
}
"#;

fn write_plugin_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bm-compat-exec-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("package.json"),
        r#"{ "name": "exec-plugin", "version": "1.0.0" }"#,
    )
    .expect("package.json");
    std::fs::write(dir.join("index.ts"), ECHO_PLUGIN).expect("index.ts");
    dir.join("index.ts")
}

#[tokio::test(flavor = "current_thread")]
async fn execute_tool_returns_plugin_result() {
    let entry = write_plugin_dir("roundtrip");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let thread = common::test_thread(mock_services(), PolicyProfile::Permissive.to_policy()).await;
    load_extension(&thread, &spec).await.expect("load");

    let result = execute_tool(
        &thread,
        "echo",
        "call-1",
        serde_json::json!({ "text": "Boen" }),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("execute");

    assert_eq!(
        result["content"][0]["text"],
        "echo:Boen",
        "插件 execute 的返回值应原样回读: {result}"
    );
    assert_eq!(result["details"]["echoed"], "Boen");

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn execute_unknown_tool_rejects() {
    let entry = write_plugin_dir("unknown");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let thread = common::test_thread(mock_services(), PolicyProfile::Permissive.to_policy()).await;
    load_extension(&thread, &spec).await.expect("load");

    let err = execute_tool(
        &thread,
        "not-registered",
        "call-2",
        serde_json::json!({}),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();
    // __pi_execute_tool throws `Unknown tool` → task rejection surfaces
    // through await_js_task — must never hang.
    assert!(
        err.to_string().contains("rejected") || err.to_string().contains("Unknown tool"),
        "unexpected error: {err}"
    );

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}
