//! BoenMind 领域层：配置、会话存储、工作文件夹、pi agent 封装。
//!
//! 本 crate 不依赖任何 HTTP 框架，供 bm-server（axum）与桌面壳（Tauri）复用。

pub mod agent;
pub mod config;
pub mod db;
pub mod plugins;
pub mod workspace;

pub use config::{AppConfig, ProviderConfig, ProviderKind};
pub use db::Db;
