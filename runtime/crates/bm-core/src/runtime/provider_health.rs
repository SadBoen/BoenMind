//! Provider 熔断健康(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

/// M7 S5:Provider 健康状态(HTTP 熔断/MCP 重连共用;进程内软状态)。
#[derive(Debug, Clone, Default)]
pub struct ProviderHealth {
    pub status: &'static str, // "healthy" | "unavailable"
    /// HTTP:连续失败计数(>=3 开闸);MCP:未用。
    pub fail_streak: u32,
    /// MCP:unavailable 期间的重连探针次数(>=3 封禁)。
    pub reconnect_attempts: u32,
    /// HTTP:熔断冷却截止(半开放行探测);MCP:未用。
    pub cooldown_until: Option<chrono::DateTime<chrono::Utc>>,
}

pub(crate) const PROVIDER_FAIL_THRESHOLD: u32 = 3;
pub(crate) const PROVIDER_COOLDOWN_MS: i64 = 30_000;
pub(crate) const MCP_RECONNECT_LIMIT: u32 = 3;

/// "mcp.<server>.<tool>" -> "mcp.<server>"(健康面主体;其余原样)。
pub(crate) fn mcp_provider_of(capability: &str) -> String {
    let parts: Vec<&str> = capability.split('.').collect();
    if parts.len() >= 3 && parts[0] == "mcp" {
        format!("mcp.{}", parts[1])
    } else {
        capability.to_string()
    }
}

/// 健康迁移(只在状态变化时发事件;payload 见 registry)。
pub(crate) fn emit_provider_health(
    w: &mut World,
    provider: &str,
    from: &str,
    to: &str,
    reason: &str,
) {
    w.emit(
        EventType::ProviderHealthChanged,
        None,
        None,
        None,
        serde_json::json!({
            "provider": provider,
            "from": from,
            "to": to,
            "reason": reason,
        }),
    );
}

/// HTTP 模型连接器:连续失败计账(>=3 开闸熔断,冷却 30s)。
pub(crate) fn note_provider_failure(w: &mut World, provider: &str, reason: &str) {
    let now = w.config.clock.now();
    let entry = w.provider_health.entry(provider.to_string()).or_default();
    entry.fail_streak += 1;
    if entry.status != "unavailable" && entry.fail_streak >= PROVIDER_FAIL_THRESHOLD {
        entry.status = "unavailable";
        entry.cooldown_until = Some(now + chrono::Duration::milliseconds(PROVIDER_COOLDOWN_MS));
        emit_provider_health(w, provider, "healthy", "unavailable", reason);
    } else if entry.status == "unavailable" {
        // P1(第四轮评审):半开探测失败必须重开冷却——否则冷却过期后每个
        // 请求都穿透打到死 provider,熔断器只挡前 30 秒。
        entry.cooldown_until = Some(now + chrono::Duration::milliseconds(PROVIDER_COOLDOWN_MS));
    }
}

/// 成功落定:清计数;若在 unavailable(半开探测/重连成功)则恢复 healthy。
pub(crate) fn note_provider_success(w: &mut World, provider: &str, reason: &str) {
    let Some(entry) = w.provider_health.get_mut(provider) else {
        return;
    };
    entry.fail_streak = 0;
    entry.reconnect_attempts = 0;
    if entry.status == "unavailable" {
        entry.status = "healthy";
        entry.cooldown_until = None;
        emit_provider_health(w, provider, "unavailable", "healthy", reason);
    }
}
