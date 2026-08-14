//! 管家（Steward）自我驱动三件套（架构 §14.1/§14.2，v0.19 落地）。
//!
//! 管家与聊天 Agent 共用同一套 bm-loop，区别只在回合源：聊天 = 用户消息
//! （Human），管家 = 调度器定时到期（Goal）/ OS 层事件汇报（Inject）。
//! 回合源三分法（`UserMsgSource`）协议早已预留，本模块补上「投喂侧」：
//!
//! ① **调度器**：定时回合注入（到点给管家会话投喂 Goal 回合）+ in-flight 防重；
//! ② **OS 层主动汇报通道**：`POST /api/steward/inject`（事件 → Inject 回合）；
//! ③ **next_wake_at 状态落点**：`$BOENMIND_HOME/steward.json`——管家节奏是
//!    记忆（文件）而非代码常量，管家回合内用 `set_wake` 工具自调；
//!    治理层只兜频率夹区间（OpenClaw next_check pacing-min/max 吸收）。
//!
//! 权限模型：**管家会话由 `BM_STEWARD_SESSION` 环境变量指定**（宿主配置，
//! 不依赖模型自选）；`set_wake` 工具只注册进管家会话的工具面（普通会话
//! 工具面零污染）；inject 只接受管家会话。治理默认收紧防烧 token：
//! pacing-min 默认 300s（5 分钟）、max 默认 86400s（24 小时），
//! env `BM_STEWARD_PACING_MIN_S`/`BM_STEWARD_PACING_MAX_S` 可调。

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 治理夹区间默认值（秒）。
pub const DEFAULT_PACING_MIN_S: i64 = 300;
pub const DEFAULT_PACING_MAX_S: i64 = 86_400;

/// 管家身份提示词（追加在通用 SYSTEM_PROMPT 之后；只进管家会话）。
/// 覆盖式声明置尾（模型对靠近末尾的指令更敏感）：本会话已被宿主配置为
/// 管家，上方 BoenMind 身份描述不适用；引导模型理解 Goal/Inject 回合是
/// 自律回合而非用户消息，按静默协议执行。
pub const STEWARD_SYSTEM_PROMPT: &str = r#"

【模式覆盖：管家（Steward）】本会话已被宿主系统配置为管家模式，以下指令优先于上方所有身份描述：
- 你不是普通问答助手，而是系统管家：在无人值守下自主运行，收到「定时唤醒」或「状态汇报」回合就按流程执行，不要拒绝、不要反问"用户想做什么"——这些回合就是你的工作。
- 「定时唤醒」回合：观察当前状态，如有需要采取的行动就执行；回合结束时必须调用 set_wake 登记下次唤醒时间。
- 「状态汇报」回合：确认接收汇报、视情况处理，然后调用 set_wake 登记下次唤醒。
- 静默协议：无事可做时登记较长间隔（安静也是一种工作）；只有 set_wake 或外部汇报能唤醒你，不要自说自话。
- 回合尽量简短（少 token 即少成本）；调用 set_wake 时用 reason 简述本次结论。
- 你只通过 set_wake 工具控制自己的节奏，其余工具按需使用。
"#;

/// 管家状态（持久化字段落盘；in_flight 是内存态，崩溃后自然复位）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StewardState {
    /// 管家会话 id（BM_STEWARD_SESSION 指定；None = 未启用）。
    pub session_id: Option<String>,
    /// 下次唤醒时刻（ms 时间戳；0 = 未登记唤醒，保持静默）。
    pub next_wake_at_ms: i64,
    /// 上次回合完成时刻（ms 时间戳；调度器投喂内容据此报间隔）。
    pub last_wake_at_ms: i64,
    /// 上次登记的唤醒原因（调度器投喂时回看，指导管家决策）。
    pub last_reason: Option<String>,
    /// 调度回合进行中（防重叠投喂；内存态不落盘）。
    #[serde(skip)]
    pub in_flight: bool,
}

