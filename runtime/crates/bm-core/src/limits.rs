//! W10(ADR-0024):运行时限制集中配置面。
//!
//! 全仓超时/上限/熔断类运行时项收敛为单一 `Limits` 结构;持久化 =
//! `<数据目录>/config/limits.json`(私有管理文件,不入冻结合同)。规则:
//! - 缺文件/坏键 = 回退代码默认(行为零变化,不拒启);
//! - 每键加载期安全钳制(区间单点在本文件 KEY_META);
//! - 优先级 env > 文件 > 代码默认(存量 BOEN_TURN_TIMEOUT_SECS 语义不变,
//!   由装配方在启动期折算进 Cell 并记来源);
//! - 热生效 = `LimitsCell` 共享快照单元,消费点读时取值(限制是运行配置
//!   而非域状态,不走单写者命令面;McpHub 共享 sink 同款先例)。

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

// ---- 限制结构(键序 = 设置页分组序)----------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    // 【命令执行】
    pub exec_default_ms: u64,
    pub exec_max_ms: u64,
    pub exec_output_max_chars: usize,
    pub job_retention_max: usize,
    pub job_retention_max_bytes: u64,
    // 【工具轮】
    pub tool_wait_ms: u64,
    pub approval_wait_ms: u64,
    pub tool_rounds_max: u32,
    pub loop_breaker_consecutive: u32,
    pub loop_breaker_window: usize,
    // 【模型调用】
    pub model_call_timeout_secs: u64,
    pub model_max_attempts: u32,
    // 【流式应答】
    pub stream_hard_cap_ms: u64,
    pub nonstream_wait_ms: u64,
    pub stream_keepalive_ms: u64,
    // 【MCP / Provider 健康】
    pub mcp_default_tool_timeout_ms: u64,
    pub mcp_remote_timeout_ms: u64,
    pub mcp_stdio_write_timeout_ms: u64,
    pub mcp_respawn_window_ms: u64,
    pub mcp_restart_limit: u32,
    pub mcp_reconnect_limit: u32,
    pub provider_fail_threshold: u32,
    pub provider_cooldown_ms: u64,
    // 【上下文 / 记忆】
    pub history_max_turns: usize,
    pub history_max_chars: usize,
    pub audit_entry_max_chars: usize,
    pub context_tail_max_bytes: u64,
    pub context_tail_entries: usize,
    pub context_search_max_limit: usize,
    pub session_messages_max_limit: usize,
    // 【任务监护】
    pub watchdog_stall_after_ms: i64,
    pub watchdog_hard_limit_ms: i64,
    pub watchdog_tick_ms: i64,
    pub autorun_default_max_turns: u32,
    // 【文件工具 / 技能】
    pub fs_rw_max_bytes: u64,
    pub fs_search_default_results: usize,
    pub fs_search_max_results: usize,
    pub fs_output_max_chars: usize,
    pub fs_skip_file_bytes: u64,
    pub fs_read_max_lines: usize,
    pub skill_default_timeout_ms: u64,
    // 【门户 / 管理面】
    pub login_max_failures: u32,
    pub login_lockout_secs: u64,
    pub portal_cookie_max_age_secs: u64,
    pub fs_preview_max_bytes: u64,
    pub fs_download_max_bytes: u64,
    pub fs_download_max_entries: usize,
    pub fs_delete_batch_max: usize,
    pub fs_browse_max_entries: usize,
    pub update_check_timeout_secs: u64,
    pub upgrade_download_timeout_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            exec_default_ms: 120_000,
            exec_max_ms: 600_000,
            exec_output_max_chars: 16_000,
            job_retention_max: 50,
            job_retention_max_bytes: 100 * 1024 * 1024,
            tool_wait_ms: 60_000,
            approval_wait_ms: 300_000,
            // 2026-09-07 架构评审 P0-1:总轮数安全网(变参轮转此前无界烧钱;
            // v0.0.10 刻意不设 30 上限的口径由「64 的宽松安全网 + 0=关」承接——
            // 正常链式工具调用远达不到,熔断只拦失控)。
            tool_rounds_max: 64,
            loop_breaker_consecutive: 5,
            loop_breaker_window: 20,
            model_call_timeout_secs: 120,
            model_max_attempts: 3,
            stream_hard_cap_ms: 900_000,
            nonstream_wait_ms: 180_000,
            stream_keepalive_ms: 10_000,
            mcp_default_tool_timeout_ms: 30_000,
            mcp_remote_timeout_ms: 60_000,
            mcp_stdio_write_timeout_ms: 10_000,
            mcp_respawn_window_ms: 60_000,
            mcp_restart_limit: 3,
            mcp_reconnect_limit: 3,
            provider_fail_threshold: 3,
            provider_cooldown_ms: 30_000,
            history_max_turns: 20,
            history_max_chars: 24_000,
            audit_entry_max_chars: 16 * 1024,
            context_tail_max_bytes: 2 * 1024 * 1024,
            context_tail_entries: 120,
            context_search_max_limit: 200,
            session_messages_max_limit: 200,
            watchdog_stall_after_ms: 15 * 60 * 1000,
            watchdog_hard_limit_ms: 24 * 60 * 60 * 1000,
            watchdog_tick_ms: 60 * 1000,
            autorun_default_max_turns: 6,
            fs_rw_max_bytes: 16 * 1024 * 1024,
            fs_search_default_results: 80,
            fs_search_max_results: 500,
            fs_output_max_chars: 16_000,
            fs_skip_file_bytes: 1024 * 1024,
            fs_read_max_lines: 10_000,
            skill_default_timeout_ms: 10_000,
            login_max_failures: 5,
            login_lockout_secs: 15 * 60,
            portal_cookie_max_age_secs: 30 * 24 * 3600,
            fs_preview_max_bytes: 512 * 1024,
            fs_download_max_bytes: 256 * 1024 * 1024,
            fs_download_max_entries: 5000,
            fs_delete_batch_max: 100,
            fs_browse_max_entries: 1000,
            update_check_timeout_secs: 20,
            upgrade_download_timeout_secs: 600,
        }
    }
}

