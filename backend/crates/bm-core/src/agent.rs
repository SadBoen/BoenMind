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
- 涉及用户文件时，只访问工作文件夹范围内的内容。"#;

/// 从 agent 事件流转出的、面向前端 SSE 的扁平事件。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentStreamEvent {
    /// 正文增量
    TextDelta { delta: String },
    /// 思考过程增量
    ThinkingDelta { delta: String },
    /// 工具调用开始
    ToolCallStart { name: String },
    /// 工具调用参数增量
    ToolCallDelta { delta: String },
    /// 一次对话回合结束（含错误时由 error 携带）
    TurnEnd,
    /// 整个 prompt 处理结束
    Done,
    /// 出错
    Error { message: String },
}

/// 创建 agent 会话句柄。
///
/// `provider`/`model` 来自会话或全局配置；工具在 v1 默认不启用
/// （`enabled_tools` 为空），知识答疑与干活代理的能力后续期接入。
pub async fn create_session_handle(
    provider: &ProviderConfig,
    model: &str,
    working_dir: &Path,
) -> Result<AgentSessionHandle, pi::sdk::Error> {
    // 注意：调用前需确保 PI_CODING_AGENT_DIR 已设置、models.json 已同步，
    // 见 bm-server 的启动流程（sync_pi_models_json + set_var）
    let options = SessionOptions {
        provider: Some(provider.kind.pi_name().to_string()),
        model: Some(model.to_string()),
        api_key: provider.api_key.clone(),
        working_directory: Some(working_dir.to_path_buf()),
        enabled_tools: Some(Vec::new()),
        no_session: true,
        system_prompt: Some(SYSTEM_PROMPT.to_string()),
        include_cwd_in_prompt: false,
        ..Default::default()
    };
    create_agent_session(options).await
}

/// 将 pi 的 AgentEvent 映射为 BoenMind 事件（可能产出 0..n 个）。
pub fn map_agent_event(event: AgentEvent) -> Vec<AgentStreamEvent> {
    use pi::sdk::ContentBlock;

    match event {
        AgentEvent::MessageUpdate {
            assistant_message_event,
            ..
        } => match assistant_message_event {
            AssistantMessageEvent::TextDelta { delta, .. } => {
                vec![AgentStreamEvent::TextDelta { delta }]
            }
            AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                vec![AgentStreamEvent::ThinkingDelta { delta }]
            }
            AssistantMessageEvent::ToolCallDelta { delta, .. } => {
                vec![AgentStreamEvent::ToolCallDelta { delta }]
            }
            AssistantMessageEvent::ToolCallStart { partial, .. } => {
                let name = partial
                    .content
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::ToolCall(tool_call) => Some(tool_call.name.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "tool".to_string());
                vec![AgentStreamEvent::ToolCallStart { name }]
            }
            AssistantMessageEvent::TextEnd { .. }
            | AssistantMessageEvent::ThinkingEnd { .. }
            | AssistantMessageEvent::ToolCallEnd { .. }
            | AssistantMessageEvent::Start { .. }
            | AssistantMessageEvent::TextStart { .. }
            | AssistantMessageEvent::ThinkingStart { .. } => Vec::new(),
            other => {
                // 兜底：后续 pi 版本可能新增事件变体，直接忽略
                let _ = other;
                Vec::new()
            }
        },
        AgentEvent::TurnEnd { .. } => vec![AgentStreamEvent::TurnEnd],
        AgentEvent::AgentEnd { error, .. } => {
            if let Some(err) = error {
                vec![AgentStreamEvent::Error { message: err }]
            } else {
                vec![AgentStreamEvent::Done]
            }
        }
        AgentEvent::AgentStart { .. }
        | AgentEvent::TurnStart { .. }
        | AgentEvent::MessageStart { .. } => Vec::new(),
        other => {
            let _ = other;
            Vec::new()
        }
    }
}