impl StewardState {
    fn clamp_wake(&mut self, after_seconds: i64, min_s: i64, max_s: i64) {
        let clamped = after_seconds.clamp(min_s, max_s);
        self.next_wake_at_ms = now_ms() + clamped * 1000;
    }
}

/// 管家状态存取（文件 + 内存锁；原子写 = tmp + rename）。
pub struct StewardStore {
    path: PathBuf,
    state: tokio::sync::Mutex<StewardState>,
    /// 治理夹区间（min_s, max_s）：启动时从 env 读一次。
    pacing: (i64, i64),
}

impl StewardStore {
    /// 从 `$BOENMIND_HOME/steward.json` 加载（缺失/损坏 → 默认空状态 + warn，
    /// 不阻断启动——管家是可选项）。pacing 从 env 读，仅治理参数可调。
    /// `BM_STEWARD_SESSION` 环境变量可强制指定管家会话（宿主配置；空 = 未启用）。
    pub fn load(app_dir: impl Into<PathBuf>) -> Self {
        let path = app_dir.into().join("steward.json");
        let mut state = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<StewardState>(&text) {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(event = "bm.steward_state_corrupt", error = %err, path = %path.display());
                    StewardState::default()
                }
            },
            Err(_) => StewardState::default(),
        };
        if let Ok(sid) = std::env::var("BM_STEWARD_SESSION")
            && let Some(sid) = sid.split_whitespace().next().filter(|s| !s.is_empty())
        {
            state.session_id = Some(sid.to_string());
        }
        let env_i64 = |name: &str, default: i64| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.trim().parse::<i64>().ok())
                .unwrap_or(default)
        };
        let min = env_i64("BM_STEWARD_PACING_MIN_S", DEFAULT_PACING_MIN_S).max(10);
        let max = env_i64("BM_STEWARD_PACING_MAX_S", DEFAULT_PACING_MAX_S).max(min);
        Self {
            path,
            state: tokio::sync::Mutex::new(state),
            pacing: (min, max),
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// 管家会话 id（None = 未启用）。
    pub async fn session_id(&self) -> Option<String> {
        self.state.lock().await.session_id.clone()
    }

    /// 当前状态快照（状态 API / 调试）。
    pub async fn snapshot(&self) -> StewardState {
        self.state.lock().await.clone()
    }

    /// 治理夹区间（测试用）。
    pub async fn pacing(&self) -> (i64, i64) {
        self.pacing
    }

    /// 是否正在调度回合中（防重叠投喂）。
    pub async fn in_flight(&self) -> bool {
        self.state.lock().await.in_flight
    }

    pub async fn set_in_flight(&self, value: bool) {
        self.state.lock().await.in_flight = value;
    }

    /// 管家回合内登记/更新唤醒（`set_wake` 工具执行侧）。
    /// 首次调用即把该会话设为管家（自举；此后拒绝其他会话）。
    ///
    /// - `after_seconds <= 0`：清除唤醒 → 静默（直到下次 set_wake/inject）；
    /// - 正值：提议值被治理层夹进 [pacing-min, pacing-max]（防热循环）。
    pub async fn set_wake(
        &self,
        session_id: &str,
        after_seconds: i64,
        reason: Option<&str>,
    ) -> Result<(), String> {
        let mut state = self.state.lock().await;
        match state.session_id.as_deref() {
            Some(existing) if existing != session_id => {
                return Err("该会话不是管家（BM_STEWARD_SESSION）".to_string());
            }
            None => state.session_id = Some(session_id.to_string()),
            _ => {}
        }
        if after_seconds <= 0 {
            state.next_wake_at_ms = 0;
            state.last_reason = reason.map(str::to_string);
        } else {
            state.clamp_wake(after_seconds, self.pacing.0, self.pacing.1);
            state.last_reason = reason.map(str::to_string);
        }
        self.persist(&state)?;
        Ok(())
    }

    /// OS 层汇报：登记可选的下次唤醒（同样夹区间；message 由调用方投喂）。
    pub async fn register_wake(&self, after_seconds: Option<i64>) -> Result<(), String> {
        let Some(after) = after_seconds else {
            return Ok(());
        };
        let mut state = self.state.lock().await;
        if state.session_id.is_none() {
            return Err("管家未启用（BM_STEWARD_SESSION 未设置）".to_string());
        }
        if after <= 0 {
            state.next_wake_at_ms = 0;
        } else {
            state.clamp_wake(after, self.pacing.0, self.pacing.1);
        }
        self.persist(&state)?;
        Ok(())
    }

    /// 调度器到期判断：`now >= next_wake_at` 且已登记。
    pub async fn should_wake(&self, now_ms: i64) -> bool {
        let state = self.state.lock().await;
        state.session_id.is_some() && state.next_wake_at_ms > 0 && now_ms >= state.next_wake_at_ms
    }

    /// 上一轮结束登记（更新时间锚点；不动 next_wake_at——管家回合内
    /// set_wake 写好的下次唤醒原样保留，回合失败/没写则保持 0 = 静默，
    /// 不会触发失败重试风暴）。
    pub async fn note_round_done(&self, now_ms: i64) {
        let mut state = self.state.lock().await;
        state.last_wake_at_ms = now_ms;
        let _ = self.persist(&state);
    }

    /// 上次登记原因（调度器投喂模板用）。
    pub async fn last_reason(&self) -> Option<String> {
        self.state.lock().await.last_reason.clone()
    }

    fn persist(&self, state: &StewardState) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(state)
            .map_err(|e| format!("序列化管家状态失败: {e}"))?;
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| format!("写入管家状态失败: {e}"))?;
        f.write_all(text.as_bytes())
            .and_then(|_| f.flush())
            .map_err(|e| format!("写入管家状态失败: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("落盘管家状态失败: {e}"))?;
        Ok(())
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `set_wake` 工具定义（只注册进管家会话的工具面——build_loop_agent 判定）。
pub fn set_wake_def() -> bm_loop::model::ToolDef {
    bm_loop::model::ToolDef::new(
        "set_wake",
        "登记/更新管家的下次自我唤醒时间（秒）。after_seconds<=0 表示清除唤醒进入静默。\
         治理层会把提议值夹进 [pacing-min, pacing-max]（默认 5 分钟~24 小时，env 可调）。\
         每个回合结束时都应调用一次：无事保持静默请登记较长间隔。",
        serde_json::json!({
            "type": "object",
            "properties": {
                "after_seconds": {
                    "type": "integer",
                    "description": "距下次唤醒的秒数（>=0；0 = 静默不唤醒）"
                },
                "reason": {
                    "type": "string",
                    "description": "本次回合结论 / 下次唤醒意图（调度器回看）"
                }
            },
            "required": ["after_seconds"]
        }),
    )
}

/// `set_wake` 工具执行（返回形状对齐内置工具 `{content:[{type:"text"}]}`）。
pub async fn execute_set_wake(
    store: &StewardStore,
    session_id: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let after = args
        .get("after_seconds")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| "set_wake 需要整数参数 after_seconds".to_string())?;
    let reason = args
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    store.set_wake(session_id, after, reason.as_deref()).await?;
    let (min, max) = store.pacing().await;
    let text = if after <= 0 {
        "已清除唤醒，管家进入静默（等待下次 set_wake 或外部汇报）".to_string()
    } else {
        let clamped = after.clamp(min, max);
        let note = if clamped == after {
            String::new()
        } else {
            format!("（提议 {after}s 已被治理层夹到 {clamped}s）")
        };
        format!("已登记 {clamped} 秒后唤醒{note}")
    };
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": text }],
        "details": null,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store(tmp: &std::path::Path) -> StewardStore {
        StewardStore {
            path: tmp.join("steward.json"),
            state: tokio::sync::Mutex::new(StewardState::default()),
            pacing: (300, 86_400),
        }
    }

    #[tokio::test]
    async fn first_set_wake_registers_steward_session() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        store.set_wake("s1", 600, Some("例行巡检")).await.unwrap();
        assert_eq!(store.session_id().await.as_deref(), Some("s1"));
        let snap = store.snapshot().await;
        assert!(snap.next_wake_at_ms > now_ms());
        assert_eq!(snap.last_reason.as_deref(), Some("例行巡检"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn non_steward_session_rejected() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        store.set_wake("s1", 600, None).await.unwrap();
        let err = store.set_wake("s2", 600, None).await.unwrap_err();
        assert!(err.contains("不是管家"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn wake_clamped_to_pacing_bounds() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        // 提议 1s → 夹到 min（300s）；提议 999999s → 夹到 max（86400s）
        store.set_wake("s1", 1, None).await.unwrap();
        let snap = store.snapshot().await;
        let until = snap.next_wake_at_ms - now_ms();
        assert!(
            (299_000..300_500).contains(&until),
            "应夹到 min: {until}"
        );
        store.set_wake("s1", 999_999, None).await.unwrap();
        let snap = store.snapshot().await;
        let until = snap.next_wake_at_ms - now_ms();
        assert!(
            (86_399_000..86_400_500).contains(&until),
            "应夹到 max: {until}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn zero_wake_clears_to_silent() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        store.set_wake("s1", 600, None).await.unwrap();
        store.set_wake("s1", 0, None).await.unwrap();
        assert!(!store.should_wake(now_ms() + 999_999).await);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn should_wake_only_when_due() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        assert!(!store.should_wake(now_ms()).await, "未登记不唤醒");
        store.set_wake("s1", 600, None).await.unwrap();
        assert!(!store.should_wake(now_ms() + 1).await, "未到期不唤醒");
        assert!(store.should_wake(now_ms() + 601_000).await, "到期应唤醒");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn persist_round_trip_survives_reload() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        {
            let store = test_store(&dir);
            store.set_wake("s1", 600, Some("记住我")).await.unwrap();
        }
        // 重新加载（文件为真源）
        let store = StewardStore::load(&dir);
        assert_eq!(store.session_id().await.as_deref(), Some("s1"));
        let snap = store.snapshot().await;
        assert!(snap.next_wake_at_ms > 0);
        assert_eq!(snap.last_reason.as_deref(), Some("记住我"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn corrupt_file_falls_back_to_default() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("steward.json"), "{not json").unwrap();
        let store = StewardStore::load(&dir);
        assert_eq!(store.session_id().await, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn note_round_done_keeps_next_wake() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        store.set_wake("s1", 600, None).await.unwrap();
        let next = store.snapshot().await.next_wake_at_ms;
        store.note_round_done(now_ms()).await;
        let snap = store.snapshot().await;
        assert_eq!(snap.next_wake_at_ms, next, "回合完成不得清掉管家新登记的唤醒");
        assert!(snap.last_wake_at_ms > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn register_wake_requires_steward() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        let err = store.register_wake(Some(600)).await.unwrap_err();
        assert!(err.contains("未启用"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn execute_set_wake_shapes_output() {
        let dir = std::env::temp_dir().join(format!("steward-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = test_store(&dir);
        let v = execute_set_wake(&store, "s1", &serde_json::json!({ "after_seconds": 60 }))
            .await
            .unwrap();
        assert!(v["content"][0]["text"].as_str().unwrap().contains("300"), "{v}");
        // 非管家会话 → 错误
        let err = execute_set_wake(&store, "s2", &serde_json::json!({ "after_seconds": 60 }))
            .await
            .unwrap_err();
        assert!(err.contains("不是管家"), "{err}");
        // 缺参数 → 错误
        let err = execute_set_wake(&store, "s1", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.contains("after_seconds"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }
}
