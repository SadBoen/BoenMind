//! B3 — loading path: evaluate a plugin entry file and read back the
//! registered tool surface.
//!
//! Mirrors legacy `load_one_extension` (legacy/pi_agent_rust/src/extensions.rs:13588)
//! and `await_js_task` (19112), simplified for the single-runtime host thread
//! model: register read roots → `__pi_load_extension` bootstrap wrapped in a
//! JS task → pump loop (reusing B2 [`HostThread::pump_once`]) until the task
//! resolves → return the ExtensionBody JSON.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::extensions::safe_canonicalize;
use crate::extensions_js::{json_to_js, js_to_json};
use crate::host::HostThread;
use crate::scheduler::Clock as SchedulerClock;

/// Extension protocol version stamped into load specs (legacy
/// `extensions.rs:1701 pub const PROTOCOL_VERSION: &str = "1.0"`).
pub const PROTOCOL_VERSION: &str = "1.0";

/// Load specification for one extension entry.
// extracted from legacy/pi_agent_rust/src/extensions.rs:10195-10276
// (JsExtensionLoadSpec + from_entry_path；from_manifest 留 B4 安装路径，
// 届时由内核插件清单直接构造字段)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsExtensionLoadSpec {
    pub extension_id: String,
    pub entry_path: PathBuf,
    pub name: String,
    pub version: String,
    pub api_version: String,
}

impl JsExtensionLoadSpec {
    /// Derive a spec from an entry file path. `index.ts` entries take the
    /// parent directory name as extension id; `package.json` next to the
    /// entry supplies name/version when present.
    pub fn from_entry_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::validation(format!(
                "Extension entry does not exist: {}",
                path.display()
            )));
        }

        let entry_path = safe_canonicalize(path);

        let file_stem = entry_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if file_stem.is_empty() {
            return Err(Error::validation(format!(
                "Extension entry has no filename: {}",
                entry_path.display()
            )));
        }

        let extension_id = if file_stem == "index" {
            entry_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            file_stem
        };

        if extension_id.is_empty() {
            return Err(Error::validation(format!(
                "Could not derive extension id from entry path: {}",
                entry_path.display()
            )));
        }

        let mut name = extension_id.clone();
        let mut version = "0.0.0".to_string();

        if let Some(parent) = entry_path.parent() {
            let manifest_path = parent.join("package.json");
            if manifest_path.exists()
                && let Ok(raw) = std::fs::read_to_string(&manifest_path)
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw)
            {
                if let Some(manifest_name) = json.get("name").and_then(serde_json::Value::as_str)
                    && !manifest_name.trim().is_empty()
                {
                    name = manifest_name.trim().to_string();
                }
                if let Some(manifest_version) =
                    json.get("version").and_then(serde_json::Value::as_str)
                    && !manifest_version.trim().is_empty()
                {
                    version = manifest_version.trim().to_string();
                }
            }
        }

        Ok(Self {
            extension_id,
            entry_path,
            name,
            version,
            api_version: PROTOCOL_VERSION.to_string(),
        })
    }
}

fn next_task_id(prefix: &str) -> String {
    static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{id}")
}

/// State returned by the JS `__pi_task_take` bridge.
enum JsTaskTakeResult {
    Missing,
    Pending,
    Resolved(serde_json::Value),
    Rejected {
        code: Option<String>,
        message: String,
        stack: Option<String>,
    },
    /// Fallback for unknown shapes (kept for protocol-compat inspection).
    Snapshot(serde_json::Value),
}

