//! 子代理（subagent）子进程入口。
//!
//! 上游 `subagent` 工具（pi_agent_rust@44ddf80/src/subagents.rs）会 spawn **当前
//! 可执行文件**（独立部署下即 bm-server 二进制）并传固定参数形状：
//! `--mode json --print --no-session --tools <csv> [--model M] [--thinking T]
//! [--skill S] [--append-system-prompt P] Task: <task>`，然后按 stdout 逐行
//! JSON 事件流收取结果（协议见 `subagents.rs::ingest_child_event`）。
//!
//! 本模块实现该入口：识别调用 → 解析参数 → 解析 provider（父进程经环境变量
//! 注入，或按模型匹配）→ **用自研 bm-loop 跑一轮隔离回合**（pi 废除第②步：
//! 不再依赖 pi SDK；InMemory 事件日志 + BuiltinTools 执行器 + OpenAiClient
//! 直连）→ 以协议事件流回传（事件形状与 pi AgentEvent serde 逐字段对齐，
//! 父侧 `ingest_child_event` 零改动）。日志一律走 stderr，stdout 只承载协议事件。

use std::io::Write;
use std::sync::Arc;

use bm_loop::engine::{LoopConfig, ReactLoopAgent, TurnRequest};
use bm_loop::llm::OpenAiClient;
use bm_loop::model::ToolRegistry;
use bm_loop::points::{LoopHooks, StepCtx};
use bm_protocol::{BranchId, HeaderReason, SessionId, UserMsgSource};
use bm_kernel::{EventLog, InMemoryEventStore};

/// 子代理子进程的解析后参数（对齐 `subagents.rs::child_args` 的形状）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildArgs {
    /// 角色定义白名单工具（CSV）
    pub tools: Vec<String>,
    /// 角色定义指定模型（可空，回落全局默认）
    pub model: Option<String>,
    /// 思考强度（--thinking 值，如 "off"/"low"）
    pub thinking: Option<String>,
    /// 角色定义正文（system prompt，追加到 pi 默认提示之后）
    pub append_system_prompt: Option<String>,
    /// 任务描述（位置参数 "Task: <task>"）
    pub task: String,
}

/// 判别 argv 是否为子代理子进程调用：上游 spawn 的形状是
/// `--mode json --print --no-session ...`（正常 HTTP 服务启动不会带这些参数）。
pub fn should_enter_child_mode(args: &[String]) -> bool {
    let has_json = args
        .iter()
        .position(|a| a == "--mode")
        .is_some_and(|i| args.get(i + 1).is_some_and(|v| v == "json"));
    let has_no_session = args.iter().any(|a| a == "--no-session");
    let has_parent = std::env::var("PI_SUBAGENT_PARENT_PID").is_ok_and(|v| !v.is_empty());
    (has_json || has_parent) && (has_no_session || has_parent)
}

/// 解析子代理参数（纯函数，便于对齐协议测试）。
pub fn parse_child_args(args: &[String]) -> Result<ChildArgs, String> {
    let mut tools = Vec::new();
    let mut model = None;
    let mut thinking = None;
    let mut append = None;
    let mut positionals = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            // 带值旗标：消费下一个 token
            "--tools" | "--model" | "--thinking" | "--skill" | "--append-system-prompt" => {
                let key = args[i].clone();
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| format!("{key} 缺少参数值"))?
                    .clone();
                match key.as_str() {
                    "--tools" => {
                        tools = value
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect();
                    }
                    "--model" => model = Some(value),
                    "--thinking" => thinking = Some(value),
                    // --skill 第一版忽略：技能池化后按角色分配（见 docs/expert-team.md 阶段 2）
                    "--append-system-prompt" => append = Some(value),
                    _ => {}
                }
            }
            // 无值旗标；--mode 的值 "json" 也必须消费，否则会被当作位置参数
            "--mode" => {
                i += 1;
                let _ = args.get(i);
            }
            "--print" | "--no-session" | "--help" | "--version" => {}
            flag if flag.starts_with('-') => {
                // 未知旗标：忽略自身（不消费值，保持位置参数识别宽松）
            }
            other => positionals.push(other.to_string()),
        }
        i += 1;
    }
    let task = positionals.join(" ").trim().to_string();
    if task.is_empty() {
        return Err("缺少任务描述（位置参数 Task: ...）".to_string());
    }
    Ok(ChildArgs {
        tools,
        model,
        thinking,
        append_system_prompt: append,
        task,
    })
}

