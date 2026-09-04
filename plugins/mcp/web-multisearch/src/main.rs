//! web-multisearch MCP server(Rust 版)—— 单 exe、零运行时依赖。
//!
//! 工具面 `web_search_lite`(免费四源)/ `web_search_all`(全部已配置源)。
//! 2026-09-04 起供应商可扩展:内置 12 家默认模板预填,设置页可新增全新
//! 供应商(接口地址 / 方式 / key 传法 / 参数名 / 结果路径 / 字段映射),
//! 新增供应商走通用 JSON 适配器。月度用量按供应商记账(usage.json)。
//!
//! 协议:MCP 2024-11-05,JSON-RPC over stdio(逐行),手写零 SDK。
//! 额外 JSON-RPC 方法(供 BoenMind 管理面用,非 MCP 标准):
//! - `web_search_test`  params: { provider_id, query, limit? } → 单源真搜索
//! - `web_usage`       params: {} → { month, providers: {id: used} }
//!
//! 配置:`--config <json>`(BoenMind 传 config/mcp-web_multisearch.json);
//! 文件按 mtime 热读,设置页改动下一次搜索立即生效,无需重启。

mod cascade;
mod config;
mod fusion;
mod keys;
mod sources;
mod usage;

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use cascade::{is_available, resolve_providers, provider_keys};
use config::Config;
use sources::aggregate;
use usage::UsageLedger;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "web-multisearch";
const SERVER_VERSION: &str = "0.3.0";

/// 免费四源(lite 工具专用;按内置 id 恒有)。
const LITE_IDS: [&str; 4] = ["searxng", "ddgs", "jina", "marginalia"];

struct Ctx {
    cfg: Mutex<Config>,
    client: reqwest::Client,
    usage: Mutex<UsageLedger>,
}

/// 自描述声明:插件目录扫描的识别载体。
///
/// config_schema 从「扁平 11 字段」改为新 `providers` 描述:
/// 单条 `type:"providers"` 项,`items` 载内置 12 家默认模板(id/name/builtin/
/// endpoint/method/auth/auth_name/query_param/limit_param/results_path/
/// title_field/url_field/desc_field/parse/quota)。BoenMind 设置页据此渲染
/// 下拉式供应商列表 + 每家可编辑字段 + 用量进度条。
fn self_description() -> Value {
    let templates: Vec<Value> = cascade::builtin_templates()
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "builtin": p.builtin,
                "endpoint": p.endpoint,
                "method": p.method,
                "auth": p.auth,
                "auth_name": p.auth_name,
                "query_param": p.query_param,
                "limit_param": p.limit_param,
                "results_path": p.results_path,
                "title_field": p.title_field,
                "url_field": p.url_field,
                "desc_field": p.desc_field,
                "parse": p.parse,
                "quota": p.quota,
            })
        })
        .collect();
    let schema = json!([
        {
            "key": "providers",
            "label": "搜索供应商",
            "type": "providers",
            "items": templates,
            "hint": "可选内置 12 家,或点「新增」接入全新搜索服务(通用引擎:接口地址/方式/key/参数名/结果字段)",
        },
        {
            "key": "default_limit",
            "label": "默认返回条数",
            "type": "range",
            "min": 1,
            "max": 20,
            "default": 5,
        },
    ]);
    json!({
        "name": "web_multisearch",
        "title": "聚合搜索(可扩展供应商)",
        "description": "并行调用全部已配置搜索源,RRF 融合排序+CJK 同题镜像合并去重,多 Key 自动轮换。供应商可扩展:内置 12 家+自定义通用引擎。工具:web_search_lite(免费四源)/web_search_all(全源)。",
        "config_schema": schema,
        "suggested_entry": {
            "transport": "stdio",
            "args": ["--config", "{config_file}"],
            "tool_timeout_ms": 30000,
            "restart_limit": 3,
        },
    })
}

#[tokio::main]
async fn main() {
    let mut config_path: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => {
                config_path = args.next().map(std::path::PathBuf::from);
            }
            "--self-describe" => {
                let mut out = serde_json::to_string(&self_description()).expect("声明序列化");
                out.push('\n');
                print!("{out}");
                return;
            }
            other => {
                eprintln!(
                    "[{SERVER_NAME}] 未知参数:{other}(支持 --config <json> / --self-describe)"
                );
            }
        }
    }
    eprintln!(
        "[{SERVER_NAME}] v{SERVER_VERSION} 启动;config={}",
        config_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(未指定)".into())
    );

    let usage = UsageLedger::from_config_path(config_path.as_deref());
    let ctx = Arc::new(Ctx {
        cfg: Mutex::new(Config::new(config_path)),
        client: reqwest::Client::builder()
            .user_agent(format!("{SERVER_NAME}/{SERVER_VERSION}"))
            .build()
            .expect("HTTP 客户端构造"),
        usage: Mutex::new(usage),
    });

    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[{SERVER_NAME}] 无法解析的输入行:{e}");
                continue;
            }
        };
        if let Some(resp) = handle_request(&msg, &ctx).await {
            let mut out = serde_json::to_string(&resp).expect("响应序列化");
            out.push('\n');
            if let Err(e) = stdout.write_all(out.as_bytes()).await {
                eprintln!("[{SERVER_NAME}] stdout 写入失败,退出:{e}");
                break;
            }
            let _ = stdout.flush().await;
        }
    }
}

