//! B6 — event dispatch path: push a named event to extension handlers
//! (`pi.on("tool_result", ...)` / `pi.on("startup", ...)`) through the
//! `__pi_dispatch_extension_event` bridge and pump until handlers settle.
//!
//! Mirrors legacy `dispatch_extension_event_phase_sharded`
//! (legacy/pi_agent_rust/src/extensions.rs:13867), single-runtime form:
//! the event dispatch promise is wrapped in a JS task (`__pi_task_start`),
//! then the B3 [`await_js_task`] pump loop drives hostcalls + the event
//! loop until the handlers resolve, reject or time out.
//!
//! Return value: the handler chain's last non-undefined return (event
//! handlers may transform/block — the host interprets it per event);
//! `serde_json::Value::Null` when no handler was registered or all returned
//! `undefined`.
//!
//! Call convention note (empirically verified against `__pi_execute_tool`
//! in tests/execute.rs): the first tuple element passed to
//! `Function::call` does not bind to a JS formal parameter — the JS bridge
//! entrypoints are declared without the bridge secret, and the trailing
//! `(bridge_secret, …)` prefix in the Rust call site matches the existing
//! `execute_tool`/`load_extension` convention. Keep it identical here.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::extensions_js::json_to_js;
use crate::host::HostThread;
use crate::load::{await_js_task, next_task_id};
use crate::scheduler::Clock as SchedulerClock;

/// Dispatch one event to the extension handlers registered for
/// `event_name` (all phases: direct `pi.on` hooks + event-bus hooks).
///
/// `event_payload` is delivered as the handler's first argument (its `type`
/// field, when present, is preserved verbatim — mirroring the legacy tagged
/// serialization); `ctx_payload` feeds the JS `__pi_make_extension_ctx`
/// (`{ cwd, hasUI, sessionEntries, … }` — see extensions_js.rs). Handlers
/// without session context pass `serde_json::json!({})`; the bridge
/// tolerates the empty shape.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_extension_event<C: SchedulerClock + 'static>(
    thread: &HostThread<C>,
    event_name: &str,
    event_payload: serde_json::Value,
    ctx_payload: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let runtime = thread.runtime();
    let task_id = next_task_id("task-event");

    let bridge_secret = runtime.bridge_secret().to_string();
    let event_name_owned = event_name.to_string();
    let event_owned = event_payload.clone();
    let ctx_owned = ctx_payload.clone();
    let bootstrap = runtime
        .with_ctx(|ctx| {
            let global = ctx.globals();
            let dispatch_fn: rquickjs::Function<'_> = global.get("__pi_dispatch_extension_event")?;
            let task_start: rquickjs::Function<'_> = global.get("__pi_task_start")?;
            let event_js = json_to_js(&ctx, &event_owned)?;
            let ctx_js = json_to_js(&ctx, &ctx_owned)?;
            let promise: rquickjs::Value<'_> = dispatch_fn.call((
                bridge_secret.as_str(),
                event_name_owned,
                event_js,
                ctx_js,
            ))?;
            let _task: String = task_start.call((bridge_secret.as_str(), task_id.as_str(), promise))?;
            Ok(())
        })
        .await;
    bootstrap.map_err(|err| {
        Error::extension(format!(
            "Failed to dispatch event '{event_name}': {err}"
        ))
    })?;

    await_js_task(thread, &task_id, timeout).await
}