/// Read the state of a JS task started via `__pi_task_start`.
async fn take_js_task_state<C: SchedulerClock + 'static>(
    thread: &HostThread<C>,
    task_id: &str,
) -> Result<JsTaskTakeResult> {
    let bridge_secret = thread.runtime().bridge_secret().to_string();
    thread
        .runtime()
        .with_ctx(|ctx| {
            let global = ctx.globals();
            let take_fn: rquickjs::Function<'_> = global.get("__pi_task_take")?;
            let value: rquickjs::Value<'_> = take_fn.call((bridge_secret.as_str(), task_id))?;
            if value.is_null() || value.is_undefined() {
                return Ok(JsTaskTakeResult::Missing);
            }
            if let Some(obj) = value.as_object()
                && let Ok(status) = obj.get::<_, String>("status")
            {
                match status.as_str() {
                    "pending" => return Ok(JsTaskTakeResult::Pending),
                    "resolved" => {
                        let resolved_js = obj.get::<_, rquickjs::Value<'_>>("value").ok();
                        let resolved_json = if let Some(value) = resolved_js {
                            js_to_json(&value)?
                        } else {
                            serde_json::Value::Null
                        };
                        return Ok(JsTaskTakeResult::Resolved(resolved_json));
                    }
                    "rejected" => {
                        let (code, message, stack) = obj
                            .get::<_, rquickjs::Value<'_>>("error")
                            .ok()
                            .and_then(|error_value| error_value.as_object().cloned())
                            .map_or_else(
                                || (None, "Unknown JS task error".to_string(), None),
                                |error_obj| {
                                    (
                                        error_obj.get::<_, String>("code").ok(),
                                        error_obj.get::<_, String>("message").unwrap_or_else(
                                            |_| "Unknown JS task error".to_string(),
                                        ),
                                        error_obj.get::<_, String>("stack").ok(),
                                    )
                                },
                            );
                        return Ok(JsTaskTakeResult::Rejected {
                            code,
                            message,
                            stack,
                        });
                    }
                    _ => {}
                }
            }
            Ok(JsTaskTakeResult::Snapshot(js_to_json(&value)?))
        })
        .await
}

/// Pump + poll until a JS task resolves, rejects or times out.
/// Legacy `await_js_task` (extensions.rs:19112), single-runtime form.
pub async fn await_js_task<C: SchedulerClock + 'static>(
    thread: &HostThread<C>,
    task_id: &str,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let started_at = Instant::now();

    loop {
        if started_at.elapsed() > timeout {
            return Err(Error::extension(format!(
                "JS task {task_id} timed out after {}ms",
                timeout.as_millis()
            )));
        }

        thread.pump_once().await?;
        match take_js_task_state(thread, task_id).await? {
            JsTaskTakeResult::Resolved(value) => return Ok(value),
            JsTaskTakeResult::Rejected {
                code,
                message,
                stack,
            } => {
                let code = code.unwrap_or_else(|| "unknown".to_string());
                let stack = stack.map_or(String::new(), |s| format!("\n{s}"));
                return Err(Error::extension(format!(
                    "JS task {task_id} rejected ({code}): {message}{stack}"
                )));
            }
            JsTaskTakeResult::Missing
            | JsTaskTakeResult::Pending
            | JsTaskTakeResult::Snapshot(_) => {}
        }
        if !thread.runtime().has_pending() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

/// Load one extension entry into the thread's runtime.
///
/// Registers the entry's directory as a read root, boots the extension
/// through the `__pi_load_extension` bridge, then drives the event loop
/// until the load task completes. The bridge resolves `true` on success —
/// the registered surface (tools, hooks, providers…) is read back via
/// `get_registered_tools` and friends afterwards.
pub async fn load_extension<C: SchedulerClock + 'static>(
    thread: &HostThread<C>,
    spec: &JsExtensionLoadSpec,
) -> Result<serde_json::Value> {
    let runtime = thread.runtime();

    // Register the extension's root directory so `readFileSync` can access
    // bundled assets within the extension's own directory tree (legacy
    // collect_extension_roots_from_paths, single-entry form).
    if let Some(root) = spec.entry_path.parent() {
        runtime.add_extension_root_with_id(root.to_path_buf(), Some(spec.extension_id.as_str()));
    }

    let meta = serde_json::json!({
        "name": spec.name,
        "version": spec.version,
        "apiVersion": spec.api_version,
    });

    // QuickJS module resolver requires forward-slash paths.
    let entry_specifier = spec.entry_path.display().to_string().replace('\\', "/");
    let task_id = next_task_id("task-load");

    let bridge_secret = runtime.bridge_secret().to_string();
    let meta_value = meta.clone();
    let bootstrap = runtime
        .with_ctx(|ctx| {
            let global = ctx.globals();
            let load_fn: rquickjs::Function<'_> = global.get("__pi_load_extension")?;
            let task_start: rquickjs::Function<'_> = global.get("__pi_task_start")?;
            let meta_js = json_to_js(&ctx, &meta_value)?;
            let promise: rquickjs::Value<'_> = load_fn.call((
                bridge_secret.as_str(),
                spec.extension_id.clone(),
                entry_specifier,
                meta_js,
            ))?;
            let _task: String = task_start.call((bridge_secret.as_str(), task_id.as_str(), promise))?;
            Ok(())
        })
        .await;
    bootstrap.map_err(|err| {
        Error::extension(format!(
            "Failed to bootstrap extension {}: {err}",
            spec.extension_id
        ))
    })?;

    await_js_task(thread, &task_id, Duration::from_secs(10)).await
}
