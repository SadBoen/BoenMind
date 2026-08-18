//! quickjs-bridge：QuickJS 宿主桥（占位）。
//!
//! 设计见 docs/design/QUICKJS_BRIDGE_DESIGN.md。本 crate 当前仅占位，
//! 保证 workspace 结构完整；宿主 API 面（host.llm / host.tools / host.session
//! / host.config / host.log）与 rquickjs 异步桥在后续里程碑实现。
//!
//! 边界（grok 评审 + 实测定稿）：
//! - JS 只做编排胶水；重逻辑（字符串/正则/JSON/网络/文件）一律回调宿主 Rust API；
//! - 类型只跨 JSON + 显式 schema；失败模型 `{ok, err:{code,retryable}}` 与 ToolGate 同码；
//! - rquickjs 不注入 fs/fetch（权限治理单点）；一个 JS 插件 = 一个入口模块 + manifest（声明 host 面）。
