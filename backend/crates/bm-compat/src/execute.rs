//! B4 — tool execution path: invoke a registered extension tool through the
//! `__pi_execute_tool` bridge and pump until it resolves.
//!
//! Mirrors legacy `execute_extension_tool_sharded`
//! (legacy/pi_agent_rust/src/extensions.rs:14353), single-runtime form:
//! the runtime is owned by the host thread so there is no shard routing —
//! `__pi_execute_tool` → `__pi_task_start` wraps the promise in a JS task,
//! then the B3 [`await_js_task`] pump loop drives hostcalls + the event loop
//! until the tool resolves, rejects or times out.
//!
//! Return value: the tool's JSON result (the promise resolution of the
//! plugin's `execute` — typically `{content: [...], details: ...}`);
//! `Err` on unknown tool / validation failure / task rejection / timeout.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::extensions_js::json_to_js;
use crate::host::HostThread;
use crate::load::{await_js_task, next_task_id};
use crate::scheduler::Clock as SchedulerClock;

/// Execute one registered extension tool.
///
/// `ctx_payload` is fed into the JS `__pi_make_extension_ctx` (session /
/// extension runtime context seen by the plugin's `execute`). Callers that
/// have no session context pass `serde_json::json!({})` — the bridge
/// tolerates the empty shape.
#[allow(clippy::too_many_arguments)]
pub async fn execute_tool<C: SchedulerClock + 'static>(
    thread: &HostThread<C>,
    tool_name: &str,
    tool_call_id: &str,
    input: serde_json::Value,
    ctx_payload: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let runtime = thread.runtime();
    let task_id = next_task_id("task-tool");

    let bridge_secret = runtime.bridge_secret().to_string();
    let (tool_name_owned, tool_call_id_owned) = (tool_name.to_string(), tool_call_id.to_string());
    let input_owned = input.clone();
    let ctx_owned = ctx_payload.clone();
    let bootstrap = runtime
        .with_ctx(|ctx| {
            let global = ctx.globals();
            let exec_fn: rquickjs::Function<'_> = global.get("__pi_execute_tool")?;
            let task_start: rquickjs::Function<'_> = global.get("__pi_task_start")?;
            let input_js = json_to_js(&ctx, &input_owned)?;
            let ctx_js = json_to_js(&ctx, &ctx_owned)?;
            let promise: rquickjs::Value<'_> = exec_fn.call((
                bridge_secret.as_str(),
                tool_name_owned,
                tool_call_id_owned,
                input_js,
                ctx_js,
            ))?;
            let _task: String = task_start.call((bridge_secret.as_str(), task_id.as_str(), promise))?;
            Ok(())
        })
        .await;
    bootstrap.map_err(|err| {
        Error::extension(format!(
            "Failed to start tool '{tool_name}': {err}"
        ))
    })?;

    await_js_task(thread, &task_id, timeout).await
}
