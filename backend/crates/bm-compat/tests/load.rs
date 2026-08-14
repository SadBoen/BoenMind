//! B3 loading path integration tests: spec derivation + real plugin load
//! through the `__pi_load_extension` bridge + registered-tool readback.

mod common;

use std::path::PathBuf;

use bm_compat::extensions::{ExtensionPolicy, PolicyProfile};
use bm_compat::extensions_js::ExtensionToolDef;
use bm_compat::load::{load_extension, JsExtensionLoadSpec};

use common::mock_services;

/// Minimal plugin: default-exported init function registers one tool,
/// no imports, no hostcalls at load.
const MINIMAL_PLUGIN: &str = r#"
export default function init(pi) {
  pi.registerTool({
    name: "echo",
    description: "echo back the text",
    parameters: { type: "object", properties: { text: { type: "string" } } },
    execute: async (_callId, _input) => ({ ok: true }),
  });
}
"#;

/// Create a temp extension dir (index.ts + package.json), returns entry path.
fn write_plugin_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bm-compat-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("package.json"),
        r#"{ "name": "echo-plugin", "version": "1.2.3" }"#,
    )
    .expect("package.json");
    std::fs::write(dir.join("index.ts"), MINIMAL_PLUGIN).expect("index.ts");
    dir.join("index.ts")
}

#[test]
fn load_spec_derives_from_entry_path() {
    let entry = write_plugin_dir("spec");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");
    // index.ts entry → extension id = parent dir name (temp dir name).
    assert_eq!(
        spec.extension_id,
        entry.parent().unwrap().file_name().unwrap().to_str().unwrap()
    );
    // package.json supplies name/version.
    assert_eq!(spec.name, "echo-plugin");
    assert_eq!(spec.version, "1.2.3");
    assert_eq!(spec.api_version, bm_compat::load::PROTOCOL_VERSION);
    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

#[test]
fn load_spec_rejects_missing_entry() {
    let err = JsExtensionLoadSpec::from_entry_path("definitely-not-there.ts").unwrap_err();
    assert!(err.to_string().contains("does not exist"));
}

#[tokio::test(flavor = "current_thread")]
async fn load_extension_registers_tools() {
    let entry = write_plugin_dir("load");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let thread = common::test_thread(
        mock_services(),
        bm_compat::extensions::PolicyProfile::Permissive.to_policy(),
    )
    .await;

    let body = load_extension(&thread, &spec).await.expect("load");
    // __pi_load_extension resolves `true` on success (the registered
    // surface is read back through get_registered_tools, not the task value).
    assert_eq!(body, serde_json::Value::Bool(true));

    let tools: Vec<ExtensionToolDef> = thread
        .runtime()
        .get_registered_tools()
        .await
        .expect("registered tools");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"echo"), "echo tool registered, got {names:?}");

    let echo = tools.iter().find(|t| t.name == "echo").unwrap();
    assert_eq!(echo.description, "echo back the text");
    assert!(echo.parameters.get("properties").is_some());

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn load_extension_rejects_broken_entry() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bm-compat-broken-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("broken.ts");
    std::fs::write(&entry, "this is not valid typescript {{{").expect("broken.ts");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let thread = common::test_thread(
        mock_services(),
        bm_compat::extensions::PolicyProfile::Permissive.to_policy(),
    )
    .await;

    let err = load_extension(&thread, &spec).await.unwrap_err();
    // Either the bootstrap eval fails or the task rejects — both must
    // surface as an Error, never hang or panic.
    assert!(
        err.to_string().contains("bootstrap")
            || err.to_string().contains("rejected")
            || err.to_string().contains("timed out"),
        "unexpected error: {err}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
