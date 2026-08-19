//! rquickjs 内嵌引擎 + host 注册（全局 `host` 对象）+ JS 执行与异步泵。
//!
//! 落地顺序 §5.2：把 [`HostApi`] trait 注册进 rquickjs 全局 `host`，异步泵打通。
//!
//! # 异步泵架构（rquickjs 0.6 无 Node 事件循环，宿主必须驱动 Promise）
//!
//! rquickjs 0.6 的 `Async` fn 注册成 JS Promise 后，内部把 Rust future 压进
//! **runtime 的 spawner**，只能由 `AsyncContext` + `AsyncRuntime::drive()`/`idle()`
//! 驱动（常规 `Ctx::eval` 注释明说 "futures are not polled"）。因此：
//!
//! - **引擎 = `AsyncContext`，跑在专用异步线程的独立 tokio runtime 上**；
//!   该 runtime 常驻 `tokio::spawn(rt.drive())` 泵 JS 任务（同 rquickjs 官方
//!   `async_test_case` 的 `drive` 用法）。
//! - **HostApi 调用（tools_invoke / llm_complete_stream 等）**在 JS 插件线程上
//!   同步 `block_on` 到**宿主** tokio runtime 执行——与泵线程的独立 runtime
//!   分离，杜绝跨 runtime deadlock。
//! - JS 插件编排：`host.llm.complete(req)` 等返回 Promise，`await` 即串行等待；
//!   泵线程持续 poll，宿主调用完成后 resolve。
//!
//! # 跨桥类型（JSON 字符串，规避 rquickjs 0.6 闭包 lifetime 坑）
//!
//! rquickjs 0.6 的 `IntoJsFunc` 要求闭包返回不携带 `'js` lifetime 的类型；
//! `Value<'js>`/`Ctx<'js>` 在带捕获闭包里推断不出统一 lifetime。因此 host 面
//! 原始函数**统一返回 JSON 字符串**（`Result<String>`），JS 侧包装层 `JSON.parse`
//! 还原成对象——对齐官方 dsh 生态 `JSON.stringify` 风格，纯逻辑插件可接受。
//!
//! # host 注册面（JS 插件可见）
//!
//! - `host.log(level, msg)` 同步
//! - `host.config.get(plugin_id, key)` 同步
//! - `host.tools.list()` 同步 / `host.tools.invoke(name, args)` 异步
//! - `host.llm.complete(req)` 异步
//! - `host.session.append(session_id, event)` / `get(session_id)` / `poll(session_id, cursor)` 同步
//!
//! 不注册 `host.agent.step`（JS 当 Tool/Policy，不当第二 Agent）。

use std::sync::Arc;

use rquickjs::function::Async;
use rquickjs::prelude::Func;
use rquickjs::{async_with, AsyncContext, AsyncRuntime, Function};

use crate::HostApi;

/// 桥运行时：AsyncContext 引擎 + 宿主 API 实现。
pub struct JsBridge {
    /// 持有宿主引用（Arc 生命周期与桥一致；注册进 JS 的函数各自再 clone）。
    #[allow(dead_code)]
    host: Arc<dyn HostApi>,
    /// JS 专用线程的 tokio runtime（跑 AsyncContext + drive 泵）。
    rt: tokio::runtime::Runtime,
    ctx: AsyncContext,
}

/// JS 侧 host 包装层：原始 `__host_*` 函数返回 JSON 字符串，这里还原成对象。
///
/// `__host_tools_invoke` / `__host_llm_complete` 是 `Async` fn（返回 JS Promise），
/// 包装方法必须是 async（返回 Promise），由调用方 `await`——QuickJS 无同步等待。
const HOST_WRAPPER: &str = r#"
globalThis.host = {
  log: (level, msg) => __host_log(level, msg),
  config: {
    get: (pluginId, key) => JSON.parse(__host_config_get(pluginId, key)),
  },
  tools: {
    list: () => JSON.parse(__host_tools_list()),
    invoke: async (name, args) => JSON.parse(await __host_tools_invoke(name, JSON.stringify(args))),
  },
  llm: {
    complete: async (req) => JSON.parse(await __host_llm_complete(JSON.stringify(req))),
  },
  session: {
    append: (sessionId, event) => JSON.parse(__host_session_append(sessionId, JSON.stringify(event))),
    get: (sessionId) => JSON.parse(__host_session_get(sessionId)),
    poll: (sessionId, cursor) => JSON.parse(__host_session_poll(sessionId, cursor)),
  },
};
"#;

impl JsBridge {
    /// 创建引擎并把 `host` 注册进 JS 全局对象（`globalThis.host`）。
    ///
    /// 引擎跑在专用异步线程的独立 tokio runtime 上；`rt.drive()` 常驻泵 JS 任务。
    pub fn new(host: Arc<dyn HostApi>) -> Result<Self, String> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .thread_name("quickjs-pump")
            .enable_all()
            .build()
            .map_err(|e| format!("create JS runtime: {e}"))?;

