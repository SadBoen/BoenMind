//! pi agent 封装：将会话选项映射到 BoenMind 的提供商配置，并把 agent
//! 事件流转换为面向 SSE 的扁平事件流。
//!
//! 设计约束：
//! - 不在本模块持有长生命周期的会话句柄（句柄由调用方持有，便于跨请求复用）
//! - 事件映射为纯函数，便于单元测试

use std::path::Path;

use pi::model::AssistantMessageEvent;
use pi::sdk::{
    AgentEvent, AgentSessionHandle, SessionOptions,
    create_agent_session,
};

use crate::config::ProviderConfig;

/// BoenMind 系统提示词：个人知识管理助手定位。
pub const SYSTEM_PROMPT: &str = r#"你是 BoenMind，一个专注工作与知识的个人助理。

你的职责：
1. 知识答疑：基于用户工作文件夹中的资料与通用知识回答问题，重要结论尽量给出依据。
2. 干活代理：用户请求执行任务时，明确列出步骤并执行。

行为准则：
- 回答简洁、准确、结构化；不确定时明确说明。
- 中文回答，除非用户使用其他语言。
- 涉及用户文件时，只访问工作文件夹范围内的内容。
- 不要修改工作文件夹内 .boenmind 目录下的任何文件（该目录是系统配置与索引数据，误改会破坏功能）。"#;

/// 从 agent 事件流转出的、面向前端 SSE 的扁平事件。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentStreamEvent {
    /// 正文增量（pi 的思考文本也以 `<think>` 标签随正文下发，思考增量事件不单列）
    TextDelta { delta: String },
    /// 工具调用开始（携带完整参数 JSON，来自 pi ToolExecutionStart）
    ToolCallStart { id: String, name: String, args: serde_json::Value },
    /// 工具调用结束（is_error 决定前端展示颜色）
    ToolCallEnd { id: String, name: String, is_error: bool },
    /// 整个 prompt 处理结束（含取消；前端据此固化流式内容）
    Done,
    /// 出错
    Error { message: String },
}

/// 创建 agent 会话句柄。
///
/// `provider`/`model` 来自会话或全局配置；`extension_paths` 为启用的插件路径
/// （pi QuickJS 运行时加载 TypeScript 扩展）；`skills_prompt` 为启用的 skill
/// 注入文本（available_skills 块，空串不注入）；`thinking` 为思考强度（如 "off"/"low"）；
/// `compaction` 为按模型解析的压缩设置（水线/尾部保护），`None` 时走 pi 现有全局行为。
pub async fn create_session_handle(
    provider: &ProviderConfig,
    model: &str,
    working_dir: &Path,
    extension_paths: Vec<std::path::PathBuf>,
    skills_prompt: &str,
    thinking: Option<&str>,
    compaction: Option<crate::compaction::ResolvedCompaction>,
) -> Result<AgentSessionHandle, pi::sdk::Error> {
    // 注意：调用前需确保 PI_CODING_AGENT_DIR 已设置、models.json 已同步，
    // 见 bm-server 的启动流程（sync_pi_models_json + set_var）
    let thinking_level = thinking
        .and_then(|t| t.parse::<pi::model::ThinkingLevel>().ok());
    let system_prompt = if skills_prompt.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{SYSTEM_PROMPT}{skills_prompt}")
    };
    let options = SessionOptions {
        provider: Some(provider.kind.pi_name(&provider.id)),
        model: Some(model.to_string()),
        api_key: provider.api_key.clone(),
        working_directory: Some(working_dir.to_path_buf()),
        // 内置工具全开：skill 需要 read/write/bash 等才能真正加载与执行；
        // 纯对话时代（无工具）无法使用 skill 文件与脚本。
        // subagent 为 opt-in 工具（上游 sdk.rs 注释），显式追加启用——
        // 子代理会 spawn 本进程（bm-server）的 --mode json 入口，见 bm-server subagent_child。
        enabled_tools: Some(
            pi::sdk::BUILTIN_TOOL_NAMES
                .iter()
                .copied()
                .chain(["subagent"])
                .map(|n| n.to_string())
                .collect(),
        ),
        no_session: true,
        system_prompt: Some(system_prompt),
        include_cwd_in_prompt: false,
        thinking: thinking_level,
        extension_paths,
        // 插件政策：默认（Prompt 模式，自动允许 read/write/http/events/session，拒绝 exec/env）
        extension_policy: None,
        // BoenMind 补丁对接：按模型压缩覆盖（水线/尾部/窗口），None 走 pi 默认
        compaction_settings: compaction.map(|c| pi::compaction::ResolvedCompactionSettings {
            enabled: c.enabled,
            context_window_tokens: c.context_window,
            reserve_tokens: c.reserve_tokens,
            keep_recent_tokens: c.keep_recent_tokens,
        }),
        ..Default::default()
    };
    create_agent_session(options).await
}

