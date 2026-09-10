//! 测试装配(TestRig)与黄金轨迹期望描述。

use bm_contract::budget::Budget;
use bm_contract::events::EventEnvelope;
use bm_contract::ids::{BmId, IdGen, SeqIdGen};
use bm_contract::wire::{AgentSpec, SendInputParams, SessionCreateParams};
use bm_core::CoreResult;
use bm_core::clock::MockClock;
use bm_core::ports::SecretStore;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use std::sync::Arc;
use std::time::Instant;

pub const MODEL_A: &str = "zhipu.glm-4-flash";
pub const MODEL_B: &str = "openai.gpt-4o-mini";
pub const STANDARD_BUDGET_TOKENS: u64 = 50_000;
pub const STANDARD_BUDGET_TURNS: u32 = 10;

/// 台架限制:黄金轨迹录制时的有界口径(ADR-0028 前的旧默认值)。
/// 生产默认已归零(0=不限制,见 ADR-0028);台架必须钉住有界值——
/// 轨迹场景与脚本步数/降级链长耦合(如 GT-B 两次超时后按链长收束),
/// 吃生产默认会让脚本越界步返回 Internal 而非 Timeout。
pub fn test_limits() -> bm_core::LimitsCell {
    bm_core::LimitsCell::new(bm_core::Limits {
        model_call_timeout_secs: 120,
        model_max_attempts: 3,
        tool_wait_ms: 60_000,
        approval_wait_ms: 300_000,
        tool_rounds_max: 64,
        loop_breaker_consecutive: 5,
        history_max_turns: 20,
        history_max_chars: 24_000,
        stream_hard_cap_ms: 900_000,
        nonstream_wait_ms: 180_000,
        autorun_default_max_turns: 6,
        ..bm_core::Limits::default()
    })
}

pub struct TestRig {
    pub handle: RuntimeHandle,
    pub ids: Arc<SeqIdGen>,
    pub clock: Arc<MockClock>,
    pub connector: Arc<MockConnector>,
    pub secrets: Arc<MemSecretStore>,
    _dir: Option<tempfile::TempDir>,
    pub data_dir: Option<std::path::PathBuf>,
}

impl TestRig {
    /// GT 场景 A 同款装配:双模型链 + 标准 50000/10 预算 + 凭据已入库。
    pub async fn standard(script: Vec<Step>) -> Self {
        rig(script, true, Vec::new(), None).await
    }

    /// M7:带额外能力(如 MCP stub 集)与异步执行器的标准装配。
    pub async fn standard_with(
        script: Vec<Step>,
        extra_caps: Vec<(
            bm_contract::capability::CapabilityManifest,
            Arc<dyn bm_core::registry::CapabilityProvider>,
        )>,
        executor: Option<Arc<dyn bm_core::ports::AsyncCapabilityExecutor>>,
    ) -> Self {
        rig(script, true, extra_caps, executor).await
    }

    pub fn budget(&self) -> Option<Budget> {
        Some(Budget {
            max_tokens: STANDARD_BUDGET_TOKENS,
            max_turns: STANDARD_BUDGET_TURNS,
            extra: Default::default(),
        })
    }

    pub async fn create_session(&self) -> CoreResult<(BmId, BmId)> {
        self.create_session_budget(STANDARD_BUDGET_TOKENS, STANDARD_BUDGET_TURNS)
            .await
    }

    pub async fn create_session_budget(
        &self,
        max_tokens: u64,
        max_turns: u32,
    ) -> CoreResult<(BmId, BmId)> {
        let req = self.ids.next_id("req");
        let res = self
            .handle
            .session_create(
                req,
                SessionCreateParams {
                    agent: AgentSpec {
                        name: "assistant".into(),
                        model_chain: vec![MODEL_A.into(), MODEL_B.into()],
                        budget: Some(Budget {
                            max_tokens,
                            max_turns,
                            extra: Default::default(),
                        }),
                        system_prompt: None,
                        workspace_id: None,
                        allowed_tools: None,
                    },
                },
            )
            .await?;
        Ok((res.session_id, res.agent_id))
    }

