mod config;
mod executor;
mod indexer;
mod store;

use std::sync::{Arc, Mutex};

use config::Config;
use executor::{execute_batch, execute_code, execute_file};
use indexer::{index_path, search};
use serde_json::{json, Value};
use store::Store;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "context_mode";
const SERVER_VERSION: &str = "0.1.0";

struct Ctx {
    config: Mutex<Config>,
    store: Store,
}

fn self_description() -> Value {
    json!({
        "name": SERVER_NAME,
        "title": "上下文模式（Rust）",
        "description": "本地 FTS5/BM25 上下文检索、会话快照恢复与受限执行；不依赖 Node/Python 运行插件本体。",
        "config_schema": [
            {"key":"data_dir","label":"上下文数据目录","type":"string","default":"","hint":"留空则使用 BoenMind 数据目录下的 context-mode"},
            {"key":"allowed_roots","label":"允许根目录","type":"string","default":"","hint":"逗号分隔；索引和执行 cwd 只能位于这些目录内"},
            {"key":"max_file_bytes","label":"单文件上限","type":"range","min":1024,"max":16777216,"default":1048576,"unit":"bytes"},
            {"key":"max_files","label":"单次索引文件数","type":"range","min":1,"max":5000,"default":5000},
            {"key":"max_output_bytes","label":"执行输出上限","type":"range","min":1024,"max":2097152,"default":262144,"unit":"bytes"},
            {"key":"default_timeout_ms","label":"默认执行超时","type":"range","min":100,"max":600000,"default":30000,"unit":"ms"},
            {"key":"execution_enabled","label":"启用宿主执行","type":"select","default":"false","options":[{"value":"false","label":"关闭（推荐）"},{"value":"true","label":"开启（仍需 Broker 审批）"}],"hint":"插件不是操作系统级沙箱；开启前请确认 allowed_roots 和宿主程序"}
        ],
        "suggested_entry": {
            "transport":"stdio",
            "args":["--config","{config_file}"],
            "tool_timeout_ms":30000,
            "restart_limit":3
        }
    })
}

#[tokio::main]
async fn main() {
    let mut config_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--self-describe" => {
                println!(
                    "{}",
                    serde_json::to_string(&self_description()).expect("serialize description")
                );
                return;
            }
            "--config" => config_path = args.next().map(std::path::PathBuf::from),
            other => eprintln!(
                "[{SERVER_NAME}] 未知参数:{other}（支持 --config <json> / --self-describe）"
            ),
        }
    }

    let config = match Config::load(config_path.as_deref()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("[{SERVER_NAME}] 配置加载失败:{error}");
            return;
        }
    };
    if let Err(error) = std::fs::create_dir_all(&config.data_dir) {
        eprintln!("[{SERVER_NAME}] 创建数据目录失败:{error}");
        return;
    }
    let db_path = config.data_dir.join("context-mode.sqlite3");
    let store = match Store::open(&db_path) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("[{SERVER_NAME}] SQLite 初始化失败:{error}");
            return;
        }
    };
    eprintln!(
        "[{SERVER_NAME}] v{SERVER_VERSION} 启动;data_dir={}",
        config.data_dir.display()
    );
    let ctx = Arc::new(Ctx {
        config: Mutex::new(config),
        store,
    });

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("[{SERVER_NAME}] JSON 解析失败:{error}");
                continue;
            }
        };
        if let Some(response) = handle_request(&message, &ctx).await {
            let mut encoded = serde_json::to_string(&response).expect("serialize response");
            encoded.push('\n');
            if stdout.write_all(encoded.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    }
}

async fn handle_request(message: &Value, ctx: &Ctx) -> Option<Value> {
    let id = match message.get("id") {
        Some(Value::Null) | None => return None,
        Some(value) => value.clone(),
    };
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION}
        })),
        "ping" => Ok(json!({})),
        "notifications/initialized" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_defs()})),
        "tools/call" => call_tool(ctx, message).await,
        _ => Err((-32601, format!("方法不存在:{method}"))),
    };
    Some(match result {
        Ok(value) => json!({"jsonrpc":"2.0", "id":id, "result":value}),
        Err((code, message)) => {
            json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
        }
    })
}

async fn call_tool(ctx: &Ctx, message: &Value) -> Result<Value, (i64, String)> {
    let name = message
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments = message
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result = match name {
        "ctx_index" => index_tool(ctx, &arguments),
        "ctx_search" => search_tool(ctx, &arguments),
        "ctx_session_append" => session_append_tool(ctx, &arguments),
        "ctx_session_snapshot" => session_snapshot_tool(ctx, &arguments),
        "ctx_session_restore" => session_restore_tool(ctx, &arguments),
        "ctx_execute" => execute_code_tool(ctx, &arguments).await,
        "ctx_execute_file" => execute_file_tool(ctx, &arguments).await,
        "ctx_batch_execute" => execute_batch_tool(ctx, &arguments).await,
        _ => return Err((-32602, format!("未知工具:{name}"))),
    };
    Ok(tool_result(result))
}

fn tool_result(result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => {
            json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).expect("serialize tool result")}],"structuredContent":value,"isError":false})
        }
        Err(error) => {
            let value = json!({"success":false,"error":error});
            json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).expect("serialize tool error")}],"structuredContent":value,"isError":true})
        }
    }
}

fn config(ctx: &Ctx) -> Config {
    ctx.config.lock().expect("config lock").clone()
}

fn index_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let cfg = config(ctx);
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    index_path(&ctx.store, &cfg, path.as_deref())
}

fn search_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if query.is_empty() {
        return Err("query 参数不能为空".into());
    }
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 50) as usize;
    search(&ctx.store, query, limit)
}

