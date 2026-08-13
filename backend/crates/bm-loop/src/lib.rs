//! 自研 ReactLoopAgent（主线 A6 骨架）。
//!
//! 目标（docs/HANDOFF_KERNEL_PHASE1.md §四 A6）：事件日志从"消息面级事实"
//! 升级为"执行级事实"后的**自研驱动循环**，替换 pi loop：
//!
//! - **turn/step 双层**：turn = 一次用户/目标输入到回复收尾；step = 一个
//!   模型-工具循环迭代；
//! - **inbox 双队列**：next-turn（待处理回合）/ next-step（回合内待执行步骤）；
//! - **每步从事件日志投影**（EventLog::derive_messages）构造模型可见历史；
//! - **五个扩展点**（[`points::LoopHooks`]）：pre-step / request /
//!   request-error / tools pre+post / turn-stopping；
//! - **LLM client**：OpenAI 兼容流式（复用 bm-core providers 配置）——
//!   A6 主体实现；
//! - **压缩双触发**：0.8 水线软触发 + overflow 硬触发，接自研压缩引擎，
//!   落 CompactionStart/Summary/End 事务——A6 主体实现；
//!
//! 验收：替换 pi loop 跑通同一套 30 轮 A/B 压缩对比（方法论已有）。
//!
//! 骨架范围（本 commit）：工具注册表 + 双队列 + 五扩展点接口定稿
//! （B4 pi-compat 的工具注册接入点即 [`model::ToolRegistry`]）。

pub mod engine;
pub mod model;
pub mod points;

pub use engine::{ReactLoopAgent, StepRequest, TurnRequest};
pub use model::{ToolDef, ToolRegistry};
pub use points::{LoopHooks, RequestCtx, StepCtx, StopCtx, ToolCtx};