    pub fn input(&self, session: &BmId, agent: &BmId, content: &str) -> SendInputParams {
        SendInputParams {
            session_id: session.clone(),
            agent_id: agent.clone(),
            content: content.into(),
            model_override: None,
            workspace_override: None,
            input_trust: bm_contract::wire::InputTrust::Trusted,
        }
    }

    pub async fn send(
        &self,
        session: &BmId,
        agent: &BmId,
        content: &str,
    ) -> CoreResult<bm_contract::wire::Receipt> {
        let req = self.ids.next_id("req");
        let params = self.input(session, agent, content);
        self.handle.send_input(req, params).await
    }

    /// 全量事件流(含 runtime.* 等无会话关联事件;诊断端口)。
    pub async fn all_events(&self) -> Vec<EventEnvelope> {
        self.handle.events_all().await
    }

    pub async fn stop(self) {
        self.handle.stop("test_done").await;
    }

    /// 轮询等待指定操作落定为终态收据
    pub async fn wait_terminal(&self, op: &BmId) -> bm_contract::wire::Receipt {
        wait_terminal_handle(&self.handle, op).await
    }
}

/// 轮询等待指定操作落定为终态收据。
/// `timeout` = None 不设限(沿用 `wait_terminal_handle` 旧语义);
/// `interval` 为轮询间隔,按场景时长取舍。
pub async fn wait_terminal_within(
    handle: &RuntimeHandle,
    op: &BmId,
    timeout: Option<std::time::Duration>,
    interval: std::time::Duration,
) -> bm_contract::wire::Receipt {
    let deadline = timeout.map(|t| tokio::time::Instant::now() + t);
    loop {
        let r = handle
            .operations_get(bm_contract::wire::GetOperationParams {
                operation_id: op.clone(),
            })
            .await
            .expect("收据查询");
        if r.state.is_terminal() {
            return r;
        }
        if let Some(d) = deadline {
            assert!(tokio::time::Instant::now() < d, "操作未在时限内落定");
        }
        tokio::time::sleep(interval).await;
    }
}

/// 轮询等待 RuntimeHandle 的指定操作落定为终态收据(不设时限)。
pub async fn wait_terminal_handle(handle: &RuntimeHandle, op: &BmId) -> bm_contract::wire::Receipt {
    wait_terminal_within(handle, op, None, std::time::Duration::from_millis(5)).await
}

