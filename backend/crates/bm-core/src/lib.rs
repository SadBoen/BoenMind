//! BoenMind 领域层：配置、会话存储、工作文件夹、pi agent 封装。
//!
//! 本 crate 不依赖任何 HTTP 框架，供 bm-server（axum）与桌面壳（Tauri）复用。

pub mod agent;
pub mod compaction;
pub mod config;
pub mod db;
pub mod error;
mod http_util;
pub mod plugin_settings;
pub mod plugin_test;
pub mod plugins;
pub mod providers;
pub mod skills;
pub mod thinking;
pub mod workspace;

pub use config::{AppConfig, ProviderConfig, ProviderKind};
pub use db::Db;
pub use error::AppError;
