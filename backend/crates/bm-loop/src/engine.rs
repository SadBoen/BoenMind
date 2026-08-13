//! ReactLoopAgent 骨架：inbox 双队列 + turn/step 位置跟踪。
//!
//! 循环主体（run）在 A6 主体实现：
//! 投影（EventLog::derive_messages）→ pre-step → request（OpenAI 兼容
//! 流式）→ 流式 chunk 落日志 → 工具执行（ToolRegistry 分发）→
//! turn-stopping 判定 → 压缩双触发（0.8 水线 / overflow 硬触发）。

use std::collections::VecDeque;

use crate::model::ToolRegistry;
use crate::points::LoopHooks;

/// 待处理回合（用户/目标输入）。
#[derive(Debug, Clone)]
pub struct TurnRequest {
    pub content: String,
    pub source: bm_protocol::UserMsgSource,
}

/// 回合内待执行步骤（目标驱动/继续指令注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepRequest {
    pub turn: u32,
}

/// 自研 loop 骨架。`run` 循环为 A6 主体实现（占位注释见模块头）。
pub struct ReactLoopAgent<H: LoopHooks = ()> {
    hooks: H,
    tools: ToolRegistry,
    /// inbox 双队列：next-turn（回合级）/ next-step（回合内步骤级）
    turn_queue: VecDeque<TurnRequest>,
    step_queue: VecDeque<StepRequest>,
    /// 当前位置（turn, step）；None = 空闲
    current: Option<(u32, u32)>,
    /// 回合计数（TurnStart 事件数 + 1 的进程内镜像；恢复时以日志为准）
    turn_count: u32,
}

impl<H: LoopHooks> ReactLoopAgent<H> {
    pub fn new(hooks: H, tools: ToolRegistry) -> Self {
        Self {
            hooks,
            tools,
            turn_queue: VecDeque::new(),
            step_queue: VecDeque::new(),
            current: None,
            turn_count: 0,
        }
    }

    /// 入队一个回合（next-turn 队列）。
    pub fn enqueue_turn(&mut self, req: TurnRequest) {
        self.turn_queue.push_back(req);
    }

    /// 入队一个步骤（next-step 队列）。
    pub fn enqueue_step(&mut self, req: StepRequest) {
        self.step_queue.push_back(req);
    }

    /// 待处理回合数。
    pub fn pending_turns(&self) -> usize {
        self.turn_queue.len()
    }

    /// 待处理步骤数。
    pub fn pending_steps(&self) -> usize {
        self.step_queue.len()
    }

    /// 当前位置（(turn, step)，None = 空闲）。
    pub fn current_position(&self) -> Option<(u32, u32)> {
        self.current
    }

    /// 开始新回合：分配 turn 号（进程内计数；恢复时以日志 TurnStart 计数为准），
    /// 步进位置置为 (turn, 0)。run 循环在落 TurnStart 事件后调用。
    pub fn begin_turn(&mut self) -> u32 {
        self.turn_count += 1;
        let turn = self.turn_count;
        self.current = Some((turn, 0));
        turn
    }

    /// 步进到下一步（step + 1）。run 循环在落 StepStart 事件后调用。
    pub fn advance_step(&mut self) -> (u32, u32) {
        let (turn, step) = self.current.get_or_insert((self.turn_count.max(1), 0));
        *step += 1;
        (*turn, *step)
    }

    /// 回合收尾：清当前位置。run 循环在落 TurnEnd 事件后调用。
    pub fn end_turn(&mut self) {
        self.current = None;
    }

    /// 工具注册表（B4：QuickJS 引擎的工具在此汇合）。
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    /// 扩展点访问（A6 主体调用；插件挂点）。
    pub fn hooks(&mut self) -> &mut H {
        &mut self.hooks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> ReactLoopAgent {
        ReactLoopAgent::new((), ToolRegistry::new())
    }

    #[test]
    fn inbox_queues_fifo() {
        let mut a = agent();
        a.enqueue_turn(TurnRequest {
            content: "t1".into(),
            source: bm_protocol::UserMsgSource::Human,
        });
        a.enqueue_turn(TurnRequest {
            content: "t2".into(),
            source: bm_protocol::UserMsgSource::Goal,
        });
        a.enqueue_step(StepRequest { turn: 1 });
        assert_eq!(a.pending_turns(), 2);
        assert_eq!(a.pending_steps(), 1);
        assert_eq!(a.turn_queue.pop_front().unwrap().content, "t1");
        assert_eq!(a.step_queue.pop_front().unwrap().turn, 1);
    }

    #[test]
    fn turn_step_position_tracking() {
        let mut a = agent();
        assert_eq!(a.current_position(), None, "初始空闲");
        assert_eq!(a.begin_turn(), 1);
        assert_eq!(a.current_position(), Some((1, 0)));
        assert_eq!(a.advance_step(), (1, 1));
        assert_eq!(a.advance_step(), (1, 2));
        a.end_turn();
        assert_eq!(a.current_position(), None);
        assert_eq!(a.begin_turn(), 2, "回合计数递增");
    }
}
