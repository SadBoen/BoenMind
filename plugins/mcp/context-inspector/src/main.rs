mod config;
mod inspector;

use config::Config;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "context_inspector";
const SERVER_VERSION: &str = "0.1.0";

/// 官方自描述元数据规范 (--self-describe 输出)
fn self_describe_payload() -> Value {
    json!({
        "name": SERVER_NAME,
        "title": "大模型交互透视与诊断插件",
        "description": "BoenMind 官方只读透视分析插件：解剖 Prompt 配方构成，提供模型窗口真实水位、推理思考分账、文件副作用追踪与 Token 暴增诊断。",
        "config_schema": [
            {
                "key": "data_dir",
                "label": "数据目录路径",
                "type": "string",
                "hint": "可选。默认自动挂接当前 BoenMind 运行环境的数据目录"
            }
        ],
        "suggested_entry": {
            "transport": "stdio",
            "args": [
                "--data-dir",
                "{data_dir}"
            ],
            "tool_timeout_ms": 15000,
            "restart_limit": 3
        }
    })
}

/// 声明支持的 4 个只读直通工具
fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "context_inspect_snapshot",
                "description": "深度拆解大模型调用的 Prompt 配方（人设/技能/目录/工具箱/历史轮次）与真实 Token 水位",
                "annotations": {
                    "readOnlyHint": true
                },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "可选。限定特定会话 ID；未提供则取最新快照"
                        },
                        "seq": {
                            "type": "integer",
                            "description": "可选。指定要分析的具体快照序号 seq"
                        }
                    }
                }
            },
            {
                "name": "context_diagnose_spikes",
                "description": "多轮历史 Token 暴增与刺客诊断：智能比对相邻轮次增量，揪出引起上下文激增的轮次",
                "annotations": {
                    "readOnlyHint": true
                },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "会话 ID"
                        },
                        "threshold_diff": {
                            "type": "integer",
                            "description": "触发绝对激增警报的 Token 差值（默认 2500）"
                        },
                        "threshold_ratio": {
                            "type": "number",
                            "description": "触发倍率激增警报的倍数（默认 2.0）"
                        }
                    },
                    "required": ["session_id"]
                }
            },
            {
                "name": "context_track_file_effects",
                "description": "本地工程文件副作用追踪：提取当前会话触发的全部 fs.* 与 system.exec 文件读写操作",
                "annotations": {
                    "readOnlyHint": true
                },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "会话 ID"
                        }
                    },
                    "required": ["session_id"]
                }
            },
            {
                "name": "context_search_history",
                "description": "跨会话搜索历史上下文快照与交互记录",
                "annotations": {
                    "readOnlyHint": true
                },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "搜索关键词"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "返回匹配条数上限（默认 20，至多 100）"
                        }
                    },
                    "required": ["query"]
                }
            }
        ]
    })
}

/// 处理 JSON-RPC 方法分发
fn handle_rpc(cfg: &Config, req: &Value) -> Option<Value> {
    let id = req.get("id");
    // 通知类消息无需回复 (如 notifications/initialized)
    if id.is_none() || id.unwrap().is_null() {
        return None;
    }
    let id = id.unwrap().clone();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            }
        })),
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {}
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": tools_list()
        })),
        "tools/call" => {
            let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));

            let result_data = match tool_name {
                "context_inspect_snapshot" => {
                    let sid = args.get("session_id").and_then(Value::as_str);
                    let seq = args.get("seq").and_then(Value::as_u64);
                    inspector::inspect_snapshot(cfg, sid, seq)
                }
                "context_diagnose_spikes" => {
                    let sid = args.get("session_id").and_then(Value::as_str).unwrap_or("");
                    let diff = args.get("threshold_diff").and_then(Value::as_u64);
                    let ratio = args.get("threshold_ratio").and_then(Value::as_f64);
                    inspector::diagnose_spikes(cfg, sid, diff, ratio)
                }
                "context_track_file_effects" => {
                    let sid = args.get("session_id").and_then(Value::as_str).unwrap_or("");
                    inspector::track_file_effects(cfg, sid)
                }
                "context_search_history" => {
                    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
                    let limit = args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize);
                    inspector::search_history(cfg, query, limit)
                }
                _ => {
                    return Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32602,
                            "message": format!("未知工具: {tool_name}")
                        }
                    }));
                }
            };

            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": serde_json::to_string_pretty(&result_data).unwrap_or_default()
                        }
                    ],
                    "structuredContent": result_data,
                    "isError": false
                }
            }))
        }
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("方法不存在: {method}")
            }
        })),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--self-describe") {
        println!(
            "{}",
            serde_json::to_string(&self_describe_payload()).unwrap()
        );
        std::process::exit(0);
    }

    let cfg = Config::from_args();
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(req) = serde_json::from_str::<Value>(trimmed) {
            if let Some(resp) = handle_rpc(&cfg, &req) {
                let serialized = serde_json::to_string(&resp).unwrap();
                let _ = stdout.write_all(serialized.as_bytes()).await;
                let _ = stdout.write_all(b"\n").await;
                let _ = stdout.flush().await;
            }
        }
    }
}