fn session_append_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let session = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let role = args
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    if session.is_empty() || role.is_empty() || content.is_empty() {
        return Err("session_id、role、content 不能为空".into());
    }
    let seq = ctx.store.append_event(session, role, content)?;
    Ok(json!({"success":true,"session_id":session,"seq":seq}))
}

fn session_snapshot_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let session = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if session.is_empty() {
        return Err("session_id 不能为空".into());
    }
    let max = args
        .get("max_messages")
        .and_then(Value::as_u64)
        .unwrap_or(40)
        .clamp(1, 200) as usize;
    let snapshot = ctx.store.snapshot(session, max)?;
    Ok(json!({"success":true,"session_id":session,"snapshot":snapshot}))
}

fn session_restore_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let session = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if session.is_empty() {
        return Err("session_id 不能为空".into());
    }
    Ok(json!({"success":true,"session_id":session,"messages":ctx.store.restore(session)?}))
}

async fn execute_code_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let cfg = config(ctx);
    if !cfg.execution_enabled {
        return Err("execution_disabled:请在 context-mode 配置中显式设置 execution_enabled=true，并通过 Broker 审批".into());
    }
    execute_code(&cfg, args).await
}
async fn execute_file_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let cfg = config(ctx);
    if !cfg.execution_enabled {
        return Err("execution_disabled:请在 context-mode 配置中显式设置 execution_enabled=true，并通过 Broker 审批".into());
    }
    execute_file(&cfg, args).await
}
async fn execute_batch_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    let cfg = config(ctx);
    if !cfg.execution_enabled {
        return Err("execution_disabled:请在 context-mode 配置中显式设置 execution_enabled=true，并通过 Broker 审批".into());
    }
    execute_batch(&cfg, args).await
}

fn tool_defs() -> Vec<Value> {
    let object = |properties: Value, required: Value| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    vec![
        json!({"name":"ctx_index","description":"索引允许根目录内的 UTF-8 文本文件。","inputSchema":object(json!({"path":{"type":"string","description":"允许根目录内的文件或目录；省略则索引全部允许根目录"}}),json!([])),"annotations":{"destructiveHint":true}}),
        json!({"name":"ctx_search","description":"使用 SQLite FTS5/BM25 搜索已索引上下文。","inputSchema":object(json!({"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":50}}),json!(["query"])),"annotations":{"readOnlyHint":true}}),
        json!({"name":"ctx_session_append","description":"追加一条独立会话事件。","inputSchema":object(json!({"session_id":{"type":"string"},"role":{"type":"string"},"content":{"type":"string"}}),json!(["session_id","role","content"])),"annotations":{"destructiveHint":true}}),
        json!({"name":"ctx_session_snapshot","description":"为会话保存最近消息快照。","inputSchema":object(json!({"session_id":{"type":"string"},"max_messages":{"type":"integer","minimum":1,"maximum":200}}),json!(["session_id"])),"annotations":{"destructiveHint":true}}),
        json!({"name":"ctx_session_restore","description":"恢复会话消息，优先返回快照并补齐后续事件。","inputSchema":object(json!({"session_id":{"type":"string"}}),json!(["session_id"])),"annotations":{"readOnlyHint":true}}),
        json!({"name":"ctx_execute","description":"在允许根目录内执行受限宿主程序；执行类工具必须经过 Broker 审批。","inputSchema":object(json!({"language":{"type":"string","enum":["shell","python","node"]},"code":{"type":"string"},"argv":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"},"timeout_ms":{"type":"integer","minimum":100,"maximum":600000},"max_output_bytes":{"type":"integer","minimum":1024,"maximum":2097152}}),json!(["language","code"])),"annotations":{"destructiveHint":true}}),
        json!({"name":"ctx_execute_file","description":"执行允许根目录内的 shell/Python/Node 文件。","inputSchema":object(json!({"path":{"type":"string"},"language":{"type":"string","enum":["shell","python","node"]},"argv":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"}}),json!(["path"])),"annotations":{"destructiveHint":true}}),
        json!({"name":"ctx_batch_execute","description":"顺序执行最多 16 个受限执行项，逐项返回状态。","inputSchema":object(json!({"items":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"object"}}}),json!(["items"])),"annotations":{"destructiveHint":true}}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_ctx() -> (TempDir, Ctx) {
        let dir = TempDir::new().expect("temp");
        let cfg = Config::for_test(dir.path().to_path_buf());
        let store = Store::open(&cfg.data_dir.join("test.sqlite3")).expect("db");
        (
            dir,
            Ctx {
                config: Mutex::new(cfg),
                store,
            },
        )
    }

    #[tokio::test]
    async fn protocol_shapes() {
        let (_dir, ctx) = test_ctx();
        let init = handle_request(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}), &ctx)
            .await
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(init["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(handle_request(
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            &ctx
        )
        .await
        .is_none());
        let list = handle_request(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), &ctx)
            .await
            .unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 8);
    }

    #[tokio::test]
    async fn session_round_trip_and_search() {
        let (_dir, ctx) = test_ctx();
        let append = json!({"session_id":"s1","role":"user","content":"rust context searchable"});
        let response = handle_request(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ctx_session_append","arguments":append}}), &ctx).await.unwrap();
        assert_eq!(response["result"]["isError"], false);
        let restored = session_restore_tool(&ctx, &json!({"session_id":"s1"})).unwrap();
        assert_eq!(
            restored["messages"][0]["content"],
            "rust context searchable"
        );
    }

    #[test]
    fn self_description_shape() {
        let d = self_description();
        assert_eq!(d["name"], SERVER_NAME);
        assert_eq!(d["suggested_entry"]["args"][1], "{config_file}");
    }
}
