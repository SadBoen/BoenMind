//! Shared test helpers for bm-compat integration tests.

use std::sync::{Arc, Mutex};

use bm_compat::extensions::ExtensionPolicy;
use bm_compat::host::{HostServices, HostThread};
use bm_compat::scheduler::{HostcallOutcome, WallClock};

/// Mock services: records each routed call, returns a canned outcome.
pub struct MockServices {
    pub calls: Mutex<Vec<String>>,
    pub approve: bool,
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

pub fn mock_services() -> Arc<MockServices> {
    Arc::new(MockServices {
        calls: Mutex::new(Vec::new()),
        approve: false,
    })
}

/// Boot a fresh runtime + host thread on the current-thread executor.
/// Generic over the service impl so test files can inject their own mocks
/// (e.g. session.rs 的 SessionMockServices) alongside the shared MockServices.
pub async fn test_thread<S: HostServices + 'static>(
    services: Arc<S>,
    policy: ExtensionPolicy,
) -> HostThread<WallClock> {
    let runtime = std::rc::Rc::new(
        bm_compat::extensions_js::PiJsRuntime::new()
            .await
            .expect("runtime boot"),
    );
    HostThread::new(runtime, services, policy)
}
