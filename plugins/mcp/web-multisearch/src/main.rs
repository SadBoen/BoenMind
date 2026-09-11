//! web-multisearch MCP server(Rust 版)—— 单 exe、零运行时依赖。
//!
//! 工具面 `web_search_lite`(免费四源)/ `web_search_all`(全部已配置源)。
//! 2026-09-04 起供应商可扩展:内置 13 家默认模板预填,设置页可新增全新
//! 供应商(接口地址 / 方式 / key 传法 / 参数名 / 结果路径 / 字段映射),
//! 新增供应商走通用 JSON 适配器。月度用量按供应商记账(usage.json)。
//!
//! 协议:MCP 2024-11-05,JSON-RPC over stdio,协议循环经 boenmind-plugin-sdk 单源(#56)。
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

use boenmind_plugin_sdk::{
    emit_self_describe_if_requested, run_stdio, McpService, SelfDescribe, ServerInfo,
    SuggestedEntry, ToolDef, ToolOutput,
};
use serde_json::{json, Value};

use cascade::{is_available, provider_keys, resolve_any, resolve_providers};
use config::Config;
use sources::aggregate;
use usage::UsageLedger;

const SERVER_NAME: &str = "web_multisearch";
const SERVER_VERSION: &str = "0.3.0";

/// 免费四源(lite 工具专用;按内置 id 恒有)。
const LITE_IDS: [&str; 4] = ["searxng", "ddgs", "jina", "marginalia"];

struct Ctx {
    cfg: Mutex<Config>,
    client: reqwest::Client,
    usage: Mutex<UsageLedger>,
}

/// 自描述声明(合同 `mcp-self-describe.v0_1`):插件目录扫描的识别载体。
///
/// config_schema 从「扁平 11 字段」改为新 `providers` 描述:
/// 单条 `type:"providers"` 项,`items` 载内置 13 家默认模板(id/name/builtin/
/// endpoint/method/auth/auth_name/query_param/limit_param/results_path/
/// title_field/url_field/desc_field/parse/quota)。BoenMind 设置页据此渲染
/// 下拉式供应商列表 + 每家可编辑字段 + 用量进度条。
fn self_description() -> SelfDescribe {
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
    let schema = vec![
        json!({
            "key": "providers",
            "label": "搜索供应商",
            "type": "providers",
            "items": templates,
            "hint": "可选内置 13 家,或点「新增」接入全新搜索服务(通用引擎:接口地址/方式/key/参数名/结果字段)",
        }),
        json!({
            "key": "default_limit",
            "label": "默认返回条数",
            "type": "range",
            "min": 1,
            "max": 20,
            "default": 5,
        }),
    ];
    SelfDescribe::new(
        SERVER_NAME,
        "聚合搜索(可扩展供应商)",
        "并行调用全部已配置搜索源,RRF 融合排序+CJK 同题镜像合并去重,多 Key 自动轮换。供应商可扩展:内置 13 家+自定义通用引擎。工具:web_search_lite(免费四源)/web_search_all(全源)。",
        SuggestedEntry::stdio(vec!["--config".into(), "{config_file}".into()])
            .with_tool_timeout_ms(30_000)
            .with_restart_limit(3),
    )
    .with_config_schema(schema)
}

/// 插件协议面实现:2 个只读搜索工具 + 2 个管理面扩展方法。
struct Search {
    ctx: Arc<Ctx>,
}

