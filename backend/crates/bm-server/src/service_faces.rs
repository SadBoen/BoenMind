//! 服务面实现（SERVICE_FACES 图纸）：内置能力包成 kernel 服务。
//!
//! 第一批（低风险面）：settings（bm-core plugin_settings 函数包）/
//! stats（事件日志聚合）。注册点 = serve_inner 的 KernelBuilder（与
//! bm-compactor 同轨）；消费方经 `kernel.port` 取用（routes/plugins.rs、
//! routes/sessions.rs 已改为取服务）——"服务面 = 承诺 API，实现面可换"。

use std::sync::Arc;

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
