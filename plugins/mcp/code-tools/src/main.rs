//! code-tools MCP server——编码任务一等工具面:查(search)/读(read)/
//! 改(write/edit)。rg 引擎(grep-searcher/grep-regex)以库内嵌,不依赖
//! 宿主机装 grep/rg;allowed_roots 白名单沙箱(ADR-0006 权限显式化)。
//!
//! 权限分级(annotations → 宿主 M7 S3 映射):
//! search/read = readOnlyHint → read-only 直通;write/edit = destructiveHint
//! → external-side-effect + required(审批卡)。
//!
//! 协议:MCP 2024-11-05,JSON-RPC over stdio(逐行),手写零 SDK。
//! 配置:`--config <json>`(BoenMind 传 config/mcp-code_tools.json);
//! 根目录表启动时定,改根需「重载 MCP」。工具内部错误一律 isError:false +
//! JSON ok/error(错误详情须回喂给模型,ToolError 通道不带输出)。

mod config;
mod fsops;
mod guard;
mod search;

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use config::Config;
use guard::Roots;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "code-tools";
const SERVER_VERSION: &str = "0.1.0";

struct Ctx {
    cfg: Config,
    roots: Roots,
}

/// 自描述声明:插件目录扫描的识别载体。`args` 模板中的 `{config_file}` 由
/// 批准方(BoenMind webadmin)替换为实际数据目录配置路径。
fn self_description() -> Value {
    let schema = json!([
        {"key": "allowed_roots", "label": "允许访问的根目录", "type": "string", "default": "",
         "hint": "绝对路径,分号分隔多个;全部工具只许碰这些目录内;改后「重载 MCP」"},
        {"key": "max_results", "label": "搜索命中上限", "type": "range", "min": 1, "max": 500, "default": 80},
        {"key": "max_output_chars", "label": "单次输出字符上限", "type": "range", "min": 1000, "max": 65536, "default": 16000},
        {"key": "max_file_bytes", "label": "搜索跳过的超大文件阈值(字节)", "type": "range", "min": 256, "max": 33554432, "default": 1048576},
    ]);
    json!({
        "name": "code_tools",
        "title": "代码工具(查/读/改)",
        "description": "编码任务工具面:search=内容搜索(rg 引擎内嵌,免装 grep);read=带行号读文件分页;write=写文件;edit=精确字符串替换(不走 sed)。查/读免审批直通,写/改走审批卡,allowed_roots 沙箱防逃逸。",
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
            // 自描述(两段式接入的"扫描发现"基础):紧凑单行 JSON 后退出
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
    let cfg = Config::load(config_path.as_deref());
    let roots = Roots::new(&cfg.allowed_roots);
    eprintln!(
        "[{SERVER_NAME}] v{SERVER_VERSION} 启动;roots={}",
        roots
            .roots()
            .iter()
            .map(|p| guard::display_path(p))
            .collect::<Vec<_>>()
            .join(";")
    );

    let ctx = Arc::new(Ctx { cfg, roots });

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
        if let Some(resp) = handle_request(&msg, &ctx) {
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
fn handle_request(msg: &Value, ctx: &Ctx) -> Option<Value> {
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
            match name.as_str() {
                "search" | "read" | "write" | "edit" => {
                    let out = run_tool(ctx, &name, &arguments);
                    Ok(json!({
                        "content": [{"type": "text", "text": serde_json::to_string_pretty(&out).expect("结果序列化")}],
                        "structuredContent": out,
                        "isError": false,
                    }))
                }
                other => Err((-32602, format!("未知工具:{other}"))),
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

/// 工具分发(供协议层与单测共用)。错误一律 ok:false JSON,不抛协议错误。
fn run_tool(ctx: &Ctx, name: &str, arguments: &Value) -> Value {
    match name {
        "search" => search::search(&ctx.cfg, &ctx.roots, arguments),
        "read" => fsops::read(&ctx.cfg, &ctx.roots, arguments),
        "write" => fsops::write(&ctx.cfg, &ctx.roots, arguments),
        "edit" => fsops::edit(&ctx.cfg, &ctx.roots, arguments),
        other => json!({"ok": false, "error": format!("未知工具:{other}")}),
    }
}

fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "search",
            "description": "在工作区根目录内做内容搜索(ripgrep 同款引擎已内嵌,无需系统装 grep)。支持正则;返回 文件+行号+命中行。查代码、找定义/用法、定位字符串一律先用这个。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索内容(默认按正则解释;纯文本加 fixed=true)"},
                    "fixed": {"type": "boolean", "description": "true=按字面文本搜索(不做正则解释),默认 false"},
                    "case_sensitive": {"type": "boolean", "description": "大小写敏感,默认 false(忽略大小写)"},
                    "max_results": {"type": "integer", "description": "命中上限(可选,默认 80,封顶 500)"}
                },
                "required": ["query"]
            },
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "read",
            "description": "读文本文件,带行号;大文件用 offset(起始行,1 起)/limit(行数)分页。改文件前先读原文。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "文件路径(工作区内绝对路径,或相对第一根目录的相对路径)"},
                    "offset": {"type": "integer", "description": "起始行号(1 起,默认 1)"},
                    "limit": {"type": "integer", "description": "最多读多少行(默认 2000)"}
                },
                "required": ["path"]
            },
            "annotations": {"readOnlyHint": true},
        }),
        json!({
            "name": "write",
            "description": "写文件(新建或整文覆盖),自动创建父目录。整文覆盖会丢弃原内容,慎用;改已有文件优先用 edit。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "目标文件路径(工作区内)"},
                    "content": {"type": "string", "description": "完整文件内容(UTF-8 文本)"}
                },
                "required": ["path", "content"]
            },
            "annotations": {"destructiveHint": true},
        }),
        json!({
            "name": "edit",
            "description": "精确字符串替换编辑:old_string 必须与文件原文逐字一致(含缩进)且唯一命中;多处命中会被拒绝(补上下文或传 replace_all=true);CRLF 文件自动兼容。改代码首选。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "目标文件路径(工作区内)"},
                    "old_string": {"type": "string", "description": "要被替换的精确原文"},
                    "new_string": {"type": "string", "description": "替换后的内容(删内容传空串)"},
                    "replace_all": {"type": "boolean", "description": "多处命中时全部替换,默认 false"}
                },
                "required": ["path", "old_string", "new_string"]
            },
            "annotations": {"destructiveHint": true},
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_root(dir: &std::path::Path) -> Ctx {
        let mut cfg = Config::load(None);
        cfg.allowed_roots = vec![dir.display().to_string()];
        let roots = Roots::new(&cfg.allowed_roots);
        Ctx { cfg, roots }
    }

    #[test]
    fn initialize_shape() {
        let dir = tempfile::tempdir().expect("tmp");
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            &ctx_with_root(dir.path()),
        )
        .expect("应答");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "code-tools");
    }

    #[test]
    fn notification_gets_no_reply() {
        let dir = tempfile::tempdir().expect("tmp");
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            &ctx_with_root(dir.path()),
        );
        assert!(resp.is_none());
    }

    #[test]
    fn tools_list_four_with_right_approval_annotations() {
        let dir = tempfile::tempdir().expect("tmp");
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            &ctx_with_root(dir.path()),
        )
        .expect("应答");
        let tools = resp["result"]["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0]["name"], "search");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true, "search 直通");
        assert_eq!(tools[1]["name"], "read");
        assert_eq!(tools[1]["annotations"]["readOnlyHint"], true, "read 直通");
        assert_eq!(tools[2]["name"], "write");
        assert_eq!(
            tools[2]["annotations"]["destructiveHint"], true,
            "write 审批"
        );
        assert_eq!(tools[3]["name"], "edit");
        assert_eq!(
            tools[3]["annotations"]["destructiveHint"], true,
            "edit 审批"
        );
    }

    #[test]
    fn write_read_edit_roundtrip_via_protocol() {
        let dir = tempfile::tempdir().expect("tmp");
        let ctx = ctx_with_root(dir.path());
        let call = |id: i64, name: &str, args: Value| {
            handle_request(
                &json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                        "params":{"name":name,"arguments":args}}),
                &ctx,
            )
            .expect("应答")["result"]["structuredContent"]
                .clone()
        };
        let w = call(
            3,
            "write",
            json!({"path": "a.txt", "content": "hello world\n"}),
        );
        assert_eq!(w["ok"], true);
        let r = call(4, "read", json!({"path": "a.txt"}));
        assert!(r["content"].as_str().expect("b").contains("hello world"));
        let e = call(
            5,
            "edit",
            json!({"path": "a.txt", "old_string": "world", "new_string": "code-tools"}),
        );
        assert_eq!(e["replacements"], 1);
        let s = call(6, "search", json!({"query": "code-tools", "fixed": true}));
        assert_eq!(s["total_matches"], 1);
    }

    #[test]
    fn unknown_tool_is_invalid_params() {
        let dir = tempfile::tempdir().expect("tmp");
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                    "params":{"name":"nope","arguments":{}}}),
            &ctx_with_root(dir.path()),
        )
        .expect("应答");
        assert_eq!(resp["error"]["code"], -32602);
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let dir = tempfile::tempdir().expect("tmp");
        let resp = handle_request(
            &json!({"jsonrpc":"2.0","id":5,"method":"resources/list"}),
            &ctx_with_root(dir.path()),
        )
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
        assert_eq!(d["name"], "code_tools");
        assert!(!d["title"].as_str().unwrap().is_empty());
        assert_eq!(d["config_schema"].as_array().unwrap().len(), 4);
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
    fn declaration_is_single_line_compact() {
        let out = serde_json::to_string(&self_description()).expect("序列化");
        assert!(!out.contains('\n'), "扫描方按行解析,声明必须单行");
    }
}
