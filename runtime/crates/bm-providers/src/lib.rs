//! bm-providers:ports 的 M1 实现。真实外部进程是 M7;本 crate 全部进程内。
//! 默认交付以确定性 mock 为主(规格 §4.3),真实 GLM 适配器 feature 门控。

pub mod builtin;
pub mod mock_model;
pub mod secret;

#[cfg(feature = "glm")]
pub mod glm_http;