// ---- 每键元数据(钳制区间/大白话标签;设置页与 GET /admin/limits 单一出处)--

pub struct KeyMeta {
    pub key: &'static str,
    /// 设置页分组。
    pub group: &'static str,
    /// 大白话标签。
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    /// false = 编译期/内部项,页面灰显只读。
    pub editable: bool,
}

macro_rules! meta {
    ($key:literal, $group:literal, $label:literal, $min:literal, $max:literal) => {
        KeyMeta {
            key: $key,
            group: $group,
            label: $label,
            min: $min,
            max: $max,
            editable: true,
        }
    };
}

/// 键元数据表(顺序 = 设置页展示序)。新增限制键必须在此登记,否则
/// 管理面不可见(结构体字段允许存在但视为内部项)。
pub const KEY_META: &[KeyMeta] = &[
    meta!(
        "exec_default_ms",
        "命令执行",
        "单条命令默认超时(毫秒)",
        1_000.0,
        600_000.0
    ),
    meta!(
        "exec_max_ms",
        "命令执行",
        "单条命令最长超时/前台硬顶(毫秒)",
        60_000.0,
        600_000.0
    ),
    meta!(
        "exec_output_max_chars",
        "命令执行",
        "命令输出保留字符数",
        1_000.0,
        200_000.0
    ),
    meta!(
        "job_retention_max",
        "命令执行",
        "后台作业保留个数",
        1.0,
        500.0
    ),
    meta!(
        "job_retention_max_bytes",
        "命令执行",
        "后台作业日志总量上限(字节)",
        1_048_576.0,
        1_073_741_824.0
    ),
    meta!(
        "tool_wait_ms",
        "工具轮",
        "免审批工具等待结果(毫秒)",
        5_000.0,
        600_000.0
    ),
    meta!(
        "approval_wait_ms",
        "工具轮",
        "等用户审批时限(毫秒)",
        10_000.0,
        1_800_000.0
    ),
    meta!(
        "tool_rounds_max",
        "工具轮",
        "单回合工具调用总轮数上限(0=不设上限)",
        0.0,
        1_000.0
    ),
    meta!(
        "loop_breaker_consecutive",
        "工具轮",
        "防空转熔断:同命令同参连续几次(0=关)",
        0.0,
        50.0
    ),
    meta!(
        "loop_breaker_window",
        "工具轮",
        "防空转熔断:记忆窗口条数",
        5.0,
        100.0
    ),
    meta!(
        "model_call_timeout_secs",
        "模型调用",
        "每次模型调用超时(秒)",
        10.0,
        3_600.0
    ),
    meta!(
        "model_max_attempts",
        "模型调用",
        "模型降级链重试次数",
        1.0,
        3.0
    ),
    meta!(
        "stream_hard_cap_ms",
        "流式应答",
        "流式回答总时长硬顶(毫秒)",
        60_000.0,
        7_200_000.0
    ),
    meta!(
        "nonstream_wait_ms",
        "流式应答",
        "非流式回答等待(毫秒)",
        30_000.0,
        3_600_000.0
    ),
    meta!(
        "stream_keepalive_ms",
        "流式应答",
        "流式保活间隔(毫秒)",
        1_000.0,
        60_000.0
    ),
    meta!(
        "mcp_default_tool_timeout_ms",
        "MCP 与通道健康",
        "MCP 工具默认超时(毫秒)",
        1_000.0,
        600_000.0
    ),
    meta!(
        "mcp_remote_timeout_ms",
        "MCP 与通道健康",
        "远程 MCP 请求超时(毫秒)",
        5_000.0,
        600_000.0
    ),
    meta!(
        "mcp_stdio_write_timeout_ms",
        "MCP 与通道健康",
        "MCP 本地管道写入超时(毫秒)",
        1_000.0,
        60_000.0
    ),
    meta!(
        "mcp_respawn_window_ms",
        "MCP 与通道健康",
        "插件崩溃重生熔断窗口(毫秒)",
        10_000.0,
        600_000.0
    ),
    meta!(
        "mcp_restart_limit",
        "MCP 与通道健康",
        "窗口内允许重生次数",
        1.0,
        20.0
    ),
    meta!(
        "mcp_reconnect_limit",
        "MCP 与通道健康",
        "重连探针封禁次数",
        1.0,
        20.0
    ),
    meta!(
        "provider_fail_threshold",
        "MCP 与通道健康",
        "模型通道连续失败熔断次数",
        2.0,
        20.0
    ),
    meta!(
        "provider_cooldown_ms",
        "MCP 与通道健康",
        "模型通道熔断冷却(毫秒)",
        5_000.0,
        600_000.0
    ),
    meta!(
        "history_max_turns",
        "上下文与记忆",
        "喂给模型的最近对话轮数",
        1.0,
        200.0
    ),
    meta!(
        "history_max_chars",
        "上下文与记忆",
        "喂给模型的对话总字数",
        1_000.0,
        200_000.0
    ),
    meta!(
        "audit_entry_max_chars",
        "上下文与记忆",
        "单条审计记录截断字符数",
        1_000.0,
        1_000_000.0
    ),
    meta!(
        "context_tail_max_bytes",
        "上下文与记忆",
        "上下文页尾读字节上限",
        65_536.0,
        67_108_864.0
    ),
    meta!(
        "context_tail_entries",
        "上下文与记忆",
        "上下文页返回条数",
        10.0,
        1_000.0
    ),
    meta!(
        "context_search_max_limit",
        "上下文与记忆",
        "跨会话检索单次上限条数",
        10.0,
        1_000.0
    ),
    meta!(
        "session_messages_max_limit",
        "上下文与记忆",
        "会话消息分页单页上限条数",
        10.0,
        1_000.0
    ),
    meta!(
        "watchdog_stall_after_ms",
        "任务监护",
        "任务无进展判停滞(毫秒)",
        60_000.0,
        86_400_000.0
    ),
    meta!(
        "watchdog_hard_limit_ms",
        "任务监护",
        "任务累计硬顶转 blocked(毫秒)",
        300_000.0,
        604_800_000.0
    ),
    meta!(
        "watchdog_tick_ms",
        "任务监护",
        "监护扫描节拍(毫秒)",
        10_000.0,
        600_000.0
    ),
    meta!(
        "autorun_default_max_turns",
        "任务监护",
        "自动驾驶默认轮数",
        1.0,
        50.0
    ),
    meta!(
        "fs_rw_max_bytes",
        "文件工具与技能",
        "文件读写大小上限(字节)",
        1_024.0,
        268_435_456.0
    ),
    meta!(
        "fs_search_default_results",
        "文件工具与技能",
        "搜索默认返回条数",
        1.0,
        500.0
    ),
    meta!(
        "fs_search_max_results",
        "文件工具与技能",
        "搜索返回条数硬顶",
        10.0,
        10_000.0
    ),
    meta!(
        "fs_output_max_chars",
        "文件工具与技能",
        "搜索/读取输出字符上限",
        1_000.0,
        1_000_000.0
    ),
    meta!(
        "fs_skip_file_bytes",
        "文件工具与技能",
        "搜索跳过大于此大小的文件(字节)",
        1_024.0,
        1_073_741_824.0
    ),
    meta!(
        "fs_read_max_lines",
        "文件工具与技能",
        "单次读取行数上限",
        100.0,
        200_000.0
    ),
    meta!(
        "skill_default_timeout_ms",
        "文件工具与技能",
        "技能脚本默认超时(毫秒)",
        100.0,
        600_000.0
    ),
    meta!(
        "login_max_failures",
        "门户与管理面",
        "登录失败锁定次数",
        3.0,
        50.0
    ),
    meta!(
        "login_lockout_secs",
        "门户与管理面",
        "登录锁定时长(秒)",
        60.0,
        86_400.0
    ),
    meta!(
        "portal_cookie_max_age_secs",
        "门户与管理面",
        "登录 Cookie 有效期(秒)",
        3_600.0,
        31_536_000.0
    ),
    meta!(
        "fs_preview_max_bytes",
        "门户与管理面",
        "文件预览大小上限(字节)",
        1_024.0,
        67_108_864.0
    ),
    meta!(
        "fs_download_max_bytes",
        "门户与管理面",
        "打包下载总量上限(字节)",
        1_048_576.0,
        10_737_418_240.0
    ),
    meta!(
        "fs_download_max_entries",
        "门户与管理面",
        "打包下载条目上限",
        100.0,
        100_000.0
    ),
    meta!(
        "fs_delete_batch_max",
        "门户与管理面",
        "单次批量删除上限",
        1.0,
        1_000.0
    ),
    meta!(
        "fs_browse_max_entries",
        "门户与管理面",
        "目录浏览单层条数上限",
        10.0,
        100_000.0
    ),
    meta!(
        "update_check_timeout_secs",
        "门户与管理面",
        "检查更新超时(秒)",
        5.0,
        120.0
    ),
    meta!(
        "upgrade_download_timeout_secs",
        "门户与管理面",
        "升级包下载超时(秒)",
        60.0,
        7_200.0
    ),
];

