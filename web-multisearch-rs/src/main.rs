//! web-multisearch MCP server(Rust 版)—— 单 exe、零运行时依赖。
//!
//! 与 Python 版(BoenMind 仓外 boenmind-mcp-servers/web-multisearch)等价:
//! 工具面 `web_search_lite`(免费四源)/ `web_search_all`(12 源),RRF 融合
//! 排序 + CJK 镜像合并去重 + 逗号多 Key 401/403/429 轮换;manifest 标注
//! readOnlyHint → BoenMind 侧 approval=not-required(默认直通)。
//!
//! 协议:MCP 2024-11-05,JSON-RPC over stdio(逐行),手写零 SDK。
//! 配置:`--config <json>`(BoenMind 传 config/mcp-web_multisearch.json);
//! 文件按 mtime 热读,设置页改 Key 下一次搜索立即生效,无需重启。
//! env 兜底:SERPER_API_KEY / JINA_API_KEY / ... (同名大写)。

mod config;
mod fusion;
mod keys;
mod sources;

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use config::Config;
use sources::{aggregate, resolve, ALL_SOURCES, LITE_SOURCES};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "web-multisearch";
const SERVER_VERSION: &str = "0.2.0";

struct Ctx {
    cfg: Mutex<Config>,
    client: reqwest::Client,
}

/// 自描述声明:插件目录扫描的识别载体。name/标题/描述/config_schema 与
/// Python 版 manifest.json 逐字对齐;`args` 模板中的 `{config_file}` 由
/// 批准方(BoenMind webadmin)替换为实际数据目录配置路径。
fn self_description() -> Value {
    let schema = json!([
        {"key": "searxng_url", "label": "SearXNG 实例地址", "type": "string", "default": "",
         "hint": "自托管 SearXNG 的地址(JSON API);留空则 searxng 源禁用"},
        {"key": "serper_api_key", "label": "Serper API Key(Google SERP)", "type": "secret", "default": "",
         "hint": "逗号分隔多把,401/403/429 自动轮换"},
        {"key": "jina_api_key", "label": "Jina API Key", "type": "secret", "default": "",
         "hint": "搜索+正文抓取(Reader);免费额度可用"},
        {"key": "tavily_api_key", "label": "Tavily API Key", "type": "secret", "default": "",
         "hint": "每月 1000 次免费"},
        {"key": "exa_api_key", "label": "Exa API Key", "type": "secret", "default": "",
         "hint": "语义搜索"},
        {"key": "brave_api_key", "label": "Brave Search API Key", "type": "secret", "default": "",
         "hint": "每月 2000 次免费"},
        {"key": "langsearch_api_key", "label": "LangSearch API Key", "type": "secret", "default": "", "hint": ""},
        {"key": "linkup_api_key", "label": "Linkup API Key", "type": "secret", "default": "", "hint": ""},
        {"key": "you_api_key", "label": "You.com API Key", "type": "secret", "default": "", "hint": ""},
        {"key": "websearchapi_api_key", "label": "WebSearchAPI Key", "type": "secret", "default": "", "hint": ""},
        {"key": "default_limit", "label": "默认返回条数", "type": "range", "min": 1, "max": 20, "default": 5},
    ]);
    json!({
        "name": "web_multisearch",
        "title": "聚合搜索(12 源)",
        "description": "并行调用全部搜索源,RRF 融合排序+CJK 同题镜像合并去重,多 Key 自动轮换。工具:web_search_lite(免费四源)/web_search_all(全源)。",
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
            // 自描述(两段式接入的"扫描发现"基础,2026-09-02 用户批准):
            // 打印声明 JSON 后退出——BoenMind 插件目录扫描据此识别候选,
            // 用户在管理界面点「批准接入」后才落盘 mcp.json(显式批准=安装)。
            "--self-describe" => {
                let mut out =
                    serde_json::to_string_pretty(&self_description()).expect("声明序列化");
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

    let ctx = Arc::new(Ctx {
        cfg: Mutex::new(Config::new(config_path)),
        client: reqwest::Client::builder()
            .user_agent(format!("{SERVER_NAME}/{SERVER_VERSION}"))
            .build()
            .expect("HTTP 客户端构造"),
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
    let (limit, resolved) = {
        let mut cfg = ctx.cfg.lock().expect("配置锁");
        let limit = cfg.resolve_limit(args_limit);
        (limit, resolve(&mut cfg))
    };
    let source_names: &[&str] = if name == "web_search_lite" {
        &LITE_SOURCES
    } else {
        &ALL_SOURCES
    };
    let mode = if name == "web_search_lite" {
        "web-multisearch-lite"
    } else {
        "web-multisearch"
    };
    aggregate(&ctx.client, &resolved, mode, source_names, &query, limit).await
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
            "description": "全网搜:并行调用所有已配置搜索源(最多 12 家),RRF 融合排序+镜像合并,meta 带各源耗时遥测。用户要求「全网搜」、需要最大覆盖或交叉验证时使用。",
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
        // 工具约定:错误也返回 JSON 文本,不抛(与 Python 版一致)
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
}

#[cfg(test)]
mod self_describe_tests {
    use super::*;

    #[test]
    fn declaration_shape_matches_contract() {
        let d = self_description();
        assert_eq!(d["name"], "web_multisearch");
        assert!(!d["title"].as_str().unwrap().is_empty());
        assert_eq!(d["config_schema"].as_array().unwrap().len(), 11);
        assert_eq!(
            d["suggested_entry"]["args"][0].as_str().unwrap(),
            "--config"
        );
        assert!(d["suggested_entry"]["args"][1]
            .as_str()
            .unwrap()
            .contains("{config_file}"));
    }
}
