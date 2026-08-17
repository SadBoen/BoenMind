//! Agent 公共类型：系统提示词 + 面向前端 SSE 的扁平事件流形状。
//!
//! pi 引擎封装（create_session_handle / map_agent_event）已于 2026-08-15
//! pi 废除轮删除——执行引擎 = 自研 bm-loop，事件由 bm_engine 直接产出。

/// BoenMind 系统提示词：个人知识管理助手定位。
pub const SYSTEM_PROMPT: &str = r#"你是 BoenMind，一个专注工作与知识的个人助理。

你的职责：
1. 知识答疑：基于用户工作文件夹中的资料与通用知识回答问题，重要结论尽量给出依据。
2. 干活代理：用户请求执行任务时，明确列出步骤并执行。

行为准则：
- 回答简洁、准确、结构化；不确定时明确说明。
- 中文回答，除非用户使用其他语言。
- 涉及用户文件时，只访问工作文件夹范围内的内容。
- 不要修改工作文件夹内 .boenmind 目录下的任何文件（该目录是系统配置与索引数据，误改会破坏功能）。

派工（subagent 工具）：
- 派任务时在 task 里写明期望的输出格式（如"最终输出必须是一个 JSON 对象，含字段 summary/findings"），队员会按约定交付。
- 工具结果末尾的 <subagent-structured-result> 块是每个队员的结构化字段（agent/status/exitCode/output/error 等），取用结果以该块为准，不要依赖正文摘要转述。

改进建议（submit_refinement_suggestions 工具）：
- 任务完成后，若发现某个启用中的 skill 的描述（description）或系统提示词存在误导、不准确或明显可改进之处，调用 submit_refinement_suggestions 提交建议（含原文、建议文本、原因）。
- 建议仅被记录，用户审批后才生效——不要声称已生效；没有可改进之处时绝不调用，同一问题不要重复提交。

工具使用提示：
- read 支持 offset/limit（字节区间）：需要看文件特定行段时直接传参数，不要用 shell 命令绕（M1 验收教训：行区间靠 powershell/findstr 技巧是步数燃烧主因）。
- grep/find 默认尊重 .gitignore 与 .ignore 并跳过隐藏文件（不会遍历 target/ 等被忽略目录）；大范围搜索可传 timeout（毫秒）限制单次时长。"#;

/// 从 agent 事件流转出的、面向前端 SSE 的扁平事件。
/// Deserialize：通知面（SERVICE_FACES #13）JSON 边界往返。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentStreamEvent {
    /// 正文增量（思考文本以 `<think>` 标签随正文下发，思考增量事件不单列）
    TextDelta { delta: String },
    /// 工具调用开始（携带完整参数 JSON）
    ToolCallStart { id: String, name: String, args: serde_json::Value },
    /// 工具调用结束（is_error 决定前端展示颜色）
    ToolCallEnd { id: String, name: String, is_error: bool },
    /// 插件权限询问：插件请求某能力（exec/env/网络等），前端需弹窗让用户选择。
    /// 前端通过 POST /api/chat/permission-response 回传决策；无响应超时后后端 fail-closed 拒绝。
    PermissionRequest {
        /// 询问请求 id（回传时原样带回）
        id: String,
        extension_id: Option<String>,
        /// 能力名（exec / env / 其它 hostcall 能力）
        capability: String,
        /// 面向用户的询问文案（title: message 或 method 驱动的描述）
        message: String,
    },
    /// ask_user 工具询问：模型向用户提问，前端弹窗，回答经
    /// POST /api/chat/ask-response 回传；无响应超时后按"无回答"失败收尾。
    AskUser {
        /// 询问请求 id（回传时原样带回）
        id: String,
        /// 面向用户的问题
        question: String,
    },
    /// 任务心跳进度（每 5s 由 bm-server 心跳 task 推送；进行中任务的状态条展示）。
    /// 仅活跃 prompt 期间出现，不作为消息落库。
    TaskProgress { progress: String },
    /// 整个 prompt 处理结束（含取消；前端据此固化流式内容）
    Done,
    /// 出错
    Error { message: String },
}