impl Limits {
    /// 按元数据表逐键归界(手改文件与 PUT 同受约束;越界静默归界)。
    pub fn clamped(mut self) -> Self {
        let d = Limits::default();
        let mut v = serde_json::to_value(&self).expect("Limits 序列化");
        let obj = v.as_object_mut().expect("结构体必为对象");
        for m in KEY_META {
            if let Some(n) = obj.get_mut(m.key).and_then(|x| x.as_f64()) {
                let c = n.clamp(m.min, m.max);
                obj.insert(
                    m.key.to_string(),
                    serde_json::Number::from_f64(c)
                        .map(|n| {
                            if n.is_f64() && c.fract() == 0.0 {
                                serde_json::Value::Number(serde_json::Number::from(c as i64))
                            } else {
                                serde_json::Value::Number(n)
                            }
                        })
                        .unwrap_or(serde_json::Value::Null),
                );
            }
        }
        self = serde_json::from_value(v).unwrap_or(d);
        self
    }

    /// 从文件 JSON 合成:缺键=默认,未知键忽略,坏值=整文件回退默认。
    pub fn from_file_value(raw: &serde_json::Value) -> Self {
        let mut merged = serde_json::to_value(Limits::default()).expect("默认序列化");
        if let (Some(dst), Some(src)) = (merged.as_object_mut(), raw.as_object()) {
            for (k, val) in src {
                if dst.contains_key(k) {
                    dst.insert(k.clone(), val.clone());
                }
            }
        }
        serde_json::from_value::<Limits>(merged)
            .map(Limits::clamped)
            .unwrap_or_default()
    }

