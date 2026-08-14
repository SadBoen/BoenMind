//! B6 补充 — session 端口集成测试：加载真实 TS 插件，经
//! `__pi_execute_tool` 桥执行调用 `pi.session(op, args)` 的工具，覆盖全链路：
//! TS `pi.session` → `__pi_session_native` hostcall（op/payload 原样入队）→
//! B2 泵循环路由 → 宿主 `HostServices::session(call_id, op, payload)` →
//! 完成信号回灌 JS promise → 工具结果回读。
//!
//! 宿主侧用本文件的 `SessionMockServices`（记录 (op, payload)、注入 canned
//! 结果/错误）：共享 MockServices 的 session 端口只记 op 不记 payload、
//! 返回固定 `{ok:true}`，无法覆盖 payload 透传与错误路径。
//! `common::test_thread` 已泛化为任意 `HostServices` 实现，共享 mock 行为不变。

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bm_compat::execute::execute_tool;
use bm_compat::extensions::PolicyProfile;
use bm_compat::host::{HostServices, HostThread};
use bm_compat::load::{load_extension, JsExtensionLoadSpec};
use bm_compat::scheduler::{HostcallOutcome, WallClock};

use common::test_thread;

/// 两个工具都转调 `pi.session`：
/// `session_probe` 不捕获异常（宿主报错 → hostcall 拒绝 → 任务拒绝浮出）；
/// `session_probe_catch` 捕获并把拒绝形状（code/message）落进结果。
const SESSION_PLUGIN: &str = r#"
export default function init(pi) {
  pi.registerTool({
    name: "session_probe",
    description: "call pi.session and echo the result",
    parameters: { type: "object" },
    execute: async (_callId, input) => ({
      op: input.op,
      got: await pi.session(input.op, input.payload),
    }),
  });
  pi.registerTool({
    name: "session_probe_catch",
    description: "call pi.session and capture the rejection shape",
    parameters: { type: "object" },
    execute: async (_callId, input) => {
      try {
        return { ok: true, got: await pi.session(input.op, input.payload) };
      } catch (e) {
        return {
          ok: false,
          code: e && e.code ? e.code : null,
          message: e && e.message ? e.message : String(e),
        };
      }
    },
  });
}
"#;

fn write_plugin_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bm-compat-session-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("package.json"),
        r#"{ "name": "session-plugin", "version": "1.0.0" }"#,
    )
    .expect("package.json");
    std::fs::write(dir.join("index.ts"), SESSION_PLUGIN).expect("index.ts");
    dir.join("index.ts")
}

/// Session 端口专用 mock：记录每次 `(call_id, op, payload)` 原样，并按配置
/// 回 canned JSON（`HostcallOutcome::Success`）或 canned 错误
/// （`HostcallOutcome::Error { code, message }`）。
struct SessionMockServices {
    /// 每次 session 调用记录为 `{ call_id, op, payload }`。
    session_calls: Mutex<Vec<serde_json::Value>>,
    /// `Ok(value)` → Success；`Err((code, message))` → Error。
    response: Mutex<Result<serde_json::Value, (String, String)>>,
}

impl SessionMockServices {
    fn success(value: serde_json::Value) -> Arc<Self> {
        Arc::new(Self {
            session_calls: Mutex::new(Vec::new()),
            response: Mutex::new(Ok(value)),
        })
    }

    fn error(code: &str, message: &str) -> Arc<Self> {
        Arc::new(Self {
            session_calls: Mutex::new(Vec::new()),
            response: Mutex::new(Err((code.to_string(), message.to_string()))),
        })
    }

