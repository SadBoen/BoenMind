---
status: accepted
date: 2026-09-12
summary: 补完 surface→providers 依赖反转——抽 core 端口 SkillHost/JobBoard.list/McpAdmin,AdminConfig 全改端口,bm-providers 降为 dev-dependency,surface 源码零具体类型
supersedes: []
superseded_by: []
---

# ADR-0046: 补完 surface→providers 依赖反转(端口化管理面)

- 关联: ADR-0042(核实轮方法论;`ModelRouter` 已正确反转,本条补齐其余)、ADR-0014(W 序列管理面)、ADR-0045(身份接线)
- 背景: 重新评估发现 `bm-surface-http` **常规依赖** `bm-providers` 具体类型:`AdminConfig` 直接持 `Arc<bm_providers::mcp::McpHub>` / `jobs::JobTable` / `skill_wasm::SkillScriptManager`,管理面直接调 `openai_http::OpenAiConnector::new` / `supervisor::sync_from_config` / `sha256_file`。而**同样位置的 `ModelRouter` 早已正确抽成 core 端口**(ADR-0042)——这是**做了一半的反转**,是全项目唯一的结构性欠账。

## 决策

照 `ModelRouter` 先例,把管理面**实际用到**的 provider 能力逐个上移为 core 端口,DTO 一并上移:

1. **`ports::skill_host::SkillHost`**——wasm 宿主管理面契约:`load_plugins_file`/`register_skill`/`unregister_skill`/`unregister_all_generic`;实现方 `SkillScriptManager`。另上移 `placeholder_entries`(manifests→注册对,只用 contract + core 的 `provider_fn_with_meta`,故归 core 最合适)。
2. **`JobBoard::list()`**——`/admin/jobs` 需要的台账方法,扩现有 `JobBoard` 端口(带默认空表);实现方 `JobTable`。
3. **`ports::mcp_admin::McpAdmin`**——MCP 管理面契约:`probe_server`/`raw_request`/`stderr_tail`/`server_capabilities`/`disconnect_server`/`sync`;DTO(`StderrLine`/`SyncOutcome`/`CapabilityRegistrar`)与 `read_mcp_servers` 一并上移;实现方经组合适配器 `McpAdminAdapter(Arc<McpHub>)`(`McpHub::as_admin()`),因握手/同步需 `&Arc<McpHub>` 而 trait 方法只给 `&self`。
4. **`ModelRouter::build_connector`**——"按配置造连接器"是**工厂职责**,上移为端口方法;实现方 `RoutingConnector` 返回 `OpenAiConnector`。surface 只给 `(base_url, secrets)`,不再引用具体连接器类型。
5. **`AdminConfig` 三个字段改端口类型**(`hub`/`jobs`/`skills`);`sha256_file` 内联为 `read + bm_contract::hash::sha256_hex`(与 core 单源哈希一致)。
6. **`bm-providers` 从常规依赖降为 dev-dependency**——**编译期证明** surface 主依赖面不再需要它。

## 后果

- **反转补完**:`bm-surface-http` 源码 `bm_providers` 引用**归零**;主依赖面不再含 bm-providers(仅 dev,用于测试装配组合根/替身)。依赖图从"surface 常规依赖具体适配器"变为"surface 只依赖 core 端口"。
- **零行为变更**:501 测试全绿;`clippy -D warnings` 零警告。
- **真浏览器/HTTP 手测**:`/admin/capabilities`(身份 10/10 覆盖)、`/admin/jobs`(端口 list)、`/admin/mcp/reload` + `/admin/mcp/status`(经 `McpAdmin::sync`)均正常。
- 公共 API 路径不变:旧符号经 `pub use` re-export(`supervisor::{SyncOutcome,CapabilityRegistrar,read_mcp_servers}`、`mcp::StderrLine`),既有调用点零改动。
- 4 个新端口均带默认/窄接口,未为不存在的需求建模(`JobBoard::list` 默认空、`McpAdmin` 仅收管理面确实用到的 6 个操作)。
