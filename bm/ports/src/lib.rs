//! # bm-ports —— BoenMind 产品级契约层
//!
//! 内核契约（`kernel-contracts`）承载纯内核端口（LlmPort / FsPort / AuthPort /
//! SessionPersistPort…）；**产品级策略端口**（核心插件需要、但内核不提供的正交
//! 能力）放本层——如上下文压缩 `Compactor`。
//!
//! 由来（2026-08-20 回头看）：`Compactor` 原定义在功能插件 plugin-compactor，
//! 而核心插件 plugin-loop 编译期依赖该 trait →「核心依赖功能插件」依赖倒置
//! 硬伤。修复：trait（含默认事务实现）上提到产品契约层，功能插件只留策略
//! 实现（`DefaultCompactor`），loop 只依赖本层端口。`kernel/` submodule 只读，
//! 不进内核；本层是 BoenMind 侧的内核契约扩展面。
//!
//! 依赖纪律：本 crate 只依赖 `kernel-contracts`（纯契约），不依赖任何插件 /
//! 组合根——所有上层（plugin-loop / plugin-compactor / bm-assembly）向本层
//! 输入依赖均合法、向插件层输出依赖均违规。

pub mod compactor;

pub use compactor::{
    build_dialogue, estimate_tokens, Compactor, DEFAULT_CONTEXT_WINDOW,
};