/// 处理一条 JSON-RPC 消息:请求返回应答,通知返回 None。
async fn handle_request(msg: &Value, ctx: &Ctx) -> Option<Value> {
    let id = match msg.get("id") {
        Some(Value::Null) | None => return None, // 通知:无应答
        Some(other) => other.clone(),
    };
    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_defs() })),
        "tools/call" => {
            let name = msg
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments = msg
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if name != "web_search_lite" && name != "web_search_all" {
                Err((-32602, format!("未知工具:{name}")))
            } else {
                let out = run_tool(ctx, &name, &arguments).await;
                Ok(json!({
                    "content": [{"type": "text", "text": serde_json::to_string_pretty(&out).expect("结果序列化")}],
                    "structuredContent": out,
                    "isError": false,
                }))
            }
        }
        // 管理面扩展:单源真搜索测试(返回真实结果)
        "web_search_test" => {
            let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
            let out = run_search_test(ctx, &params).await;
            Ok(json!({
                "content": [{"type": "text", "text": serde_json::to_string_pretty(&out).expect("结果序列化")}],
                "structuredContent": out,
                "isError": false,
            }))
        }
        // 管理面扩展:读月度用量
        "web_usage" => {
            // 先解析全部 provider(内置+自定义),不持配置锁时再取用量,避免死锁
            let ids: Vec<String> = {
                let mut cfg = ctx.cfg.lock().expect("配置锁");
                resolve_providers(&mut cfg).into_iter().map(|p| p.id).collect()
            };
            let usage = ctx.usage.lock().expect("用量锁");
            let mut by_id = json!({});
            if let Some(o) = by_id.as_object_mut() {
                for id in &ids {
                    o.insert(id.clone(), json!(usage.used(id)));
                }
            }
            Ok(json!({
                "month": usage.month(),
                "providers": by_id,
            }))
        }
        other => Err((-32601, format!("方法不存在:{other}"))),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": code, "message": message},
        }),
    })
}

/// web_search_lite / web_search_all:聚合全部已配置可用源。
async fn run_tool(ctx: &Ctx, name: &str, arguments: &Value) -> Value {
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if query.is_empty() {
        return json!({"success": false, "error": "query 参数不能为空"});
    }
    let args_limit = arguments.get("limit").and_then(Value::as_i64);
    let (limit, providers) = {
        let mut cfg = ctx.cfg.lock().expect("配置锁");
        let limit = cfg.resolve_limit(args_limit);
        let mut providers = resolve_providers(&mut cfg);
        // lite 工具只保留免费四源
        if name == "web_search_lite" {
            providers.retain(|p| LITE_IDS.iter().any(|s| *s == p.id.as_str()));
        }
        (limit, providers)
    };
    let mode = if name == "web_search_lite" {
        "web-multisearch-lite"
    } else {
        "web-multisearch"
    };
    let out = aggregate(&ctx.client, &providers, mode, &query, limit).await;
    // 用量:success 且 sources_ok 里出现过的 provider 才记次数
    if out.get("success").and_then(Value::as_bool) == Some(true) {
        if let Some(ok) = out["meta"]["sources_ok"].as_array() {
            let mut usage = ctx.usage.lock().expect("用量锁");
            for id in ok {
                if let Some(pid) = id.as_str() {
                    usage.record(pid);
                }
            }
        }
    }
    out
}

/// 管理面:单源真搜索测试(测试按钮)。
async fn run_search_test(ctx: &Ctx, params: &Value) -> Value {
    let provider_id = params
        .get("provider_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let query = params
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if provider_id.is_empty() || query.is_empty() {
        return json!({
            "success": false,
            "error": "provider_id 与 query 均不能为空"
        });
    }
    let limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .and_then(|l| Some(l.clamp(1, 20) as usize))
        .unwrap_or(5);

    let provider = {
        let mut cfg = ctx.cfg.lock().expect("配置锁");
        resolve_providers(&mut cfg)
            .into_iter()
            .find(|p| p.id == provider_id)
    };
    let Some(provider) = provider else {
        return json!({
            "success": false,
            "error": format!("未知供应商: {provider_id}")
        });
    };
    if !is_available(&provider) {
        let need = if provider.parse == "searxng" {
            "需填写接口地址".to_string()
        } else if provider_keys(&provider).is_empty() && provider.parse != "ddg" && provider.parse != "marginalia" {
            "需填写 API Key".to_string()
        } else {
            "配置未就绪".to_string()
        };
        return json!({
            "success": false,
            "error": format!("{provider_id}: {need}")
        });
    }

    let started = std::time::Instant::now();
    let result = sources::run_source(&ctx.client, &provider, &query, limit).await;
    let ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(items) => {
            {
                let mut usage = ctx.usage.lock().expect("用量锁");
                usage.record(&provider.id);
            }
            json!({
                "success": true,
                "provider_id": provider.id,
                "provider_name": provider.name,
                "timing_ms": ms,
                "count": items.len(),
                "results": items.iter().map(|it| json!({
                    "title": it.title,
                    "url": it.url,
                    "description": it.description,
                })).collect::<Vec<_>>(),
            })
        }
        Err(e) => json!({
            "success": false,
            "provider_id": provider.id,
            "provider_name": provider.name,
            "timing_ms": ms,
            "error": e,
        }),
    }
}