impl McpService for Search {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new(SERVER_NAME, SERVER_VERSION)
    }

    fn tools(&self) -> Vec<ToolDef> {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "搜索关键词"},
                "limit": {"type": "integer", "description": "返回条数上限(可选,默认取配置 default_limit=5)"},
            },
            "required": ["query"],
        });
        vec![
            ToolDef::read_only(
                "web_search_lite",
                "日常聚合搜索:searxng + ddgs + jina + marginalia(全免费源)并行,RRF 融合排序+同题镜像合并去重,description 带 [来源] 标注。一般搜索优先用这个,快且免费。",
                schema.clone(),
            ),
            ToolDef::read_only(
                "web_search_all",
                "全网搜:并行调用所有已配置搜索源(内置+自定义),RRF 融合排序+镜像合并,meta 带各源耗时遥测。用户要求「全网搜」、需要最大覆盖或交叉验证时使用。",
                schema,
            ),
        ]
    }

    async fn call_tool(&self, name: &str, args: Value) -> ToolOutput {
        ToolOutput::Structured(run_tool(&self.ctx, name, &args).await)
    }

    async fn call_custom(&self, method: &str, params: Value) -> Option<Value> {
        match method {
            // 管理面扩展:单源真搜索测试(返回真实结果)
            "web_search_test" => {
                let out = run_search_test(&self.ctx, &params).await;
                Some(ToolOutput::Structured(out).into_result())
            }
            // 管理面扩展:读月度用量
            "web_usage" => {
                // 先解析全部 provider(内置+自定义),不持配置锁时再取用量,避免死锁
                let ids: Vec<String> = {
                    let mut cfg = self.ctx.cfg.lock().expect("配置锁");
                    resolve_providers(&mut cfg)
                        .into_iter()
                        .map(|p| p.id)
                        .collect()
                };
                let usage = self.ctx.usage.lock().expect("用量锁");
                let mut by_id = json!({});
                if let Some(o) = by_id.as_object_mut() {
                    for id in &ids {
                        o.insert(id.clone(), json!(usage.used(id)));
                    }
                }
                Some(json!({
                    "month": usage.month(),
                    "providers": by_id,
                }))
            }
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() {
    emit_self_describe_if_requested(&self_description());

    let mut config_path: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => {
                config_path = args.next().map(std::path::PathBuf::from);
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

    run_stdio(&Search { ctx }).await;
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
            providers.retain(|p| LITE_IDS.contains(&p.id.as_str()));
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
        .map(|l| l.clamp(1, 20) as usize)
        .unwrap_or(5);

    let provider = {
        let mut cfg = ctx.cfg.lock().expect("配置锁");
        // 管理面单查:停用家也允许真搜测试;已删墓碑返回未知
        resolve_any(&mut cfg, &provider_id)
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
        } else if provider_keys(&provider).is_empty()
            && provider.parse != "ddg"
            && provider.parse != "marginalia"
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> Search {
        Search {
            ctx: Arc::new(Ctx {
                cfg: Mutex::new(Config::new(None)),
                client: reqwest::Client::new(),
                usage: Mutex::new(UsageLedger::from_config_path(None)),
            }),
        }
    }

    #[test]
    fn server_info_reports_declared_name() {
        // ADR-0034:serverInfo.name 归一到自描述声明名(下划线,非连字符)
        let info = svc().server_info();
        assert_eq!(info.name, "web_multisearch");
        assert_eq!(info.version, "0.3.0");
    }

    #[test]
    fn tools_list_two_readonly() {
        let tools = svc().tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "web_search_lite");
        assert_eq!(tools[0].annotations.as_ref().unwrap()["readOnlyHint"], true);
        assert_eq!(tools[1].name, "web_search_all");
        assert_eq!(tools[1].annotations.as_ref().unwrap()["readOnlyHint"], true);
    }

    #[tokio::test]
    async fn call_empty_query_returns_tool_json_error() {
        let out = svc()
            .call_tool("web_search_lite", json!({"query": "   "}))
            .await;
        let ToolOutput::Structured(data) = out else {
            panic!("应返回结构化结果");
        };
        assert_eq!(data["success"], false);
        assert_eq!(data["error"], "query 参数不能为空");
    }

    #[tokio::test]
    async fn web_usage_returns_month_and_providers() {
        let out = svc()
            .call_custom("web_usage", json!({}))
            .await
            .expect("应答");
        assert_eq!(out["month"].as_str().unwrap().len(), 7);
        assert!(out["providers"]["serper"].is_u64());
    }

    #[tokio::test]
    async fn web_search_test_unknown_provider() {
        let out = svc()
            .call_custom(
                "web_search_test",
                json!({"provider_id":"nope","query":"hi"}),
            )
            .await
            .expect("应答");
        let inner = &out["structuredContent"];
        assert_eq!(inner["success"], false);
        assert!(inner["error"].as_str().unwrap().contains("未知供应商"));
    }

    #[tokio::test]
    async fn unknown_custom_method_returns_none() {
        // SDK 据此回 -32601(协议面单测在 boenmind-plugin-sdk)
        assert!(svc()
            .call_custom("resources/list", json!({}))
            .await
            .is_none());
    }
}

#[cfg(test)]
mod self_describe_tests {
    use super::*;

    #[test]
    fn declaration_shape_matches_contract() {
        let d = serde_json::to_value(self_description()).unwrap();
        assert_eq!(d["name"], "web_multisearch");
        assert!(!d["title"].as_str().unwrap().is_empty());
        // config_schema 现为 providers 类型 + default_limit
        let schema = d["config_schema"].as_array().unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema[0]["type"], "providers");
        let items = schema[0]["items"].as_array().unwrap();
        assert_eq!(items.len(), 13, "内置 13 家模板");
        assert_eq!(
            d["suggested_entry"]["args"][0].as_str().unwrap(),
            "--config"
        );
        assert!(d["suggested_entry"]["args"][1]
            .as_str()
            .unwrap()
            .contains("{config_file}"));
    }

    #[test]
    fn declaration_validates_against_contract_schema() {
        // 合同 mcp-self-describe.v0_1:name 必须下划线字符集;这里做最小结构断言
        let v = serde_json::to_value(self_description()).unwrap();
        assert!(v["name"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        assert_eq!(v["suggested_entry"]["transport"], "stdio");
        assert!(!v["description"].as_str().unwrap().is_empty());
    }

    #[test]
    fn provider_template_has_all_engine_fields() {
        let d = serde_json::to_value(self_description()).unwrap();
        let items = d["config_schema"][0]["items"].as_array().unwrap();
        let first = &items[0];
        for k in [
            "id",
            "name",
            "builtin",
            "endpoint",
            "method",
            "auth",
            "auth_name",
            "query_param",
            "limit_param",
            "results_path",
            "title_field",
            "url_field",
            "desc_field",
            "parse",
            "quota",
        ] {
            assert!(first.get(k).is_some(), "模板缺字段 {k}: {first}");
        }
    }
}