    fn calls(&self) -> Vec<serde_json::Value> {
        self.session_calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl HostServices for SessionMockServices {
    async fn execute_tool(
        &self,
        _call_id: &str,
        _name: &str,
        _input: serde_json::Value,
    ) -> HostcallOutcome {
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn exec(
        &self,
        _call_id: &str,
        _cmd: &str,
        _payload: serde_json::Value,
    ) -> HostcallOutcome {
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn http(&self, _call_id: &str, _payload: serde_json::Value) -> HostcallOutcome {
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn session(
        &self,
        call_id: &str,
        op: &str,
        payload: serde_json::Value,
    ) -> HostcallOutcome {
        self.session_calls.lock().unwrap().push(serde_json::json!({
            "call_id": call_id,
            "op": op,
            "payload": payload,
        }));
        match &*self.response.lock().unwrap() {
            Ok(value) => HostcallOutcome::Success(value.clone()),
            Err((code, message)) => HostcallOutcome::Error {
                code: code.clone(),
                message: message.clone(),
            },
        }
    }

    async fn ui(
        &self,
        _call_id: &str,
        _op: &str,
        _payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn events(
        &self,
        _call_id: &str,
        _op: &str,
        _payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }
}

async fn thread(services: Arc<SessionMockServices>) -> HostThread<WallClock> {
    test_thread(services, PolicyProfile::Permissive.to_policy()).await
}

#[tokio::test(flavor = "current_thread")]
async fn session_op_and_payload_arrive_verbatim() {
    let entry = write_plugin_dir("verbatim");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let canned = serde_json::json!([
        { "seq": 1, "role": "user", "content": "第一轮问题" },
    ]);
    let services = SessionMockServices::success(canned.clone());
    let thread = thread(services.clone()).await;
    load_extension(&thread, &spec).await.expect("load");

    let payload = serde_json::json!({
        "surface": "main",
        "maxEntries": 10,
        "tags": ["a", "b"],
        "opts": { "since": "2026-08-14" },
    });
    let result = execute_tool(
        &thread,
        "session_probe",
        "call-1",
        serde_json::json!({ "op": "getmessagesurface", "payload": payload.clone() }),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("execute");

    // op 名与 payload 应原样到达宿主 session 端口（无白名单/改名）
    let calls = services.calls();
    assert_eq!(
        calls.len(),
        1,
        "宿主 session 端口应恰好收到一次调用: {calls:?}"
    );
    assert_eq!(
        calls[0]["op"], "getmessagesurface",
        "自定义 op 名应原样透传: {calls:?}"
    );
    assert!(
        calls[0]["call_id"]
            .as_str()
            .is_some_and(|s| s.starts_with("call-")),
        "hostcall call_id 应形如 call-*: {calls:?}"
    );
    assert_eq!(calls[0]["payload"], payload, "payload 应原样透传: {calls:?}");

    // 宿主 canned 返回值应回灌 JS promise 并落进工具结果
    assert_eq!(result["op"], "getmessagesurface", "工具应回显收到的 op: {result}");
    assert_eq!(result["got"], canned, "宿主返回值应回传 TS 侧: {result}");

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn session_return_value_readback_deep() {
    let entry = write_plugin_dir("readback");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    // 对齐 bm-server getmessagesurface 的投影面形状：消息数组（seq/role/content）
    let canned = serde_json::json!([
        { "seq": 1, "role": "user", "content": "第一轮问题" },
        { "seq": 2, "role": "assistant", "content": "第一轮回答", "meta": { "tokens": 42 } },
        { "seq": 3, "role": "user", "content": "压缩后仍可见？" },
    ]);
    let services = SessionMockServices::success(canned.clone());
    let thread = thread(services.clone()).await;
    load_extension(&thread, &spec).await.expect("load");

    let result = execute_tool(
        &thread,
        "session_probe",
        "call-1",
        serde_json::json!({ "op": "getmessagesurface", "payload": { "surface": "main" } }),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("execute");

    assert_eq!(
        result["got"], canned,
        "宿主返回的 JSON（含嵌套数组/对象/数字）应深度回传 TS 侧: {result}"
    );

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn session_host_error_rejects_uncaught() {
    let entry = write_plugin_dir("reject");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let services =
        SessionMockServices::error("surface_unavailable", "getmessagesurface: 投影面不可用");
    let thread = thread(services.clone()).await;
    load_extension(&thread, &spec).await.expect("load");

    // session_probe 不捕获：宿主 Error → JS promise reject → 工具任务拒绝浮出
    // （与 execute.rs 未知工具一致，不得挂起）。
    let err = execute_tool(
        &thread,
        "session_probe",
        "call-1",
        serde_json::json!({ "op": "getmessagesurface", "payload": { "surface": "main" } }),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("rejected"),
        "hostcall 拒绝应作为任务拒绝浮出: {err}"
    );
    assert!(
        err.to_string().contains("surface_unavailable"),
        "宿主错误 code 应随拒绝浮出: {err}"
    );
    assert!(
        err.to_string().contains("投影面不可用"),
        "宿主错误 message 应随拒绝浮出: {err}"
    );

    // 调用确实到达了宿主（错误来自宿主而非策略拒绝）
    let calls = services.calls();
    assert_eq!(calls.len(), 1, "报错前宿主应收到调用: {calls:?}");
    assert_eq!(calls[0]["op"], "getmessagesurface", "{calls:?}");

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn session_host_error_shape_surfaces_to_plugin() {
    let entry = write_plugin_dir("catch");
    let spec = JsExtensionLoadSpec::from_entry_path(&entry).expect("spec");

    let services =
        SessionMockServices::error("surface_unavailable", "getmessagesurface: 投影面不可用");
    let thread = thread(services.clone()).await;
    load_extension(&thread, &spec).await.expect("load");

    // session_probe_catch 捕获拒绝：错误应带 code/message 字段
    // （__pi_complete_hostcall_impl 的拒绝形状：Error(message) + error.code）。
    let result = execute_tool(
        &thread,
        "session_probe_catch",
        "call-1",
        serde_json::json!({ "op": "getmessagesurface", "payload": { "surface": "main" } }),
        serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .expect("execute");

    assert_eq!(result["ok"], false, "插件应捕获到宿主拒绝: {result}");
    assert_eq!(
        result["code"], "surface_unavailable",
        "宿主错误 code 应送达 TS 侧: {result}"
    );
    assert_eq!(
        result["message"], "getmessagesurface: 投影面不可用",
        "宿主错误 message 应送达 TS 侧: {result}"
    );

    std::fs::remove_dir_all(entry.parent().unwrap()).ok();
}