fn tool_defs() -> Vec<Value> {
    let schema = json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "搜索关键词"},
            "limit": {"type": "integer", "description": "返回条数上限(可选,默认取配置 default_limit=5)"},
        },
        "required": ["query"],
    });
    vec![
        json!({
            "name": "web_search_lite",
            "description": "日常聚合搜索:searxng + ddgs + jina + marginalia(全免费源)并行,RRF 融合排序+同题镜像合并去重,description 带 [来源] 标注。一般搜索优先用这个,快且免费。",
            "inputSchema": schema,
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "web_search_all",
            "description": "全网搜:并行调用所有已配置搜索源(内置+自定义),RRF 融合排序+镜像合并,meta 带各源耗时遥测。用户要求「全网搜」、需要最大覆盖或交叉验证时使用。",
            "inputSchema": schema,
            "annotations": {"readOnlyHint": true},
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            cfg: Mutex::new(Config::new(None)),
            client: reqwest::Client::new(),
            usage: Mutex::new(UsageLedger::from_config_path(None)),
        }
    }

    #[tokio::test]
    async fn initialize_shape() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            &ctx(),
        )
        .await
        .expect("应答");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "web-multisearch");
    }

    #[tokio::test]
    async fn notification_gets_no_reply() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            &ctx(),
        )
        .await;
        assert!(resp.is_none());
    }

    #[tokio::test]
    async fn tools_list_two_readonly() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            &ctx(),
        )
        .await
        .expect("应答");
        let tools = resp["result"]["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "web_search_lite");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
        assert_eq!(tools[1]["name"], "web_search_all");
        assert_eq!(tools[1]["annotations"]["readOnlyHint"], true);
    }

    #[tokio::test]
    async fn call_empty_query_returns_tool_json_error() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
                    "params":{"name":"web_search_lite","arguments":{"query":"   "}}}),
            &ctx(),
        )
        .await
        .expect("应答");
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        let parsed: Value = serde_json::from_str(text).expect("text 应为 JSON");
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "query 参数不能为空");
        assert_eq!(resp["result"]["structuredContent"]["success"], false);
    }

    #[tokio::test]
    async fn unknown_tool_is_invalid_params() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                    "params":{"name":"nope","arguments":{}}}),
            &ctx(),
        )
        .await
        .expect("应答");
        assert_eq!(resp["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":5,"method":"resources/list"}),
            &ctx(),
        )
        .await
        .expect("应答");
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn web_usage_returns_month_and_providers() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":6,"method":"web_usage","params":{}}),
            &ctx(),
        )
        .await
        .expect("应答");
        assert_eq!(resp["result"]["month"].as_str().unwrap().len(), 7);
        assert!(resp["result"]["providers"]["serper"].is_u64());
    }

    #[tokio::test]
    async fn web_search_test_unknown_provider() {
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":7,"method":"web_search_test",
                    "params":{"provider_id":"nope","query":"hi"}}),
            &ctx(),
        )
        .await
        .expect("应答");
        let out = &resp["result"]["structuredContent"];
        assert_eq!(out["success"], false);
        assert!(out["error"].as_str().unwrap().contains("未知供应商"));
    }
}

#[cfg(test)]
mod self_describe_tests {
    use super::*;

    #[test]
    fn declaration_shape_matches_contract() {
        let d = self_description();
        assert_eq!(d["name"], "web_multisearch");
        assert!(!d["title"].as_str().unwrap().is_empty());
        // config_schema 现为 providers 类型 + default_limit
        let schema = d["config_schema"].as_array().unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema[0]["type"], "providers");
        let items = schema[0]["items"].as_array().unwrap();
        assert_eq!(items.len(), 12, "内置 12 家模板");
        assert_eq!(d["suggested_entry"]["args"][0].as_str().unwrap(), "--config");
        assert!(d["suggested_entry"]["args"][1]
            .as_str()
            .unwrap()
            .contains("{config_file}"));
    }

    #[test]
    fn provider_template_has_all_engine_fields() {
        let d = self_description();
        let items = d["config_schema"][0]["items"].as_array().unwrap();
        let first = &items[0];
        for k in [
            "id", "name", "builtin", "endpoint", "method", "auth", "auth_name",
            "query_param", "limit_param", "results_path", "title_field", "url_field",
            "desc_field", "parse", "quota",
        ] {
            assert!(first.get(k).is_some(), "模板缺字段 {k}: {first}");
        }
    }
}