/// 运行子代理子进程；返回进程退出码（0 成功 / 1 agent 失败 / 2 环境错误）。
///
/// pi 废除第②步：子进程不再加载 pi SDK，改由 bm-loop + InMemory 事件日志 +
/// 内置工具集 + OpenAiClient 直连跑一轮隔离回合；stdout 协议事件形状与 pi 逐字段对齐。
pub async fn run(args: &[String]) -> i32 {
    let child = match parse_child_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[bm-server:subagent] 参数解析失败: {e}");
            return 2;
        }
    };

    // 轻量初始化：只读配置（bm 引擎不需要 pi agent 目录）
    let config = bm_core::config::load();

    // provider 解析优先级：
    // 1. 父进程注入的 PI_SUBAGENT_PROVIDER_ID（bm-server 启动时按默认提供商设置）
    // 2. 角色定义 model 匹配已配置提供商的 models 列表
    // 3. 配置全局默认 / 第一个提供商
    let provider = std::env::var("PI_SUBAGENT_PROVIDER_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .and_then(|id| bm_core::config::resolve_provider(&config, Some(&id)))
        .or_else(|| {
            child.model.as_deref().and_then(|m| {
                config
                    .providers
                    .iter()
                    .find(|p| p.models.iter().any(|pm| pm == m))
            })
        })
        .or_else(|| bm_core::config::resolve_provider(&config, None));
    let Some(provider) = provider else {
        eprintln!("[bm-server:subagent] 未找到可用提供商（config.toml 无 providers）");
        return 2;
    };
    let Some(model) = child
        .model
        .clone()
        .or_else(|| bm_core::config::resolve_model(provider, None))
    else {
        eprintln!("[bm-server:subagent] 无法确定模型（角色未指定 model 且提供商无默认模型）");
        return 2;
    };

    // 工具面：角色 csv ∩ 内置工具集（pi 特有工具如 hashline_edit 无 bm
    // 对应实现，忽略——自研底座以 read/write/edit 覆盖同场景）
    let mut tools = ToolRegistry::new();
    let csv_names = if child.tools.is_empty() {
        None
    } else {
        Some(child.tools.iter().map(String::as_str).collect::<Vec<_>>())
    };
    for def in crate::builtin_tools::BuiltinTools::definitions() {
        let wanted = csv_names
            .as_ref()
            .is_none_or(|names| names.iter().any(|n| *n == def.name));
        if wanted
            && let Err(err) = tools.register(def.clone())
        {
            eprintln!("[bm-server:subagent] 工具注册失败 {}: {}", def.name, err.message);
        }
    }

    // 系统提示：基础 SYSTEM_PROMPT + 角色正文（append-system-prompt）
    let mut system_prompt = bm_core::agent::SYSTEM_PROMPT.to_string();
    if let Some(append) = &child.append_system_prompt
        && !append.trim().is_empty()
    {
        system_prompt.push_str(append);
    }

    // 15min 超时兜底（与 chat 路径同纪律；父进程 kill 即传播取消，此兜底
    // 防模型挂死时子进程孤儿化）
    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let timeout_tx = cancel_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(crate::chat::PROMPT_TIMEOUT).await;
        let _ = timeout_tx.send(true);
    });

    let llm = match crate::bm_engine::resolve_llm_config(provider, &model, child.thinking.as_deref()) {
        Ok(cfg) => OpenAiClient::new(cfg),
        Err((_status, msg)) => {
            eprintln!("[bm-server:subagent] LLM 配置失败: {msg}");
            emit_agent_end_error(format!("Failed to configure LLM: {msg}"));
            return 1;
        }
    };
    let mut agent = ReactLoopAgent::new(
        SubagentHooks::default(),
        tools,
        EventLog::new(Arc::new(InMemoryEventStore::new())),
        SessionId::new("subagent"),
        BranchId::new("main"),
        LoopConfig {
            system_prompt,
            provider: Some(provider.id.clone()),
            model: model.clone(),
            context_window: 128_000,
            max_steps: 64,
            // 子进程单轮任务：无压缩（短上下文；也不引入摘要改写）
            compactor: None,
        },
        llm,
        crate::compat_engine::QuickJsToolExecutor::new(None, "subagent", None),
    );

    // 协议事件流：stdout 逐行 JSON（形状对齐 pi AgentEvent serde——
    // message_update 正文增量 / message_end 权威内容 / agent_end 兜底）。
    // hooks 在流式 delta 时实时发 message_update（同序同源）。
    let outcome = agent
        .run_turn(
            TurnRequest {
                content: child.task.clone(),
                source: UserMsgSource::Inject,
            },
            HeaderReason::Initial,
            &mut cancel_rx,
        )
        .await;

    match &outcome {
        Ok(o) => {
            if !o.final_text.trim().is_empty() {
                emit_message_end(&o.final_text);
                emit_agent_end(&o.final_text, None);
            }
            0
        }
        Err(err) => {
            eprintln!("[bm-server:subagent] agent 运行失败: {err}");
            // agent_end 错误收尾（协议要求 agent_end 携带 messages 或 error）
            emit_agent_end_error(err.message.clone());
            1
        }
    }
}

