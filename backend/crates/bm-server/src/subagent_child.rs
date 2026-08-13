//! 子代理（subagent）子进程入口。
//!
//! 上游 `subagent` 工具（legacy/pi_agent_rust/src/subagents.rs）会 spawn **当前
//! 可执行文件**（独立部署下即 bm-server 二进制）并传固定参数形状：
//! `--mode json --print --no-session --tools <csv> [--model M] [--thinking T]
//! [--skill S] [--append-system-prompt P] Task: <task>`，然后按 stdout 逐行
//! JSON 事件流收取结果（协议见 `subagents.rs::ingest_child_event`）。
//!
//! 本模块实现该入口：识别调用 → 解析参数 → 解析 provider（父进程经环境变量
//! 注入，或按模型匹配）→ 跑一轮隔离 agent → 以协议事件流回传。日志一律走
//! stderr，stdout 只承载协议事件。

use std::io::Write;
use std::path::PathBuf;

use pi::model::AssistantMessageEvent;
use pi::sdk::{AbortHandle, AgentEvent};

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
pub async fn run(args: &[String]) -> i32 {
    let child = match parse_child_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[bm-server:subagent] 参数解析失败: {e}");
            return 2;
        }
    };

    // 轻量初始化：与 bm-server init 的 agent 侧一致（config/agent 目录/models.json），
    // 但不启动 HTTP、不开数据库（no_session 模式不持久化）
    let config = bm_core::config::load();
    // edition 2024 中 set_var 为 unsafe
    unsafe {
        std::env::set_var("PI_CODING_AGENT_DIR", bm_core::config::pi_agent_dir());
    }
    if let Err(err) = bm_core::config::sync_pi_models_json(&config) {
        eprintln!("[bm-server:subagent] models.json 同步失败: {err}");
    }

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

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let tools = if child.tools.is_empty() {
        // 与上游 DEFAULT_CHILD_TOOLS 保持一致
        vec![
            "read".to_string(),
            "bash".to_string(),
            "edit".to_string(),
            "write".to_string(),
            "grep".to_string(),
            "find".to_string(),
            "ls".to_string(),
            "hashline_edit".to_string(),
        ]
    } else {
        child.tools.clone()
    };
    let handle = match bm_core::agent::create_child_session_handle(
        provider,
        &model,
        &cwd,
        tools,
        child.thinking.as_deref(),
        child.append_system_prompt.clone().unwrap_or_default(),
    )
    .await
    {
        Ok(h) => h,
        Err(err) => {
            eprintln!("[bm-server:subagent] 会话创建失败: {err}");
            emit_agent_end_error(format!("Failed to create child agent session: {err}"));
            return 1;
        }
    };

    // 协议事件流：stdout 逐行 JSON（AgentEvent 的 serde 输出即协议格式——
    // `#[serde(tag = "type", rename_all = "snake_case")]`）。只回传父侧消费的
    // 事件类型（message_update 的正文增量 / message_end / agent_end），
    // 其余（turn/message 生命周期、thinking 增量）父侧忽略，不发节省带宽。
    let mut handle = handle;
    let agent_error: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let agent_error_cb = agent_error.clone();
    // 子代理无父侧 steer/取消（父进程直接 kill 即传播取消），持有一个永不触发的
    // abort 信号即可满足 API 形状
    let (_abort_handle, abort_signal) = AbortHandle::new();
    let result = handle
        .prompt_with_abort(
            child.task.clone(),
            abort_signal,
            move |event: AgentEvent| {
                let mut emit = false;
                if let AgentEvent::MessageUpdate {
                    assistant_message_event,
                    ..
                } = &event
                {
                    emit = matches!(assistant_message_event, AssistantMessageEvent::TextDelta { .. });
                }
                if matches!(
                    &event,
                    AgentEvent::MessageEnd { .. } | AgentEvent::AgentEnd { .. }
                ) {
                    emit = true;
                }
                if let AgentEvent::AgentEnd { error, .. } = &event
                    && let Some(err) = error
                {
                    *agent_error_cb.lock().unwrap() = Some(err.clone());
                }
                if emit {
                    emit_event(&event);
                }
            },
        )
        .await;

    if let Err(err) = result {
        eprintln!("[bm-server:subagent] agent 运行失败: {err}");
        // 若 agent 结束事件未发出（如运行期异常），补发 error 收尾
        let ended = agent_error.lock().unwrap().is_some();
        if !ended {
            emit_agent_end_error(format!("{err}"));
        }
        return 1;
    }
    if agent_error.lock().unwrap().is_some() {
        return 1;
    }
    0
}

/// 输出一行协议事件（每行 JSON + flush；stdout 是父进程解析的通道）。
fn emit_event(event: &AgentEvent) {
    let Ok(json) = serde_json::to_string(event) else {
        return;
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{json}");
    let _ = out.flush();
}

/// 补发 `agent_end` 错误事件（协议要求 agent_end 携带 messages 或 error）。
fn emit_agent_end_error(message: String) {
    let payload = serde_json::json!({
        "type": "agent_end",
        "messages": [],
        "error": message,
    });
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{payload}");
    let _ = out.flush();
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
}
