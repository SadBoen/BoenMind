//! 服务面实现（SERVICE_FACES 图纸）：内置能力包成 kernel 服务。
//!
//! 第一批（低风险面）：settings（bm-core plugin_settings 函数包）/
//! stats（事件日志聚合）。注册点 = serve_inner 的 KernelBuilder（与
//! bm-compactor 同轨）；消费方经 `kernel.port` 取用（routes/plugins.rs、
//! routes/sessions.rs 已改为取服务）——"服务面 = 承诺 API，实现面可换"。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bm_core::plugin_settings::SettingField;
use bm_protocol::{
    BoxFuture, BranchId, CoreEvent, ErrorCode, EventKind, EventQuery, EventStorePort,
    ProtocolError, SessionId, SessionUsage,
};
use serde_json::Value;

/// 设置面实现：透传 bm-core plugin_settings。
///
/// 端口边界用 JSON 传 schema（manifest settings 声明的序列化，
/// 契约层零依赖 bm-core）；实现侧反序列化为字段集。
#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsPortImpl;

/// settings schema（`Vec<SettingField>`）从端口 JSON 边界还原；非法 = 空集
/// （调用方在路由层已校验过 manifest，此处仅防御）。
fn settings_fields(schema: &Value) -> Vec<bm_core::plugin_settings::SettingField> {
    serde_json::from_value(schema.clone()).unwrap_or_default()
}

impl bm_protocol::SettingsPort for SettingsPortImpl {
    fn read(&self, plugin_id: &str, schema: &Value) -> Value {
        let fields = settings_fields(schema);
        bm_core::plugin_settings::read_settings(plugin_id, &fields)
    }

    fn read_masked(&self, plugin_id: &str, schema: &Value) -> Value {
        let fields = settings_fields(schema);
        bm_core::plugin_settings::read_settings_masked(plugin_id, &fields)
    }

    fn save(
        &self,
        plugin_id: &str,
        schema: &Value,
        values: &Value,
    ) -> Result<Value, ProtocolError> {
        let fields = settings_fields(schema);
        bm_core::plugin_settings::save_settings(plugin_id, &fields, values)
            .map_err(|e| ProtocolError::new(ErrorCode::InvalidArgument, e.to_string()))
    }
}

/// 统计面实现：assistant/message 事件聚合（原 get_session_usage 内联逻辑
/// 服务化——消费方不再自己算）。
pub struct StatsPortImpl {
    pub store: Arc<dyn EventStorePort>,
}impl bm_protocol::StatsPort for StatsPortImpl {
    fn session_usage(
        &self,
        session_id: &SessionId,
    ) -> BoxFuture<'_, Result<SessionUsage, ProtocolError>> {
        let store = self.store.clone();
        let sid = session_id.clone();
        Box::pin(async move {
            let log = bm_kernel::EventLog::new(store);
            let q = EventQuery::of_type(sid, BranchId::new("main"), "assistant/message");
            let evs = log.read_where(q).await?;
            let mut usage = SessionUsage::default();
            for ev in &evs {
                if let EventKind::Core(CoreEvent::AssistantMessage { usage: Some(u), .. }) =
                    &ev.kind
                {
                    usage.input_tokens += u.input_tokens;
                    usage.output_tokens += u.output_tokens;
                    usage.messages += 1;
                }
            }
            Ok(usage)
        })
    }
}

/// 读设置（路由层消费入口）：优先经 kernel settings 服务面；kernel 不可用
/// 退化直调 bm-core（行为一致，仅接线差异——服务面是渐进替换不是闸门）。
pub fn read_settings(
    state: &crate::AppState,
    plugin_id: &str,
    schema: &[SettingField],
    masked: bool,
) -> Value {
    if let Some(kernel) = &state.kernel
        && let Ok(p) = kernel.port::<dyn bm_protocol::SettingsPort>("settings")
    {
        let json_schema = serde_json::to_value(schema).unwrap_or(Value::Null);
        if masked {
            p.read_masked(plugin_id, &json_schema)
        } else {
            p.read(plugin_id, &json_schema)
        }
    } else if masked {
        bm_core::plugin_settings::read_settings_masked(plugin_id, schema)
    } else {
        bm_core::plugin_settings::read_settings(plugin_id, schema)
    }
}

/// 保存设置（路由层消费入口）：同上，经服务面；kernel 不可用退化直调。
pub fn save_settings(
    state: &crate::AppState,
    plugin_id: &str,
    schema: &[SettingField],
    values: &Value,
) -> Result<Value, bm_core::AppError> {
    if let Some(kernel) = &state.kernel
        && let Ok(p) = kernel.port::<dyn bm_protocol::SettingsPort>("settings")
    {
        let json_schema = serde_json::to_value(schema).unwrap_or(Value::Null);
        p.save(plugin_id, &json_schema, values)
            .map_err(|e| bm_core::AppError::Invalid(e.to_string()))
    } else {
        bm_core::plugin_settings::save_settings(plugin_id, schema, values)
    }
}