/// 子代理 hooks：流式正文增量 → stdout 协议行（message_update）。
/// 与 bm-server StreamHooks 的区别：输出目标是协议流而非 SSE。
#[derive(Default)]
struct SubagentHooks {
    /// 累积正文（协议 message 字段 = 累积 partial，与 pi 一致）
    acc: String,
}

impl LoopHooks for SubagentHooks {
    fn on_stream_chunk(&mut self, _ctx: &StepCtx, text: &str) {
        self.acc.push_str(text);
        emit_message_update(&self.acc, text);
    }
}

/// 输出一行协议事件（每行 JSON + flush；stdout 是父进程解析的通道）。
/// 形状对齐 pi AgentEvent serde（`#[serde(tag="type", rename_all="snake_case")]`），
/// 只发父侧消费的三类事件；message 字段给最小但合法的形状。
fn emit_event(event: &serde_json::Value) {
    let Ok(json) = serde_json::to_string(event) else {
        return;
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{json}");
    let _ = out.flush();
}

fn assistant_message(text: &str) -> serde_json::Value {
    serde_json::json!({
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
    })
}

/// message_update（TextDelta 增量）：`assistantMessageEvent.delta` 是父侧
/// 的唯一消费字段（ingest_child_event 的 pointer 路径）。
fn emit_message_update(acc: &str, delta: &str) {
    emit_event(&serde_json::json!({
        "type": "message_update",
        "message": assistant_message(acc),
        "assistantMessageEvent": {
            "type": "text_delta",
            "delta": delta,
        },
    }));
}

/// message_end（权威内容；父侧在 output 为空时兜底取 message 文本）。
fn emit_message_end(text: &str) {
    emit_event(&serde_json::json!({
        "type": "message_end",
        "message": assistant_message(text),
    }));
}

/// agent_end（父侧 messages 兜底）。
fn emit_agent_end(text: &str, error: Option<String>) {
    emit_event(&serde_json::json!({
        "type": "agent_end",
        "sessionId": "subagent",
        "messages": [assistant_message(text)],
        "error": error,
    }));
}

/// 补发 `agent_end` 错误事件（协议要求 agent_end 携带 messages 或 error）。
fn emit_agent_end_error(message: String) {
    emit_event(&serde_json::json!({
        "type": "agent_end",
        "sessionId": "subagent",
        "messages": [],
        "error": message,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 对齐上游 subagents.rs::child_args 的完整形状
    #[test]
    fn parse_full_child_args_shape() {
        let args = vec![
            "--mode".to_string(),
            "json".to_string(),
            "--print".to_string(),
            "--no-session".to_string(),
            "--tools".to_string(),
            "read,bash,edit".to_string(),
            "--model".to_string(),
            "deepseek-chat".to_string(),
            "--thinking".to_string(),
            "low".to_string(),
            "--append-system-prompt".to_string(),
            "你是研究员。".to_string(),
            "Task: 调研 X".to_string(),
        ];
        let parsed = parse_child_args(&args).expect("parse");
        assert_eq!(
            parsed,
            ChildArgs {
                tools: vec!["read".into(), "bash".into(), "edit".into()],
                model: Some("deepseek-chat".into()),
                thinking: Some("low".into()),
                append_system_prompt: Some("你是研究员。".into()),
                task: "Task: 调研 X".into(),
            }
        );
    }

    /// --mode 的值 "json" 不得被当作位置参数
    #[test]
    fn mode_value_not_treated_as_positional() {
        let args = vec![
            "--mode".to_string(),
            "json".to_string(),
            "--print".to_string(),
            "--no-session".to_string(),
            "--tools".to_string(),
            "read".to_string(),
            "Task: 你好".to_string(),
        ];
        let parsed = parse_child_args(&args).expect("parse");
        assert_eq!(parsed.task, "Task: 你好");
        assert_eq!(parsed.tools, vec!["read".to_string()]);
    }

    /// 多行 system prompt 是单个 argv 元素（上游 push 一个 OsString）
    #[test]
    fn multiline_system_prompt_kept_as_single_arg() {
        let args = vec![
            "--append-system-prompt".to_string(),
            "第一行\n第二行".to_string(),
            "Task: 任务".to_string(),
        ];
        let parsed = parse_child_args(&args).expect("parse");
        assert_eq!(
            parsed.append_system_prompt.as_deref(),
            Some("第一行\n第二行")
        );
    }

    /// --skill 被忽略但值被消费（不污染位置参数）
    #[test]
    fn skill_flag_value_consumed() {
        let args = vec![
            "--skill".to_string(),
            "web-scraping".to_string(),
            "Task: 任务".to_string(),
        ];
        let parsed = parse_child_args(&args).expect("parse");
        assert_eq!(parsed.task, "Task: 任务");
    }

    #[test]
    fn missing_task_is_error() {
        let err = parse_child_args(&["--tools".to_string(), "read".to_string()]).unwrap_err();
        assert!(err.contains("任务"));
    }

    #[test]
    fn missing_flag_value_is_error() {
        let err = parse_child_args(&["--model".to_string()]).unwrap_err();
        assert!(err.contains("--model"));
    }

    #[test]
    fn mode_detection() {
        let child = vec![
            "--mode".to_string(),
            "json".to_string(),
            "--no-session".to_string(),
        ];
        assert!(should_enter_child_mode(&child));
        let server = vec!["--port".to_string(), "17321".to_string()];
        assert!(!should_enter_child_mode(&server));
    }

    /// 协议形状对齐 pi AgentEvent serde：父侧 ingest_child_event 的
    /// pointer 路径 `/assistantMessageEvent/delta` 必须命中。
    #[test]
    fn message_update_shape_matches_parent_pointer() {
        let v = serde_json::json!({
            "type": "message_update",
            "message": assistant_message("你好"),
            "assistantMessageEvent": { "type": "text_delta", "delta": "好" },
        });
        let delta = v
            .pointer("/assistantMessageEvent/delta")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        assert_eq!(delta, "好");
        assert_eq!(v["message"]["role"], "assistant");
    }

    /// agent_end 的 messages 兜底形状：父侧反向找 assistant 文本。
    #[test]
    fn agent_end_messages_backstop_shape() {
        let v = serde_json::json!({
            "type": "agent_end",
            "sessionId": "subagent",
            "messages": [assistant_message("最终结论")],
            "error": serde_json::Value::Null,
        });
        let text = v["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text, "最终结论");
    }

    /// 工具面过滤：角色 csv ∩ 内置工具集（hashline_edit 等 pi 特有工具被忽略）。
    #[test]
    fn tool_csv_intersects_builtin_names() {
        let builtin: Vec<&str> = crate::builtin_tools::BuiltinTools::NAMES.to_vec();
        let csv = vec!["read".to_string(), "bash".to_string(), "hashline_edit".to_string()];
        let kept: Vec<&str> = csv
            .iter()
            .map(String::as_str)
            .filter(|n| builtin.contains(n))
            .collect();
        assert_eq!(kept, vec!["read", "bash"]);
        // 空 csv = 全部内置
        assert!(builtin.len() >= 7);
    }
}
