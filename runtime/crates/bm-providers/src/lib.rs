//! bm-providers:ports 的实现层。M7 起含真实 OpenAI 兼容 HTTP 连接器(openai_http)。
//! 默认交付以确定性 mock 为主(规格 §4.3),真实 GLM 适配器 feature 门控。

pub mod builtin;
pub mod mcp;
pub mod mock_model;
pub mod openai_http;
pub mod routing;
pub mod secret;

#[cfg(feature = "glm")]
pub mod glm_http;
