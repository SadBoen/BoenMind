mod config;
mod inspector;

use boenmind_plugin_sdk::{
    emit_self_describe_if_requested, run_stdio, McpService, SelfDescribe, ServerInfo,
    SuggestedEntry, ToolDef, ToolOutput,
};
use config::Config;
use serde_json::{json, Value};

const SERVER_NAME: &str = "context_inspector";
const SERVER_VERSION: &str = "0.1.0";

/// 插件协议面实现:4 个只读直通工具(工具表与调用处理)。
struct Inspector {
    cfg: Config,
}

impl McpService for Inspector {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new(SERVER_NAME, SERVER_VERSION)
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::read_only(
                "inspect_snapshot",
                "深度拆解大模型调用的 Prompt 配方（人设/技能/目录/工具箱/历史轮次）与真实 Token 水位",
                json!({
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string", "description": "可选。限定特定会话 ID；未提供则取最新快照"},
                        "seq": {"type": "integer", "description": "可选。指定要分析的具体快照序号 seq"}
                    }
                }),
            ),
            ToolDef::read_only(
                "diagnose_spikes",
                "多轮历史 Token 暴增与刺客诊断：智能比对相邻轮次增量，揪出引起上下文激增的轮次",
                json!({
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string", "description": "会话 ID"},
                        "threshold_diff": {"type": "integer", "description": "触发绝对激增警报的 Token 差值（默认 2500）"},
                        "threshold_ratio": {"type": "number", "description": "触发倍率激增警报的倍数（默认 2.0）"}
                    },
                    "required": ["session_id"]
                }),
            ),
            ToolDef::read_only(
                "track_file_effects",
                "本地工程文件副作用追踪：提取当前会话触发的全部 fs.* 与 system.exec 文件读写操作",
                json!({
                    "type": "object",
                    "properties": {"session_id": {"type": "string", "description": "会话 ID"}},
                    "required": ["session_id"]
                }),
            ),
            ToolDef::read_only(
                "search_history",
                "跨会话搜索历史上下文快照与交互记录",
                json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "搜索关键词"},
                        "limit": {"type": "integer", "description": "返回匹配条数上限（默认 20，至多 100）"}
                    },
                    "required": ["query"]
                }),
            ),
        ]
    }

    async fn call_tool(&self, name: &str, args: Value) -> ToolOutput {
        let cfg = &self.cfg;
        let data = match name {
            "inspect_snapshot" => {
                let sid = args.get("session_id").and_then(Value::as_str);
                let seq = args.get("seq").and_then(Value::as_u64);
                inspector::inspect_snapshot(cfg, sid, seq)
            }
            "diagnose_spikes" => {
                let sid = args.get("session_id").and_then(Value::as_str).unwrap_or("");
                let diff = args.get("threshold_diff").and_then(Value::as_u64);
                let ratio = args.get("threshold_ratio").and_then(Value::as_f64);
                inspector::diagnose_spikes(cfg, sid, diff, ratio)
            }
            "track_file_effects" => {
                let sid = args.get("session_id").and_then(Value::as_str).unwrap_or("");
                inspector::track_file_effects(cfg, sid)
            }
            "search_history" => {
                let query = args.get("query").and_then(Value::as_str).unwrap_or("");
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize);
                inspector::search_history(cfg, query, limit)
            }
            _ => unreachable!("未知工具已由 SDK 拦截"),
        };
        ToolOutput::Structured(data)
    }

    async fn call_custom(&self, _method: &str, _params: Value) -> Option<Value> {
        None
    }
}

/// 官方自描述元数据(协议合同 `mcp-self-describe.v0_1`)。
fn self_describe() -> SelfDescribe {
    SelfDescribe::new(
        SERVER_NAME,
        "大模型交互透视与诊断插件",
        "BoenMind 官方只读透视分析插件：解剖 Prompt 配方构成，提供模型窗口真实水位、推理思考分账、文件副作用追踪与 Token 暴增诊断。",
        SuggestedEntry::stdio(vec!["--data-dir".into(), "{data_dir}".into()])
            .with_tool_timeout_ms(15_000)
            .with_restart_limit(3),
    )
    .with_config_schema(vec![json!({
        "key": "data_dir",
        "label": "数据目录路径",
        "type": "string",
        "hint": "可选。默认自动挂接当前 BoenMind 运行环境的数据目录"
    })])
}

#[tokio::main]
async fn main() {
    emit_self_describe_if_requested(&self_describe());
    let svc = Inspector {
        cfg: Config::from_args(),
    };
    run_stdio(&svc).await;
}
