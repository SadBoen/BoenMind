//! 五个扩展点（A6 骨架定稿；调用点随主体实现接入）。
//!
//! 与 dsh 扩展点对齐（回合词汇表/交互契约借鉴）：
//! pre-step / request / request-error / tools pre+post / turn-stopping。

/// 步骤上下文（pre-step）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepCtx {
    pub turn: u32,
    pub step: u32,
}

/// 模型请求上下文（request / request-error）。
#[derive(Debug, Clone)]
pub struct RequestCtx {
    pub turn: u32,
    pub step: u32,
    /// 模型可见历史的审计锚点（prompt_hash，见 A2 request/header）
    pub prompt_hash: Option<String>,
}

/// 工具执行上下文（tools pre / post）。
#[derive(Debug, Clone)]
pub struct ToolCtx {
    pub turn: u32,
    pub step: u32,
    pub call_id: String,
    pub name: String,
    pub args: serde_json::Value,
}

/// 工具执行闸门（on_tool_pre 返回）：允许 / 拒绝（拒绝原因进工具结果，
/// 模型可见——"模型可见即已记录"）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolGate {
    #[default]
    Allow,
    Deny(String),
}

/// 回合停止判定上下文（turn-stopping）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopCtx {
    pub turn: u32,
}

/// Loop 扩展点。默认实现全部空操作/不拦截——插件化挂点
/// （记忆插件、Steward 观察、权限询问桥等在此接线）。
///
/// 服务面铺开（SERVICE_FACES 图纸 §二）：在 A6 骨架五个扩展点之上，
/// 补上下列点名挂点（每个 = 一个真实需求，默认空实现零破坏）：
/// context-build / compact-begin / compact-end / turn-end / provider-select。
pub trait LoopHooks: Send + Sync {
    /// 步骤开始前（每步一次；可注入/改写下一步意图）。
    fn on_pre_step(&mut self, _ctx: &StepCtx) {}

    /// 流式正文增量（SSE 前端通道挂点，A6 接线件）：每个 TextDelta 一次，
    /// 与事件日志落盘同源同序（loop 先入日志队列再调本钩子，投影侧无竞态）。
    /// 集成方（bm-server）在此转发 AgentStreamEvent::TextDelta 给前端。
    fn on_stream_chunk(&mut self, _ctx: &StepCtx, _text: &str) {}

    /// 构造模型请求前（可改写 payload：追加系统段/工具过滤）。
    fn on_request(&mut self, _ctx: &RequestCtx, _payload: &mut serde_json::Value) {}

    /// 模型请求出错后；返回 true = 由 loop 按策略重试，false = 回合失败收尾。
    fn on_request_error(&mut self, _ctx: &RequestCtx, _err: &str) -> bool {
        false
    }

    /// 工具执行前（权限/预算挂点）：返回 [`ToolGate::Deny`] 即不执行——
    /// loop 把拒绝原因作为工具结果落日志（模型可见）。
    fn on_tool_pre(&mut self, _ctx: &ToolCtx) -> ToolGate {
        ToolGate::Allow
    }

    /// 工具执行后（输出审计/记忆写入挂点）。
    fn on_tool_post(&mut self, _ctx: &ToolCtx, _ok: bool) {}

    /// 模型回复后判定回合是否收尾。默认 false（继续下一步）。
    fn on_turn_stopping(&mut self, _ctx: &StopCtx, _last_assistant_text: &str) -> bool {
        false
    }

    /// 模型请求载荷构造完成、发送前（比 on_request 更早——header/上下文
    /// 组装之后；提示词插件 D8 在此挂接）。默认不动。
    fn on_context_build(&mut self, _ctx: &RequestCtx) {}

    /// 压缩开始前（策略已判定要压缩；评估/审计挂点）。
    fn on_compact_begin(&mut self, _ctx: &RequestCtx) {}

    /// 压缩完成后（压缩摘要已落日志；统计挂点）。
    fn on_compact_end(&mut self, _ctx: &RequestCtx) {}

    /// 回合结束（TurnEnd 已落日志、end_turn 之后；统计/记忆沉淀挂点）。
    fn on_turn_end(&mut self, _ctx: &StopCtx) {}

    /// 提供商/模型选定前（路由/成本策略挂点；默认不干预）。
    fn on_provider_select(&mut self, _provider: &str, _model: &str) {}
}

/// 空实现（默认 hooks）。
impl LoopHooks for () {}