/// 装配核心:rig(临时目录自持有)与 rig_on(外部目录)共用。
async fn start_rig(
    dir: Option<std::path::PathBuf>,
    dir_guard: Option<tempfile::TempDir>,
    script: Vec<Step>,
    extra_caps: Vec<(
        bm_contract::capability::CapabilityManifest,
        Arc<dyn bm_core::registry::CapabilityProvider>,
    )>,
    executor: Option<Arc<dyn bm_core::ports::AsyncCapabilityExecutor>>,
) -> TestRig {
    let connector = Arc::new(MockConnector::new(script));
    let secrets = Arc::new(MemSecretStore::with(
        &bm_core::runtime::default_secret_ref(MODEL_A),
        "sk-demo-zhipu-secret-value-001",
    ));
    secrets
        .put(
            &bm_core::runtime::default_secret_ref(MODEL_B),
            "sk-demo-openai-secret-value-002",
        )
        .expect("内存存储写入成功");

    let clock = Arc::new(MockClock::at_ms(1_787_952_900_098));
    let ids = Arc::new(SeqIdGen::new());

    // 有落盘目录即启用写穿持久层(所有标准测试装配自动走 M2 路径)
    let store: Option<Arc<dyn bm_persist::EventStore>> = match &dir {
        Some(d) => Some(Arc::new(
            bm_persist::PersistStore::open(d).expect("打开持久层"),
        )),
        None => None,
    };

    let config = RuntimeConfig {
        // M7 S1:最小模型能力装配 + 调用方额外能力(MCP stub 等)
        capabilities: [vec![bm_providers::builtin::model_invoke_cap()], extra_caps].concat(),
        async_executor: executor,
        model_streaming: false,
        limits: test_limits(),
        job_board: None,
        version: "0.1.0-m1".into(),
        data_dir: dir.clone(),
        store,
        connector: connector.clone(),
        secret_store: secrets.clone(),
        id_gen: ids.clone(),
        clock: clock.clone(),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    let handle = RuntimeHandle::start(config).await;

    TestRig {
        handle,
        ids,
        clock,
        connector,
        secrets,
        _dir: dir_guard,
        data_dir: dir,
    }
}

/// 在给定目录上启动 Runtime(不清理目录):跨进程恢复测试用。
pub async fn rig_on(dir: &std::path::Path, script: Vec<Step>) -> TestRig {
    start_rig(Some(dir.to_path_buf()), None, script, Vec::new(), None).await
}

/// 组装一台确定性 Runtime(固定时钟起点、确定性 ID、脚本化模型)。
pub async fn rig(
    script: Vec<Step>,
    with_dir: bool,
    extra_caps: Vec<(
        bm_contract::capability::CapabilityManifest,
        Arc<dyn bm_core::registry::CapabilityProvider>,
    )>,
    executor: Option<Arc<dyn bm_core::ports::AsyncCapabilityExecutor>>,
) -> TestRig {
    let dir_guard = if with_dir {
        Some(tempfile::tempdir().expect("临时目录可建"))
    } else {
        None
    };
    let dir = dir_guard.as_ref().map(|d| d.path().to_path_buf());
    start_rig(dir, dir_guard, script, extra_caps, executor).await
}

/// 轮询直到条件成立或超时(测试辅助,默认 5s)。
pub async fn eventually<F, T>(mut f: F) -> T
where
    F: FnMut() -> Option<T>,
{
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "eventually 超时");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// 期望中的一个 payload 值。
#[derive(Clone)]
pub enum PVal {
    Id(String),
    Str(String),
    Num(i64),
    Float(f64),
    Bool(bool),
    Raw(serde_json::Value),
    Any,
}

pub fn id(s: impl Into<String>) -> PVal {
    PVal::Id(s.into())
}

/// 同一步骤重复 n 次(多回合脚本)。
pub fn repeat(step: Step, n: usize) -> Vec<Step> {
    vec![step; n]
}

/// 排序后取分位值(性能冒烟/持久化基准共用)。
pub fn percentile(sorted: &mut [u128], p: f64) -> u128 {
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

/// 期望事件:类型 + 指定字段的值断言(未列字段不检查)。
pub struct Expected {
    pub ty: bm_contract::events::EventType,
    pub payload: Vec<(&'static str, PVal)>,
}

pub fn pval_matches(spec: &PVal, v: &serde_json::Value) -> bool {
    match spec {
        PVal::Any => true,
        PVal::Id(s) | PVal::Str(s) => v.as_str() == Some(s.as_str()),
        PVal::Num(n) => v.as_i64() == Some(*n),
        PVal::Float(f) => v.as_f64().map(|x| (x - f).abs() < 1e-9).unwrap_or(false),
        PVal::Bool(b) => v.as_bool() == Some(*b),
        PVal::Raw(expected) => v == expected,
    }
}

/// 对照期望序列逐条校验一段事件流(长度必须相等)。
pub fn assert_matches(events: &[EventEnvelope], expected: &[Expected]) {
    assert_eq!(
        events.len(),
        expected.len(),
        "事件数不符:\n实际: {:?}\n期望: {:?}",
        events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>(),
        expected.iter().map(|e| e.ty.as_str()).collect::<Vec<_>>(),
    );
    for (e, x) in events.iter().zip(expected) {
        assert_eq!(e.event_type, x.ty, "第 {} 个事件类型不符", e.event_seq);
        for (key, spec) in &x.payload {
            let v = e
                .payload
                .get(*key)
                .unwrap_or_else(|| panic!("事件 {} 缺 payload 键 {}", e.event_type, key));
            assert!(
                pval_matches(spec, v),
                "事件 {} 字段 {} 不匹配: {:?}",
                e.event_type,
                key,
                v
            );
        }
    }
}