    pub fn to_file_value(&self) -> serde_json::Value {
        serde_json::to_value(self.clone().clamped()).expect("Limits 序列化")
    }
}

// ---- 共享快照单元 ----------------------------------------------------------

/// 热生效载体:各执行体/管理面持同一 Cell,读时取值(clone 很小——全数值结构)。
#[derive(Clone)]
pub struct LimitsCell(Arc<RwLock<Limits>>);

impl LimitsCell {
    pub fn new(limits: Limits) -> Self {
        Self(Arc::new(RwLock::new(limits.clamped())))
    }

    pub fn with_default() -> Self {
        Self::new(Limits::default())
    }

    pub fn get(&self) -> Limits {
        self.0.read().expect("limits 锁未中毒").clone()
    }

    pub fn set(&self, limits: Limits) {
        *self.0.write().expect("limits 锁未中毒") = limits.clamped();
    }
}

impl Default for LimitsCell {
    fn default() -> Self {
        Self::with_default()
    }
}

impl std::fmt::Debug for LimitsCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LimitsCell").field(&self.get()).finish()
    }
}

// ---- 加载与来源追踪(设置页「env 覆盖」徽标用)------------------------------

#[derive(Debug, Clone, Default)]
pub struct LimitsSources {
    /// limits.json 原文(缺文件 = None);GET /admin/limits 据此标注
    /// 每键来源 default|file。
    pub file_raw: Option<serde_json::Value>,
    /// 被启动期 env 覆写的键(目前仅 model_call_timeout_secs)。
    pub env_keys: Vec<String>,
}

