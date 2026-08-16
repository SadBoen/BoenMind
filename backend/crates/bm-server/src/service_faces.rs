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
}

impl bm_protocol::StatsPort for StatsPortImpl {
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

/// 厂商面实现：AppConfig.providers → ProviderDescriptor（bm-core 类型边界
/// 在实现内；协议层只见 JSON——零依赖纪律）。官方端点/协议形状单源
/// bm-core providers 表（ProviderConfig::descriptor），LlmPort 解析经此面。
pub struct ProviderPortImpl {
    pub config: Arc<RwLock<bm_core::config::AppConfig>>,
}

impl ProviderPortImpl {
    /// 按配置 id 查厂商描述（LlmPort 内部消费入口；同一注册表数据源）。
    pub fn descriptor(
        &self,
        provider_id: &str,
    ) -> Option<bm_core::providers::ProviderDescriptor> {
        let config = self.config.read().expect("config poisoned");
        config
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.descriptor())
    }
}

impl bm_protocol::ProviderPort for ProviderPortImpl {
    fn providers(&self) -> Value {
        let config = self.config.read().expect("config poisoned");
        let list: Vec<Value> = config
            .providers
            .iter()
            .map(|p| {
                let d = p.descriptor();
                serde_json::json!({
                    "stableId": d.stable_id,
                    "name": d.name,
                    "officialBaseUrl": d.official_base_url,
                    "shape": d.shape,
                    "models": p.models,
                })
            })
            .collect();
        Value::Array(list)
    }

    fn provider(&self, stable_id: &str) -> Option<Value> {
        let config = self.config.read().expect("config poisoned");
        config
            .providers
            .iter()
            .find(|p| p.descriptor().stable_id == stable_id)
            .map(|p| {
                let d = p.descriptor();
                serde_json::json!({
                    "stableId": d.stable_id,
                    "name": d.name,
                    "officialBaseUrl": d.official_base_url,
                    "shape": d.shape,
                    "models": p.models,
                })
            })
    }
}

/// LLM 能力面实现：bm-core providers 配置 → LlmConfig JSON 视图。
/// 桥接 resolve_llm_config（消费方 bm_engine.build_loop_agent 经服务取，
/// 服务不可用退化直调——渐进替换不是闸门）。
/// 官方端点/协议形状经 ProviderPort（厂商面）取，不再直读硬编码表
/// ——LLM provider 插件化方案 A（2026-08-16 拍板）。
pub struct LlmPortImpl {
    pub config: Arc<RwLock<bm_core::config::AppConfig>>,
    pub provider_port: Arc<ProviderPortImpl>,
}

impl bm_protocol::LlmPort for LlmPortImpl {
    fn resolve_config(
        &self,
        provider_id: &str,
        model: &str,
        thinking: Option<&str>,
    ) -> Result<Value, ProtocolError> {
        let provider = {
            let config = self.config.read().expect("config poisoned");
            config
                .providers
                .iter()
                .find(|p| p.id == provider_id)
                .cloned()
        }
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::NotFound,
                format!("provider `{provider_id}` not found"),
            )
        })?;
        let desc = self.provider_port.descriptor(provider_id).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::NotFound,
                format!("provider `{provider_id}` not found"),
            )
        })?;
        let cfg = crate::bm_engine::resolve_llm_config(&provider, &desc, model, thinking)
            .map_err(|(_, msg)| ProtocolError::new(ErrorCode::InvalidArgument, msg))?;
        let mut value = serde_json::to_value(cfg)
            .map_err(|e| ProtocolError::new(ErrorCode::InvalidArgument, e.to_string()))?;
        // 密钥不经 Port JSON 边界（审查 2026-08-17 A-8）；消费方走 CredentialsPort。
        if let Some(obj) = value.as_object_mut() {
            obj.insert("api_key".into(), Value::String(String::new()));
        }
        Ok(value)
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

/// 会话面实现：包 bm-core Db（turso 错误 → StoreUnavailable）。
pub struct SessionPortImpl {
    pub db: Arc<bm_core::db::Db>,
}

fn store_err(e: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::StoreUnavailable, e.to_string())
}

