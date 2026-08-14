//! 自研 ReactLoopAgent（主线 A6 主体）。
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
//! - **LLM client**（[`llm`]）：OpenAI 兼容流式，提供商配置由集成方从
//!   bm-core 解析注入（本 crate 不依赖 bm-core，铁律 3 见 tests/architecture.rs）；
//! - **压缩**（[`compact`]）：事务协议在 loop（三事件 + replace 遮蔽 +
//!   fail-safe 摘要失败不遮蔽）；策略在插件（[`compact::Compactor`] 契约，
//!   bm-compactor 为默认实现，可换可关）。关插件 = 软触发不动作、超窗
//!   失败回合——优雅失败（框架重点不是裸跑，v0.17 用户定调）。
//!
//! 验收：替换 pi loop 跑通同一套 30 轮 A/B 压缩对比（方法论已有）；
//! 替换开关时机由用户拍板（拍板点 4，先并行双开对比）。
//!
//! B4 接线点：QuickJS 引擎的工具经 [`model::ToolRegistry`] 注册，
//! 执行侧实现 [`engine::ToolExecutor`]（bm-server 侧做 QuickJS hostcall 分发）。

pub mod compact;
pub mod engine;
pub mod llm;
pub mod model;
pub mod points;

pub use compact::{estimate_tokens, Compactor};
pub use engine::{ReactLoopAgent, StepRequest, TurnRequest};
pub use model::{ToolDef, ToolRegistry};
pub use points::{LoopHooks, RequestCtx, StepCtx, StopCtx, ToolCtx, ToolGate};