impl LimitsSources {
    pub fn source_of(&self, key: &str) -> &'static str {
        if self.env_keys.iter().any(|k| k == key) {
            "env"
        } else if self.file_raw.as_ref().and_then(|v| v.get(key)).is_some() {
            "file"
        } else {
            "default"
        }
    }
}

/// 装配方入口:读 `<数据目录>/config/limits.json`(缺/坏 = 默认)→ 钳制 → Cell。
/// env 覆盖(BOEN_TURN_TIMEOUT_SECS)由调用方折算后登记进 Sources。
pub fn load_limits(path: &std::path::Path) -> (LimitsCell, LimitsSources) {
    let mut sources = LimitsSources::default();
    let mut limits = Limits::default();
    if let Ok(text) = std::fs::read_to_string(path)
        && let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text)
    {
        limits = Limits::from_file_value(&raw);
        sources.file_raw = Some(raw);
    }
    // 存量 env 语义(ADR-0024 §1):env > 文件。非法/缺省时回落默认
    // ——与默认相等则视为未设。
    if let Ok(v) = std::env::var("BOEN_TURN_TIMEOUT_SECS")
        && let Ok(secs) = v.parse::<i64>()
        && secs > 0
    {
        limits.model_call_timeout_secs = secs as u64;
        sources.env_keys.push("model_call_timeout_secs".to_string());
    }
    (LimitsCell::new(limits), sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_documented_values() {
        let l = Limits::default();
        assert_eq!(l.exec_default_ms, 120_000);
        assert_eq!(l.exec_max_ms, 600_000);
        assert_eq!(l.model_call_timeout_secs, 120);
        assert_eq!(l.history_max_turns, 20);
        assert_eq!(l.loop_breaker_consecutive, 5);
    }

    #[test]
    fn missing_file_shape_yields_defaults() {
        let l = Limits::from_file_value(&serde_json::json!({}));
        assert_eq!(l, Limits::default());
    }

    #[test]
    fn unknown_keys_ignored_partial_override_applied() {
        let l = Limits::from_file_value(&serde_json::json!({
            "exec_default_ms": 300_000,
            "no_such_key": 1
        }));
        assert_eq!(l.exec_default_ms, 300_000);
        assert_eq!(l.exec_max_ms, Limits::default().exec_max_ms);
    }

    #[test]
    fn out_of_range_values_clamp_on_load() {
        let l = Limits::from_file_value(&serde_json::json!({
            "exec_max_ms": 99_999_999,
            "login_lockout_secs": 1
        }));
        assert_eq!(l.exec_max_ms, 600_000);
        assert_eq!(l.login_lockout_secs, 60);
    }

    #[test]
    fn wrong_typed_file_falls_back_to_default() {
        let l = Limits::from_file_value(&serde_json::json!({ "exec_default_ms": "abc" }));
        assert_eq!(l, Limits::default());
    }

    #[test]
    fn cell_set_get_roundtrip_and_clamp() {
        let cell = LimitsCell::with_default();
        let l = Limits {
            exec_default_ms: 999_999_999, // 超 600k 天花板
            ..Limits::default()
        };
        cell.set(l);
        assert_eq!(cell.get().exec_default_ms, 600_000);
    }

    #[test]
    fn roundtrip_through_file_value_is_stable() {
        let l = Limits::default();
        assert_eq!(Limits::from_file_value(&l.to_file_value()), l);
    }

    #[test]
    fn key_meta_covers_every_serialized_key() {
        let obj = serde_json::to_value(Limits::default())
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let meta = KEY_META
            .iter()
            .map(|m| m.key.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(obj, meta, "Limits 字段与 KEY_META 必须一一对应");
    }

    // 2026-09-07 架构评审(P2):此前只断言键名一一对应,不校验区间与默认值
    // 一致性——钳制表与默认值漂移(默认越界=加载期被静默改写)无人知晓。
    #[test]
    fn key_meta_ranges_cover_defaults() {
        let defaults = serde_json::to_value(Limits::default()).unwrap();
        for m in KEY_META {
            let d = defaults[m.key]
                .as_f64()
                .unwrap_or_else(|| panic!("KEY_META {} 在 Limits 默认值中缺失或非数值", m.key));
            assert!(
                d >= m.min && d <= m.max,
                "KEY_META {} 默认值 {d} 越出钳制区间 [{}, {}]",
                m.key,
                m.min,
                m.max
            );
        }
    }
}