/// LLM 能力面实现：bm-core providers 配置 → LlmConfig JSON 视图。
/// 桥接 resolve_llm_config（消费方 bm_engine.build_loop_agent 经服务取，
/// 服务不可用退化直调——渐进替换不是闸门）。
pub struct LlmPortImpl {
    pub config: Arc<RwLock<bm_core::config::AppConfig>>,
}

impl bm_protocol::LlmPort for LlmPortImpl {
    fn resolve_config(
        &self,
        provider_id: &str,
        model: &str,
        thinking: Option<&str>,
    ) -> Result<Value, ProtocolError> {
        let config = self.config.read().expect("config poisoned");
        let provider = config
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::NotFound,
                    format!("provider `{provider_id}` not found"),
                )
            })?;
        let cfg = crate::bm_engine::resolve_llm_config(provider, model, thinking)
            .map_err(|(_, msg)| ProtocolError::new(ErrorCode::InvalidArgument, msg))?;
        serde_json::to_value(cfg)
            .map_err(|e| ProtocolError::new(ErrorCode::InvalidArgument, e.to_string()))
    }

    fn providers(&self) -> Value {
        let config = self.config.read().expect("config poisoned");
        serde_json::to_value(&config.providers).unwrap_or(Value::Null)
    }
}

/// 凭证面实现：providers 配置的 api_key 读取（明文，仅宿主内部）。
pub struct CredentialsPortImpl {
    pub config: Arc<RwLock<bm_core::config::AppConfig>>,
}

impl bm_protocol::CredentialsPort for CredentialsPortImpl {
    fn api_key(&self, provider_id: &str) -> Option<String> {
        let config = self.config.read().expect("config poisoned");
        config
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .and_then(|p| p.api_key.clone())
    }
}

/// 技能面实现：包 bm-core skills 模块（config 读写锁；网络安装是阻塞
/// 操作——消费方 spawn_blocking 内调用）。
pub struct SkillPortImpl {
    pub config: Arc<RwLock<bm_core::config::AppConfig>>,
}

impl bm_protocol::SkillPort for SkillPortImpl {
    fn list(&self) -> Result<Value, ProtocolError> {
        let config = self.config.read().expect("config poisoned");
        let skills = bm_core::skills::list_skills(&config)
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, e.to_string()))?;
        serde_json::to_value(skills)
            .map_err(|e| ProtocolError::new(ErrorCode::InvalidArgument, e.to_string()))
    }

    fn install_path(&self, path: &str) -> Result<(), ProtocolError> {
        bm_core::skills::install_skill_from_path(std::path::Path::new(path))
            .map(|_| ())
            .map_err(|e| ProtocolError::new(ErrorCode::PluginInstall, e.to_string()))
    }

    fn install_github(&self, owner: &str, repo: &str, skill_id: &str) -> Result<(), ProtocolError> {
        bm_core::skills::install_skill_from_github(owner, repo, skill_id)
            .map(|_| ())
            .map_err(|e| ProtocolError::new(ErrorCode::PluginInstall, e.to_string()))
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), ProtocolError> {
        let mut config = self.config.write().expect("config poisoned");
        bm_core::skills::set_skill_enabled(&mut config, id, enabled)
            .map_err(|e| ProtocolError::new(ErrorCode::NotFound, e.to_string()))
    }

    fn uninstall(&self, id: &str) -> Result<(), ProtocolError> {
        let mut config = self.config.write().expect("config poisoned");
        bm_core::skills::uninstall_skill(&mut config, id)
            .map_err(|e| ProtocolError::new(ErrorCode::NotFound, e.to_string()))
    }
}

/// 工具面实现：宿主工具快照（compat 引擎装配的工具集；运行期注册）。
pub struct ToolsPortImpl {
    pub tools: Arc<std::sync::Mutex<Vec<bm_loop::model::ToolDef>>>,
}

impl bm_protocol::ToolsPort for ToolsPortImpl {
    fn list(&self) -> Vec<Value> {
        self.tools
            .lock()
            .expect("tools poisoned")
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                })
            })
            .collect()
    }

    fn has(&self, name: &str) -> bool {
        self.tools
            .lock()
            .expect("tools poisoned")
            .iter()
            .any(|t| t.name == name)
    }
}

/// 调度面实现：StewardStore（治理夹区间在 store 内部；set_wake(0) = 清除）。
/// 运行期注册（steward 构建在 kernel 之后）。消费方：管家 set_wake 工具、
/// 未来唤醒策略插件。
pub struct SchedulerPortImpl {
    pub store: Arc<crate::steward::StewardStore>,
}

