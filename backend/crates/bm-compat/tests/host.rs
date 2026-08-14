//! B2 host thread integration tests.
//!
//! Kept out of the lib target on purpose: the vendored 6-file copies still
//! carry their upstream `mod tests` (proptest etc., see README TODO 1), so
//! `cargo test --lib` would drag them in. Integration tests compile only the
//! lib plus this file. Run with: `cargo test --test host`.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use bm_compat::extensions::{ExtensionOverride, ExtensionPolicy, ExtensionPolicyMode};
use bm_compat::extensions_js::{HostcallKind, HostcallRequest, PiJsRuntime};
use bm_compat::host::{check_capability, HostServices, HostThread, PolicyDecision};
use bm_compat::scheduler::{HostcallOutcome, WallClock};

fn policy(mode: ExtensionPolicyMode) -> ExtensionPolicy {
    ExtensionPolicy {
        mode,
        ..ExtensionPolicy::default()
    }
}

#[test]
fn capability_modes() {
    // Permissive allows everything, strict denies everything.
    let permissive = policy(ExtensionPolicyMode::Permissive);
    assert_eq!(
        check_capability(&permissive, "exec", None),
        PolicyDecision::Allow
    );
    let strict = policy(ExtensionPolicyMode::Strict);
    assert_eq!(
        check_capability(&strict, "read", None),
        PolicyDecision::Deny { reason: "strict" }
    );
    // Prompt: default_caps allow read, deny_caps deny exec.
    let prompt = policy(ExtensionPolicyMode::Prompt);
    assert_eq!(
        check_capability(&prompt, "read", None),
        PolicyDecision::Allow
    );
    assert_eq!(
        check_capability(&prompt, "exec", None),
        PolicyDecision::Deny { reason: "deny" }
    );
    assert_eq!(
        check_capability(&prompt, "ui", None),
        PolicyDecision::Deny { reason: "prompt" }
    );
}

#[test]
fn capability_per_extension_override() {
    let mut p = policy(ExtensionPolicyMode::Prompt);
    p.per_extension.insert(
        "ext-a".to_string(),
        ExtensionOverride {
            mode: Some(ExtensionPolicyMode::Permissive),
            allow: Vec::new(),
            deny: Vec::new(),
            quota: None,
        },
    );
    p.per_extension.insert(
        "ext-b".to_string(),
        ExtensionOverride {
            mode: None,
            allow: vec!["ui".to_string()],
            deny: vec!["read".to_string()],
            quota: None,
        },
    );
    // ext-a permissive override grants anything.
    assert_eq!(
        check_capability(&p, "exec", Some("ext-a")),
        PolicyDecision::Allow
    );
    // ext-b: deny beats global default allow.
    assert_eq!(
        check_capability(&p, "read", Some("ext-b")),
        PolicyDecision::Deny { reason: "deny" }
    );
    // ext-b: explicit allow grants an otherwise-prompt capability.
    assert_eq!(
        check_capability(&p, "ui", Some("ext-b")),
        PolicyDecision::Allow
    );
}

fn request(kind: HostcallKind) -> HostcallRequest {
    HostcallRequest {
        call_id: "call-1".to_string(),
        kind,
        payload: serde_json::json!({}),
        trace_id: 0,
        extension_id: None,
    }
}

/// Mock services: records each routed call, returns a canned outcome.
struct MockServices {
    calls: Mutex<Vec<String>>,
    approve: bool,
}

#[async_trait::async_trait]
impl HostServices for MockServices {
    async fn execute_tool(
        &self,
        call_id: &str,
        name: &str,
        _input: serde_json::Value,
    ) -> HostcallOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("tool:{call_id}:{name}"));
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn exec(
        &self,
        call_id: &str,
        cmd: &str,
        _payload: serde_json::Value,
    ) -> HostcallOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("exec:{call_id}:{cmd}"));
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn http(&self, call_id: &str, _payload: serde_json::Value) -> HostcallOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("http:{call_id}"));
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn session(
        &self,
        call_id: &str,
        op: &str,
        _payload: serde_json::Value,
    ) -> HostcallOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("session:{call_id}:{op}"));
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn ui(
        &self,
        call_id: &str,
        op: &str,
        _payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("ui:{call_id}:{op}"));
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn events(
        &self,
        call_id: &str,
        op: &str,
        _payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(format!("events:{call_id}:{op}"));
        HostcallOutcome::Success(serde_json::json!({ "ok": true }))
    }

    async fn request_approval(&self, _capability: &str, _extension_id: Option<&str>) -> bool {
        self.approve
    }
}