impl bm_protocol::SessionPort for SessionPortImpl {
    fn list(&self) -> BoxFuture<'_, Result<Value, ProtocolError>> {
        let db = self.db.clone();
        Box::pin(async move {
            let sessions = db.list_sessions().await.map_err(store_err)?;
            serde_json::to_value(sessions).map_err(store_err)
        })
    }

    fn create(
        &self,
        id: &str,
        provider_id: Option<&str>,
        model: Option<&str>,
        app: &str,
    ) -> BoxFuture<'_, Result<Value, ProtocolError>> {
        let db = self.db.clone();
        let (id, provider_id, model, app) = (
            id.to_string(),
            provider_id.map(String::from),
            model.map(String::from),
            app.to_string(),
        );
        Box::pin(async move {
            let session = db
                .create_session(&id, provider_id.as_deref(), model.as_deref(), &app)
                .await
                .map_err(store_err)?;
            serde_json::to_value(session).map_err(store_err)
        })
    }

    fn get(&self, id: &str) -> BoxFuture<'_, Result<Option<Value>, ProtocolError>> {
        let db = self.db.clone();
        let id = id.to_string();
        Box::pin(async move {
            let session = db.get_session(&id).await.map_err(store_err)?;
            match session {
                Some(s) => serde_json::to_value(s).map(Some).map_err(store_err),
                None => Ok(None),
            }
        })
    }

    fn rename(&self, id: &str, title: &str) -> BoxFuture<'_, Result<(), ProtocolError>> {
        let db = self.db.clone();
        let (id, title) = (id.to_string(), title.to_string());
        Box::pin(async move { db.rename_session(&id, &title).await.map_err(store_err) })
    }

    fn delete(&self, id: &str) -> BoxFuture<'_, Result<usize, ProtocolError>> {
        let db = self.db.clone();
        let id = id.to_string();
        Box::pin(async move { db.delete_session(&id).await.map_err(store_err) })
    }

    fn messages(&self, id: &str) -> BoxFuture<'_, Result<Value, ProtocolError>> {
        let db = self.db.clone();
        let id = id.to_string();
        Box::pin(async move {
            let msgs = db.list_messages(&id).await.map_err(store_err)?;
            serde_json::to_value(msgs).map_err(store_err)
        })
    }
}

/// 权限面实现：permission_pending 询问表（回传决策 → 唤醒等待中的上游）。
pub struct GatePortImpl {
    pub pending: Arc<
        tokio::sync::Mutex<
            HashMap<String, tokio::sync::oneshot::Sender<crate::PermissionDecision>>,
        >,
    >,
}

impl bm_protocol::GatePort for GatePortImpl {
    fn respond(
        &self,
        request_id: &str,
        allow: bool,
        always: bool,
    ) -> BoxFuture<'_, Result<(), ProtocolError>> {
        let pending = self.pending.clone();
        let rid = request_id.to_string();
        Box::pin(async move {
            let Some(tx) = pending.lock().await.remove(&rid) else {
                return Err(ProtocolError::new(
                    ErrorCode::NotFound,
                    format!("权限询问不存在: {rid}"),
                ));
            };
            let _ = tx.send(crate::PermissionDecision { allow, always });
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_core::config::{AppConfig, ProviderConfig, ProviderKind};
    use bm_protocol::{CredentialsPort, GatePort, LlmPort, NotifyPort, ToolsPort};

    fn test_config() -> Arc<RwLock<AppConfig>> {
        let mut cfg = AppConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "test-provider".into(),
            name: "测试".into(),
            kind: ProviderKind::Deepseek,
            shape: None,
            base_url: Some("https://example.com/v1".into()),
            api_key: Some("sk-test".into()),
            models: vec!["m1".into()],
            default_model: None,
        });
        Arc::new(RwLock::new(cfg))
    }

    #[test]
    fn llm_port_resolves_config_json() {
        let config = test_config();
        let provider_port = Arc::new(ProviderPortImpl { config: config.clone() });
        let port = LlmPortImpl { config, provider_port };
        let cfg = port.resolve_config("test-provider", "m1", None).unwrap();
        assert_eq!(cfg["base_url"], "https://example.com/v1");
        assert_eq!(cfg["api_key"], "");
        assert_eq!(cfg["model"], "m1");
        // 未知提供商 → NotFound
        let err = port.resolve_config("nope", "m1", None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        // 提供商清单 JSON 视图
        let list = port.providers();
        assert_eq!(list[0]["id"], "test-provider");
    }

    #[test]
    fn provider_port_lists_and_queries_by_stable_id() {
        use bm_protocol::ProviderPort;
        let port = ProviderPortImpl { config: test_config() };
        // 全量 JSON 视图：stableId 取代 pi_name（内置 deepseek → "deepseek"）
        let list = port.providers();
        let first = &list[0];
        assert_eq!(first["stableId"], "deepseek");
        assert_eq!(first["name"], "测试");
        assert_eq!(first["officialBaseUrl"], "https://api.deepseek.com/v1");
        assert_eq!(first["shape"], "openai-compatible");
        assert_eq!(first["models"][0], "m1");
        // 按 stable_id 查询；未知 → None
        assert!(port.provider("deepseek").is_some());
        assert!(port.provider("nope").is_none());
        // 官方端点单源：descriptor 与 JSON 视图一致（LlmPort 解析同源）
        let desc = port.descriptor("test-provider").unwrap();
        assert_eq!(desc.official_base_url, Some("https://api.deepseek.com/v1"));
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

    #[tokio::test]
    async fn gate_port_responds_to_pending_request() {
        let (tx, mut rx) = tokio::sync::oneshot::channel::<crate::PermissionDecision>();
        let pending: Arc<
            tokio::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<crate::PermissionDecision>>>,
        > = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        pending.try_lock().unwrap().insert("r1".into(), tx);
        let port = GatePortImpl { pending };
        // 未知询问 id → NotFound
        let err = port.respond("nope", true, false).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        // 已知 id → 决策送达
        port.respond("r1", true, true).await.unwrap();
        let decision = rx.try_recv().unwrap();
        assert!(decision.allow && decision.always);
    }
}