impl bm_protocol::SchedulerPort for SchedulerPortImpl {
    fn set_wake(
        &self,
        session_id: &str,
        after_seconds: i64,
        reason: Option<&str>,
    ) -> BoxFuture<'_, Result<(), ProtocolError>> {
        let store = self.store.clone();
        let sid = session_id.to_string();
        let reason = reason.map(String::from);
        Box::pin(async move {
            store
                .set_wake(&sid, after_seconds, reason.as_deref())
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::InvalidArgument, e))
        })
    }

    fn clear_wake(&self, session_id: &str) -> BoxFuture<'_, Result<(), ProtocolError>> {
        let store = self.store.clone();
        let sid = session_id.to_string();
        Box::pin(async move {
            store
                .set_wake(&sid, 0, None)
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::InvalidArgument, e))
        })
    }
}

/// 通知面实现：AppState.session_streams（会话 SSE 通道表，tokio Mutex）。
/// 运行期注册（session_streams 构建在 kernel 之后）。事件 = AgentStreamEvent
/// JSON 视图（实现侧 serde 往返；非法事件/通道繁忙 = 推送失败）。
pub struct NotifyPortImpl {
    pub streams:
        Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<bm_core::agent::AgentStreamEvent>>>>,
}

impl bm_protocol::NotifyPort for NotifyPortImpl {
    fn push(&self, session_id: &str, event: Value) -> bool {
        let Ok(event) = serde_json::from_value::<bm_core::agent::AgentStreamEvent>(event) else {
            return false;
        };
        // 同步方法内短临界区：try_lock（send 非阻塞，持锁即放）
        let Ok(streams) = self.streams.try_lock() else {
            return false;
        };
        let Some(tx) = streams.get(session_id).cloned() else {
            return false;
        };
        tx.send(event).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_core::config::{AppConfig, ProviderConfig, ProviderKind};
    use bm_protocol::{CredentialsPort, LlmPort, NotifyPort, ToolsPort};

    fn test_config() -> Arc<RwLock<AppConfig>> {
        let mut cfg = AppConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "test-provider".into(),
            name: "测试".into(),
            kind: ProviderKind::Deepseek,
            base_url: Some("https://example.com/v1".into()),
            api_key: Some("sk-test".into()),
            models: vec!["m1".into()],
            default_model: None,
        });
        Arc::new(RwLock::new(cfg))
    }

    #[test]
    fn llm_port_resolves_config_json() {
        let port = LlmPortImpl { config: test_config() };
        let cfg = port.resolve_config("test-provider", "m1", None).unwrap();
        assert_eq!(cfg["base_url"], "https://example.com/v1");
        assert_eq!(cfg["api_key"], "sk-test");
        assert_eq!(cfg["model"], "m1");
        // 未知提供商 → NotFound
        let err = port.resolve_config("nope", "m1", None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        // 提供商清单 JSON 视图
        let list = port.providers();
        assert_eq!(list[0]["id"], "test-provider");
    }

    #[test]
    fn credentials_port_reads_api_key() {
        let port = CredentialsPortImpl { config: test_config() };
        assert_eq!(port.api_key("test-provider").as_deref(), Some("sk-test"));
        assert_eq!(port.api_key("nope"), None);
    }

    #[test]
    fn tools_port_lists_snapshot() {
        let tools = Arc::new(std::sync::Mutex::new(vec![bm_loop::model::ToolDef::new(
            "search",
            "搜索",
            serde_json::json!({"type": "object"}),
        )]));
        let port = ToolsPortImpl { tools };
        assert!(port.has("search"));
        assert!(!port.has("nope"));
        let list = port.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "search");
        assert_eq!(list[0]["inputSchema"]["type"], "object");
    }

    #[test]
    fn notify_port_pushes_to_session_channel() {
        use bm_core::agent::AgentStreamEvent;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentStreamEvent>();
        let streams: Arc<
            tokio::sync::Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<AgentStreamEvent>>>,
        > = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        streams.try_lock().unwrap().insert("s1".into(), tx);
        let port = NotifyPortImpl { streams };
        // JSON 视图（internally tagged）→ 通道
        assert!(port.push("s1", serde_json::json!({"type": "textDelta", "delta": "hi"})));
        match rx.try_recv() {
            Ok(AgentStreamEvent::TextDelta { delta }) => assert_eq!(delta, "hi"),
            other => panic!("unexpected: {other:?}"),
        }
        // 通道不存在 → false；非法事件 → false
        assert!(!port.push("nope", serde_json::json!({"type": "textDelta", "delta": "x"})));
        assert!(!port.push("s1", serde_json::json!({"type": "unknownEvent"})));
    }
}