async fn test_thread(
    services: Arc<MockServices>,
    policy: ExtensionPolicy,
) -> HostThread<WallClock> {
    let runtime = Rc::new(PiJsRuntime::new().await.expect("runtime boot"));
    HostThread::new(runtime, services, policy)
}

#[tokio::test(flavor = "current_thread")]
async fn dispatch_one_routes_all_kinds() {
    let services = Arc::new(MockServices {
        calls: Mutex::new(Vec::new()),
        approve: false,
    });
    let thread = test_thread(services.clone(), policy(ExtensionPolicyMode::Permissive)).await;

    let kinds = vec![
        HostcallKind::Tool {
            name: "web_search".to_string(),
        },
        HostcallKind::Exec {
            cmd: "echo hi".to_string(),
        },
        HostcallKind::Http,
        HostcallKind::Session {
            op: "get_state".to_string(),
        },
        HostcallKind::Ui {
            op: "confirm".to_string(),
        },
        HostcallKind::Events {
            op: "emit".to_string(),
        },
    ];
    for kind in kinds {
        let outcome = thread.dispatch_one(&request(kind)).await;
        assert!(matches!(outcome, HostcallOutcome::Success(_)));
    }
    let calls = services.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            "tool:call-1:web_search",
            "exec:call-1:echo hi",
            "http:call-1",
            "session:call-1:get_state",
            "ui:call-1:confirm",
            "events:call-1:emit",
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn dispatch_one_policy_denies() {
    let services = Arc::new(MockServices {
        calls: Mutex::new(Vec::new()),
        approve: false,
    });
    // Prompt policy: exec is in deny_caps → hard deny, no routing.
    let thread = test_thread(services.clone(), policy(ExtensionPolicyMode::Prompt)).await;
    let outcome = thread
        .dispatch_one(&request(HostcallKind::Exec {
            cmd: "rm -rf /".to_string(),
        }))
        .await;
    assert!(
        matches!(
            outcome,
            HostcallOutcome::Error { code, .. } if code == "denied"
        ),
        "deny_caps capability must fail closed"
    );
    assert!(services.calls.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn dispatch_one_prompt_approval_grants() {
    let services = Arc::new(MockServices {
        calls: Mutex::new(Vec::new()),
        approve: true,
    });
    // Prompt policy: "ui" is not pre-authorized → approval channel
    // grants it (B5 PermissionBridge path).
    let thread = test_thread(services.clone(), policy(ExtensionPolicyMode::Prompt)).await;
    let outcome = thread
        .dispatch_one(&request(HostcallKind::Ui {
            op: "confirm".to_string(),
        }))
        .await;
    assert!(matches!(outcome, HostcallOutcome::Success(_)));
    assert_eq!(services.calls.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn pump_once_delivers_log_hostcall_end_to_end() {
    let services = Arc::new(MockServices {
        calls: Mutex::new(Vec::new()),
        approve: false,
    });
    let thread = test_thread(services, policy(ExtensionPolicyMode::Permissive)).await;
    // Fire-and-forget JS hostcall: enqueues HostcallKind::Log.
    thread
        .runtime()
        .eval("pi.log({ msg: 'hello from test' }); 42")
        .await
        .expect("eval");
    assert_eq!(thread.runtime().pending_hostcall_count(), 1);
    // Pump until quiescent: Log is dispatched inline and completed.
    thread.run().await.expect("run to quiescence");
    assert!(!thread.runtime().has_pending());
}

#[tokio::test(flavor = "current_thread")]
async fn pump_once_routes_tool_hostcall_to_services() {
    let services = Arc::new(MockServices {
        calls: Mutex::new(Vec::new()),
        approve: false,
    });
    let thread = test_thread(services.clone(), policy(ExtensionPolicyMode::Permissive)).await;
    thread
        .runtime()
        .eval("pi.tool('web_search', { query: 'x' }); 7")
        .await
        .expect("eval");
    thread.run().await.expect("run to quiescence");
    let calls = services.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].starts_with("tool:") && calls[0].contains("web_search"));
}