/// 创建子代理（subagent）子进程会话句柄。
///
/// 由 bm-server 的 `--mode json` 子代理入口调用（上游 subagent 工具 spawn 本
/// 二进制时使用），与 [`create_session_handle`] 的差异：
/// - 系统提示用 pi 默认 + `append_system_prompt`（角色定义正文经 argv 传入），
///   不注入 BoenMind 主 agent 的 SYSTEM_PROMPT
/// - 工具集来自角色定义白名单（`--tools` 参数），不是内置全开
/// - 不加载插件扩展（子代理保持轻量与隔离）
pub async fn create_child_session_handle(
    provider: &ProviderConfig,
    model: &str,
    working_dir: &Path,
    tools: Vec<String>,
    thinking: Option<&str>,
    append_system_prompt: String,
) -> Result<AgentSessionHandle, pi::sdk::Error> {
    let thinking_level = thinking
        .and_then(|t| t.parse::<pi::model::ThinkingLevel>().ok());
    let options = SessionOptions {
        provider: Some(provider.kind.pi_name(&provider.id)),
        model: Some(model.to_string()),
        api_key: provider.api_key.clone(),
        working_directory: Some(working_dir.to_path_buf()),
        enabled_tools: Some(tools),
        no_session: true,
        system_prompt: None,
        append_system_prompt: Some(append_system_prompt),
        include_cwd_in_prompt: false,
        thinking: thinking_level,
        extension_paths: Vec::new(),
        ..Default::default()
    };
    create_agent_session(options).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi::sdk::AgentEvent;

    fn end_with(error: Option<&str>) -> AgentEvent {
        AgentEvent::AgentEnd {
            session_id: std::sync::Arc::from("s"),
            messages: Vec::new(),
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn abort_maps_to_done() {
        // 用户点停止：pi 以 error: Some("Aborted") 收尾，前端须收到 Done
        // 才能固化已生成的部分文本（回归测试：曾错误映射为 Error 导致
        // UI 丢弃流式内容，而 DB 已入库，刷新后文本"复活"）
        assert!(matches!(
            map_agent_event(end_with(Some("Aborted")))[0],
            AgentStreamEvent::Done
        ));
    }

    #[test]
    fn real_error_maps_to_error() {
        assert!(matches!(
            map_agent_event(end_with(Some("upstream 502")))[0],
            AgentStreamEvent::Error { .. }
        ));
    }

    #[test]
    fn normal_end_maps_to_done() {
        assert!(matches!(
            map_agent_event(end_with(None))[0],
            AgentStreamEvent::Done
        ));
    }
}

/// 将 pi 的 AgentEvent 映射为 BoenMind 事件（可能产出 0..n 个）。
pub fn map_agent_event(event: AgentEvent) -> Vec<AgentStreamEvent> {
    match event {
        AgentEvent::MessageUpdate {
            assistant_message_event,
            ..
        } => match assistant_message_event {
            AssistantMessageEvent::TextDelta { delta, .. } => {
                vec![AgentStreamEvent::TextDelta { delta }]
            }
            // 思考增量不单列：思考内容随正文 TextDelta 以 <think> 标签下发，
            // 前端从正文解析（历史回放依赖同一格式）；结构化 thinking 流另行
            // 消费时再恢复此事件
            AssistantMessageEvent::ThinkingDelta { .. }
            | AssistantMessageEvent::ToolCallDelta { .. }
            | AssistantMessageEvent::ToolCallStart { .. }
            | AssistantMessageEvent::TextEnd { .. }
            | AssistantMessageEvent::ThinkingEnd { .. }
            | AssistantMessageEvent::Start { .. }
            | AssistantMessageEvent::TextStart { .. }
            | AssistantMessageEvent::ThinkingStart { .. } => Vec::new(),
            other => {
                // 兜底：后续 pi 版本可能新增事件变体，直接忽略
                let _ = other;
                Vec::new()
            }
        },
        // 工具真实执行开始/结束（pi SDK 权威事件，携带完整参数与执行状态）
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
            ..
        } => vec![AgentStreamEvent::ToolCallStart {
            id: tool_call_id,
            name: tool_name,
            args,
        }],
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            is_error,
            ..
        } => vec![AgentStreamEvent::ToolCallEnd {
            id: tool_call_id,
            name: tool_name,
            is_error,
        }],
        AgentEvent::TurnEnd { .. } => Vec::new(),
        AgentEvent::AgentEnd { error, .. } => match error {
            // pi 取消路径（用户点停止 / AbortSignal）以 `error: Some("Aborted")`
            // 收尾：取消不是错误，前端应收到 Done 来固化已生成的部分文本
            Some(err) if err == "Aborted" => vec![AgentStreamEvent::Done],
            Some(err) => vec![AgentStreamEvent::Error { message: err }],
            None => vec![AgentStreamEvent::Done],
        },
        AgentEvent::AgentStart { .. }
        | AgentEvent::TurnStart { .. }
        | AgentEvent::MessageStart { .. } => Vec::new(),
        other => {
            let _ = other;
            Vec::new()
        }
    }
}
