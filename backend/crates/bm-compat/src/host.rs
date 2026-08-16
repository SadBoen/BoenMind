//! B2 — host thread (the host-side pump for the vendored QuickJS engine).
//!
//! Structure mirrors pi_agent_rust `pump_js_runtime_once_for_owner`
//! (pi_agent_rust@44ddf80/src/extensions.rs:14839): drain → policy check +
//! dispatch → `complete_hostcalls_batch` → tick → second drain (catches
//! fire-and-forget hostcalls scheduled during the tick/microtask phase).
//!
//! Execution behaviour for each [`HostcallKind`] is provided by the
//! [`HostServices`] ports (wired to the kernel in B4, approval bridge in B5);
//! this module only owns the skeleton: policy decision, routing, completion
//! delivery. `HostcallKind::Log` is handled inline, same as pi_agent_rust.

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use crate::error::Error;
use crate::extensions::{ExtensionPolicy, ExtensionPolicyMode};
use crate::extensions_js::{HostcallKind, HostcallRequest, PiJsRuntime};
use crate::scheduler::{Clock as SchedulerClock, HostcallOutcome};

/// Policy decision for one capability lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Capability granted.
    Allow,
    /// Capability denied. `reason` distinguishes hard denies (`strict`,
    /// `deny`) from `prompt` (no pre-authorization; routed through
    /// [`HostServices::request_approval`] before failing closed).
    Deny { reason: &'static str },
}

/// Simplified counterpart of pi_agent_rust `PolicySnapshot::lookup`: permissive
/// allows everything, strict denies everything, prompt consults the global
/// tables merged with the per-extension override (deny wins over allow).
pub fn check_capability(
    policy: &ExtensionPolicy,
    capability: &str,
    extension_id: Option<&str>,
) -> PolicyDecision {
    let mode = extension_id
        .and_then(|id| policy.per_extension.get(id))
        .and_then(|override_| override_.mode)
        .unwrap_or(policy.mode);
    match mode {
        ExtensionPolicyMode::Permissive => PolicyDecision::Allow,
        ExtensionPolicyMode::Strict => PolicyDecision::Deny { reason: "strict" },
        ExtensionPolicyMode::Prompt => {
            let override_ = extension_id.and_then(|id| policy.per_extension.get(id));
            let denied = policy.deny_caps.iter().any(|c| c == capability)
                || override_.is_some_and(|o| o.deny.iter().any(|c| c == capability));
            if denied {
                return PolicyDecision::Deny { reason: "deny" };
            }
            let allowed = policy.default_caps.iter().any(|c| c == capability)
                || override_.is_some_and(|o| o.allow.iter().any(|c| c == capability));
            if allowed {
                PolicyDecision::Allow
            } else {
                PolicyDecision::Deny { reason: "prompt" }
            }
        }
    }
}

/// Host-side services the dispatcher routes hostcalls to.
///
/// One async port per [`HostcallKind`] that requires host execution
/// (`Log` is handled by [`HostThread`] directly). B4 wires these to the
/// kernel (tool registry merge point = `bm-loop::ToolRegistry`); B5 replaces
/// [`request_approval`](HostServices::request_approval) with the
/// PermissionBridge.
#[async_trait::async_trait]
pub trait HostServices: Send + Sync {
    /// `pi.tool(name, input)` — invoke a tool.
    async fn execute_tool(
        &self,
        call_id: &str,
        name: &str,
        input: serde_json::Value,
    ) -> HostcallOutcome;

    /// `pi.exec(cmd, args)` — execute a shell command.
    async fn exec(&self, call_id: &str, cmd: &str, payload: serde_json::Value) -> HostcallOutcome;

    /// `pi.http(request)` — make an HTTP request.
    async fn http(&self, call_id: &str, payload: serde_json::Value) -> HostcallOutcome;

    /// `pi.session(op, args)` — session operations.
    async fn session(&self, call_id: &str, op: &str, payload: serde_json::Value)
    -> HostcallOutcome;

    /// `pi.ui(op, args)` — UI operations.
    async fn ui(
        &self,
        call_id: &str,
        op: &str,
        payload: serde_json::Value,
        extension_id: Option<&str>,
    ) -> HostcallOutcome;

    /// `pi.events(op, args)` — event operations.
    async fn events(
        &self,
        call_id: &str,
        op: &str,
        payload: serde_json::Value,
        extension_id: Option<&str>,
    ) -> HostcallOutcome;

    /// Approval channel for `prompt` decisions. Default fails closed
    /// (no pre-authorization = silent deny, mirroring the fail-closed
    /// discipline); B5 overrides this with the PermissionBridge.
    async fn request_approval(&self, capability: &str, extension_id: Option<&str>) -> bool {
        let _ = (capability, extension_id);
        false
    }
}

/// B2 host thread: the host-side pump driving one [`PiJsRuntime`].
///
/// Owns the runtime handle plus the service ports and policy it dispatches
/// against. Threading is left to the caller (B4 kernel wiring) — `pump_once`
/// is the re-entrant primitive, `run` drains to quiescence.
pub struct HostThread<C: SchedulerClock + 'static> {
    runtime: Rc<PiJsRuntime<C>>,
    services: Arc<dyn HostServices>,
    policy: ExtensionPolicy,
}