        let js_rt = AsyncRuntime::new().map_err(|e| format!("create QuickJS runtime: {e}"))?;
        let ctx = rt
            .block_on(AsyncContext::full(&js_rt))
            .map_err(|e| format!("create QuickJS context: {e}"))?;

        // 泵线程：驱动 spawner 里所有 JS 任务（Async fn 的 Promise）。与宿主 runtime 分离。
        rt.spawn(js_rt.drive());

        // 各 host 面用的 Arc 克隆：`async_with!` 闭包体是 async move，闭包内 clone 会整体
        // move 捕获 `host`，故在此备齐，闭包体只引用这些局部变量。
        let h_log = host.clone();
        let h_config = host.clone();
        let h_tools = host.clone();
        let h_tools_invoke = host.clone();
        let h_llm = host.clone();
        let h_sess_append = host.clone();
        let h_sess_get = host.clone();
        let h_sess_poll = host.clone();

        rt.block_on(async_with!(ctx => |ctx| {
            let globals = ctx.globals();

            let log = Func::from(move |level: String, msg: String| {
                h_log.log(&level, &msg);
                rquickjs::Result::Ok(())
            });
            globals.set("__host_log", log).map_err(|e| e.to_string())?;

            let config_get = Func::from(move |plugin_id: String, key: String| {
                let r = h_config.config_get(&plugin_id, &key);
                to_json_string(&r)
            });
            globals.set("__host_config_get", config_get).map_err(|e| e.to_string())?;

            let tools_list = Func::from(move || {
                let r = h_tools.tools_list();
                to_json_string(&r)
            });
            globals.set("__host_tools_list", tools_list).map_err(|e| e.to_string())?;

            let tools_invoke = Function::new(
                ctx.clone(),
                Async(move |name: String, args_json: String| {
                    // 异步工具调用：JSON 字符串参数 → serde_json::Value → HostApi。
                    let host = h_tools_invoke.clone();
                    async move {
                        let parsed: serde_json::Value =
                            serde_json::from_str(&args_json).map_err(host_err)?;
                        let r = host.tools_invoke(&name, parsed).await;
                        to_json_string(&r)
                    }
                }),
            )
            .map_err(|e| e.to_string())?;
            globals
                .set("__host_tools_invoke", tools_invoke)
                .map_err(|e| e.to_string())?;

            let llm_complete = Function::new(
                ctx.clone(),
                Async(move |req_json: String| {
                    let host = h_llm.clone();
                    async move {
                        let parsed: serde_json::Value =
                            serde_json::from_str(&req_json).map_err(host_err)?;
                        let request: crate::CompleteRequest =
                            serde_json::from_value(parsed).map_err(host_err)?;
                        let cancel = crate::Cancellation::new();
                        let r = host.llm_complete_stream(request, cancel).await;
                        to_json_string(&r)
                    }
                }),
            )
            .map_err(|e| e.to_string())?;
            globals
                .set("__host_llm_complete", llm_complete)
                .map_err(|e| e.to_string())?;

            let session_append = Func::from(move |session_id: String, event_json: String| {
                let parsed: serde_json::Value =
                    serde_json::from_str(&event_json).map_err(host_err)?;
                let r = h_sess_append.session_append(&session_id, parsed);
                to_json_string(&r)
            });
            globals
                .set("__host_session_append", session_append)
                .map_err(|e| e.to_string())?;

            let session_get = Func::from(move |session_id: String| {
                let r = h_sess_get.session_get(&session_id);
                to_json_string(&r)
            });
            globals
                .set("__host_session_get", session_get)
                .map_err(|e| e.to_string())?;

            let session_poll = Func::from(move |session_id: String, cursor: u64| {
                let r = h_sess_poll.session_poll(&session_id, cursor);
                to_json_string(&r)
            });
            globals
                .set("__host_session_poll", session_poll)
                .map_err(|e| e.to_string())?;

            // JS 包装层：把 JSON 字符串还原成对象，暴露优雅 host 面。
            ctx.eval::<(), _>(HOST_WRAPPER)
                .map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        }))
        .map_err(|e| e.to_string())?;

