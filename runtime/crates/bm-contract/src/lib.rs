//! bm-contract:L1 合同库(boenmind-contracts/,v1.0 冻结)的 Rust 投影。
//!
//! 原则:合同文件是唯一真源。本 crate 内嵌全部冻结 schema/注册表文本,
//! 类型只做「方便实现」的镜像;同步测试保证镜像与合同文本不漂移。
//! 合同语义的解读条款见 milestones/M1-implementation-spec.md §8。

#[macro_use]
mod wirestr;

pub mod budget;
pub mod capability;
pub mod connector;
pub mod error_codes;
pub mod events;
pub mod exec_log;
pub mod ids;
pub mod registries;
pub mod schemas;
pub mod states;
pub mod timestamp;
pub mod wire;

/// ISO-8601 UTC 时间戳(合同约定:毫秒精度、`Z` 后缀)。
pub type BmTimestamp = String;