impl<C: SchedulerClock + 'static> HostThread<C> {
    /// Create a host thread over an existing runtime.
    pub fn new(
        runtime: Rc<PiJsRuntime<C>>,
        services: Arc<dyn HostServices>,
        policy: ExtensionPolicy,
    ) -> Self {
        Self {
            runtime,
            services,
            policy,
        }
    }

    /// Access the underlying runtime (eval/eval_file, registered tools,
    /// event enqueueing — B3 loading path drives through here).
    pub fn runtime(&self) -> &Rc<PiJsRuntime<C>> {
        &self.runtime
    }

    /// 热更新权限档位（审查 2026-08-17：设置页切档后无需重启）。
    pub fn set_policy(&mut self, policy: ExtensionPolicy) {
        self.policy = policy;
    }

    /// Policy decision + routing for a single request (pure in the sense
    /// that it does not touch the runtime queue; unit-testable directly).
    pub async fn dispatch_one(&self, request: &HostcallRequest) -> HostcallOutcome {
        let capability = request.required_capability();
        let decision = check_capability(&self.policy, capability, request.extension_id.as_deref());
        let allowed = match decision {
            PolicyDecision::Allow => true,
            PolicyDecision::Deny { reason: "prompt" } => {
                self.services
                    .request_approval(capability, request.extension_id.as_deref())
                    .await
            }
            PolicyDecision::Deny { .. } => false,
        };
        if !allowed {
            return HostcallOutcome::Error {
                code: "denied".to_string(),
                message: format!("Capability '{capability}' denied by policy"),
            };
        }

        match &request.kind {
            HostcallKind::Tool { name } => {
                self.services
                    .execute_tool(&request.call_id, name, request.payload.clone())
                    .await
            }
            HostcallKind::Exec { cmd } => {
                self.services
                    .exec(&request.call_id, cmd, request.payload.clone())
                    .await
            }
            HostcallKind::Http => {
                self.services
                    .http(&request.call_id, request.payload.clone())
                    .await
            }
            HostcallKind::Session { op } => {
                self.services
                    .session(&request.call_id, op, request.payload.clone())
                    .await
            }
            HostcallKind::Ui { op } => {
                self.services
                    .ui(
                        &request.call_id,
                        op,
                        request.payload.clone(),
                        request.extension_id.as_deref(),
                    )
                    .await
            }
            HostcallKind::Events { op } => {
                self.services
                    .events(
                        &request.call_id,
                        op,
                        request.payload.clone(),
                        request.extension_id.as_deref(),
                    )
                    .await
            }
            HostcallKind::Log => {
                tracing::info!(
                    target: "pi.extension.log",
                    payload = ?request.payload,
                    "Extension log"
                );
                HostcallOutcome::Success(serde_json::json!({ "logged": true }))
            }
        }
    }

    /// Dispatch a drained batch and deliver completions in one scheduler
    /// borrow (`complete_hostcalls_batch`).
    async fn dispatch_batch(&self, pending: VecDeque<HostcallRequest>) {
        if pending.is_empty() {
            return;
        }
        let mut completions = Vec::with_capacity(pending.len());
        for request in pending {
            if !self.runtime.is_hostcall_active(&request.call_id) {
                tracing::debug!(
                    event = "pijs.hostcall.skip_cancelled",
                    call_id = %request.call_id,
                    "Skipping hostcall dispatch because call is no longer pending"
                );
                continue;
            }
            let outcome = self.dispatch_one(&request).await;
            completions.push((request.call_id, outcome));
        }
        if !completions.is_empty() {
            self.runtime.complete_hostcalls_batch(completions);
        }
    }

    /// One host-side pump cycle (pi_agent_rust `pump_js_runtime_once_for_owner`):
    /// drain queued hostcalls, dispatch + complete, advance the event loop,
    /// then catch fire-and-forget hostcalls scheduled during the tick.
    ///
    /// Returns whether the runtime still has pending work (macrotasks,
    /// timers or incomplete hostcalls).
    pub async fn pump_once(&self) -> Result<bool, Error> {
        let pending = self.runtime.drain_hostcall_requests();
        self.dispatch_batch(pending).await;

        let _ = self.runtime.tick().await?;
        let _ = self.runtime.drain_microtasks().await?;

        let after_tick = self.runtime.drain_hostcall_requests();
        let has_after_tick = !after_tick.is_empty();
        self.dispatch_batch(after_tick).await;

        // If we dispatched any hostcalls, run another tick so their
        // completions are delivered and microtasks reach a fixpoint before
        // the caller observes the outcome.
        if has_after_tick {
            let _ = self.runtime.tick().await?;
            let _ = self.runtime.drain_microtasks().await?;
        }

        Ok(self.runtime.has_pending())
    }

    /// Pump until the runtime reaches quiescence.
    pub async fn run(&self) -> Result<(), Error> {
        while self.pump_once().await? {}
        Ok(())
    }
}