        Ok(Self { host, rt, ctx })
    }

    /// 执行一段 JS（同步）。若脚本含 `await`（顶层 Promise），请用 [`Self::exec_async`]。
    pub fn exec(&self, code: &str) -> Result<(), String> {
        let code = code.to_string();
        let res = self.rt.block_on(async_with!(self.ctx => |ctx| {
            ctx.eval::<(), _>(code).map_err(|e| e.to_string())
        }));
        res.map_err(|e| format!("exec JS: {e}"))
    }

    /// 执行一段 JS 并驱动顶层 Promise（异步泵）。
    ///
    /// QuickJS 无 Node 事件循环；脚本里 `await host.llm.complete(...)` 由泵线程驱动。
    pub fn exec_async(&self, code: &str) -> Result<(), String> {
        let code = code.to_string();
        let res = self.rt.block_on(async_with!(self.ctx => |ctx| {
            let promise = ctx.eval_promise(code).map_err(|e| e.to_string())?;
            promise
                .into_future::<()>()
                .await
                .map_err(|e| e.to_string())
        }));
        res.map_err(|e| format!("exec JS: {e}"))
    }

    /// 同步执行一段 JS 表达式并返回其值（JS 值 → JSON）。
    ///
    /// 与 [`Self::exec`] 的区别：`exec` 丢弃返回值，`eval_value` 取表达式结果。
    /// rquickjs 0.6 的异步求值（`eval_promise`）resolve 出的是模块命名空间而非
    /// 脚本值，因此**异步结果一律走全局变量 + 此方法读回**（见 [`Self::call_async`]）。
    pub fn eval_value(&self, code: &str) -> Result<serde_json::Value, String> {
        let code = code.to_string();
        let res = self.rt.block_on(async_with!(self.ctx => |ctx| {
            let v: rquickjs::Value = ctx.eval(code).map_err(|e| e.to_string())?;
            js_value_to_json(&v)
        }));
        res
    }

    /// 调用一个已注册的全局 JS 函数（含异步，函数返回 Promise 时自动 await）。
    ///
    /// 实现：脚本**顶层 await** 调用表达式并把结果写入 `globalThis.__result`，
    /// `exec_async`（`JS_EVAL_FLAG_ASYNC`）等到模块 promise 完成，再 `eval_value`
    /// 同步读回——规避 rquickjs 0.6 `eval_promise` 返回值是模块命名空间的坑。
    /// 注意不能用 `(async () => {...})()` IIFE：它不被模块顶层 await，`exec_async`
    /// 会在 IIFE 完成前返回。参数经 JSON 序列化拼进调用表达式。
    pub fn call_async(
        &self,
        name: &str,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        let expr = build_call_expr(name, args);
        let script = format!("globalThis.__result = await ({expr});");
        self.exec_async(&script)?;
        self.eval_value("globalThis.__result")
    }
}

impl std::fmt::Debug for JsBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsBridge").finish_non_exhaustive()
    }
}

/// `globalThis.<name>(<json args>)`——serde_json 的 JSON 输出是合法 JS 表达式。
fn build_call_expr(name: &str, args: &[serde_json::Value]) -> String {
    let args = args
        .iter()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("globalThis.{name}({args})")
}

/// HostResult → JSON 字符串（跨桥统一形态）。
fn to_json_string(r: &crate::HostResult) -> rquickjs::Result<String> {
    serde_json::to_string(r).map_err(host_err)
}

/// Rust 错误 → rquickjs JS 异常（闭包返回类型 `Result<T, Error>` 自动抛 JS）。
fn host_err(e: impl std::fmt::Display) -> rquickjs::Error {
    rquickjs::Error::new_into_js_message("host", "js", e.to_string())
}

/// JS 值 → serde_json::Value（仅 JSON 可序列化值；undefined/function 报错）。
fn js_value_to_json(v: &rquickjs::Value) -> Result<serde_json::Value, String> {
    use rquickjs::Type;
    Ok(match v.type_of() {
        Type::Null | Type::Undefined => serde_json::Value::Null,
        Type::Bool => serde_json::Value::Bool(v.as_bool().unwrap_or(false)),
        Type::Int => serde_json::Value::Number(serde_json::Number::from(v.as_int().unwrap_or(0))),
        Type::Float => serde_json::Number::from_f64(v.as_float().unwrap_or(0.0))
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Type::String => serde_json::Value::String(
            v.as_string()
                .map(|s| s.to_string().unwrap_or_default())
                .unwrap_or_default(),
        ),
        Type::Array => {
            let arr = v.as_array().ok_or("not an array")?;
            let mut out = Vec::new();
            for item in arr.iter() {
                let item = item.map_err(|e| e.to_string())?;
                out.push(js_value_to_json(&item)?);
            }
            serde_json::Value::Array(out)
        }
        Type::Object => {
            let obj = v.as_object().ok_or("not an object")?;
            let mut map = serde_json::Map::new();
            for item in obj.props::<String, rquickjs::Value>() {
                let (key, val) = (item.as_ref().map_err(|e| e.to_string())?.0.clone(),
                    js_value_to_json(&item.map_err(|e| e.to_string())?.1)?);
                map.insert(key, val);
            }
            serde_json::Value::Object(map)
        }
        Type::Function | Type::Symbol | Type::BigInt => {
            return Err(format!("non-JSON value: {:?}", v.type_of()));
        }
        _ => return Err(format!("non-JSON value: {:?}", v.type_of())),
    })
}
