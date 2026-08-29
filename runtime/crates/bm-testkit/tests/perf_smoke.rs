//! 性能定标(m0/perf-baseline P-01..P-08 的 M1 子集)。
//! 默认 #[ignore];回填时 `cargo test --release -p bm-testkit --test perf_smoke -- --ignored --nocapture`。
//! 口径:mock 模型无外网;延迟类先丢弃前 10 次预热(m0 定标 §2)。

use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::{AgentSpec, GetOperationParams, SessionCreateParams};
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use std::sync::Arc;
use std::time::Instant;

fn percentile(sorted: &mut [u128], p: f64) -> u128 {
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

async fn start_runtime(
    connector: Arc<dyn ModelConnector>,
    data_dir: Option<std::path::PathBuf>,
) -> RuntimeHandle {
    RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-m1".into(),
        data_dir,
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with(
            &bm_core::runtime::default_secret_ref(bm_testkit_replay::MODEL_A),
            "sk-demo-zhipu-secret-value-001",
        )),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await
}

fn session_params() -> SessionCreateParams {
    SessionCreateParams {
        agent: AgentSpec {
            name: "assistant".into(),
            model_chain: vec![bm_testkit_replay::MODEL_A.into()],
            budget: Some(bm_contract::budget::Budget {
                max_tokens: 10_000_000,
                max_turns: 1_000,
                extra: Default::default(),
            }),
        },
    }
}

/// P-01 冷启动到 ready:RuntimeHandle::start 完成(事件 1 已发,可受理
/// session.create)的墙钟时间。50 次取 p95。
#[tokio::test]
#[ignore = "性能定标,手动运行"]
async fn p01_cold_start_to_ready() {
    let mut samples = Vec::new();
    for _ in 0..50 {
        let connector: Arc<dyn ModelConnector> =
            Arc::new(MockConnector::new(vec![Step::ok("x", 1, 1)]));
        let t0 = Instant::now();
        let handle = start_runtime(connector, None).await;
        let elapsed = t0.elapsed();
        handle.stop("perf").await;
        samples.push(elapsed.as_micros());
    }
    let p95 = percentile(&mut samples, 0.95) as f64 / 1000.0;
    let p50 = percentile(&mut samples, 0.50) as f64 / 1000.0;
    println!("P-01 cold_start_ms: p50={p50:.3} p95={p95:.3}");
}

/// P-03 单回合延迟:send_input → 执行收据落定;mock 固定注入 200ms。
/// N=110(丢弃前 10 预热),取 p50/p99。
#[tokio::test]
#[ignore = "性能定标,手动运行"]
async fn p03_turn_latency_inject_200ms() {
    let dir = tempfile::tempdir().expect("临时目录");
    let connector: Arc<dyn ModelConnector> =
        Arc::new(MockConnector::repeating(Step::ok_after("答", 200)));
    let handle = start_runtime(connector, Some(dir.path().to_path_buf())).await;

    let created = handle
        .session_create(
            bm_contract::ids::IdGen::next_id(&SeqIdGen::new(), "req"),
            session_params(),
        )
        .await
        .expect("会话创建");

    let mut samples = Vec::new();
    let ids = Arc::new(SeqIdGen::new());
    for i in 0..110 {
        let t0 = Instant::now();
        let receipt = handle
            .send_input(
                IdGen::next_id(ids.as_ref(), "req"),
                bm_testkit_replay::input(&created.session_id, &created.agent_id, "问题"),
            )
            .await
            .expect("回合发起");
        loop {
            let r = handle
                .operations_get(GetOperationParams {
                    operation_id: receipt.operation_id.clone(),
                })
                .await
                .expect("查询");
            if r.state.is_terminal() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        if i >= 10 {
            samples.push(t0.elapsed().as_micros());
        }
    }
    handle.stop("perf").await;
    let p50 = percentile(&mut samples, 0.50) as f64 / 1000.0;
    let p99 = percentile(&mut samples, 0.99) as f64 / 1000.0;
    println!("P-03 turn_latency_ms(inject 200ms): p50={p50:.1} p99={p99:.1} (N=100)");
}

/// P-08 脱敏扫描开销:每条日志记录的平均耗时(含扫描),N=10 万条。
#[tokio::test]
#[ignore = "性能定标,手动运行"]
async fn p08_redaction_scan_overhead() {
    let log = bm_core::exec_log::ExecutionLog::new(None);
    log.register_scan_value("sk-demo-zhipu-secret-value-001");
    let ids = Arc::new(SeqIdGen::new());
    let sess = IdGen::next_id(ids.as_ref(), "sess");
    let agent = IdGen::next_id(ids.as_ref(), "agent");
    let op = IdGen::next_id(ids.as_ref(), "op");

    let n = 100_000u64;
    let t0 = Instant::now();
    for i in 0..n {
        log.record(log_record_of(i, sess.clone(), agent.clone(), op.clone()));
    }
    let per_entry_us = t0.elapsed().as_micros() as f64 / n as f64;
    println!("P-08 redaction_scan_us_per_entry: {per_entry_us:.3} (N={n})");
}

fn log_record_of(
    _i: u64,
    sess: bm_contract::ids::BmId,
    agent: bm_contract::ids::BmId,
    op: bm_contract::ids::BmId,
) -> bm_core::exec_log::LogRecord {
    bm_core::exec_log::LogRecord {
        kind: bm_contract::exec_log::LogKind::ModelInvocation,
        session_id: sess,
        agent_id: agent,
        operation_id: op,
        request_id: None,
        agent_state: "waiting_model".into(),
        detail: serde_json::json!({
            "model_id": "zhipu.glm-4-flash",
            "attempt": 1,
            "usage": {"tokens_in": 412, "tokens_out": 58},
            "latency_ms": 1873,
            "stream_interrupted": false,
            "note": format!("样例条目 {_i},携带一段较长的文本以模拟真实负载场景的序列化与扫描开销。"),
        }),
        ts: bm_contract::timestamp::now(),
    }
}

/// 装配辅助(与测试装配同构)。
mod bm_testkit_replay {
    pub const MODEL_A: &str = "zhipu.glm-4-flash";

    pub fn input(
        session_id: &bm_contract::ids::BmId,
        agent_id: &bm_contract::ids::BmId,
        content: &str,
    ) -> bm_contract::wire::SendInputParams {
        bm_contract::wire::SendInputParams {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            content: content.into(),
            input_trust: bm_contract::wire::InputTrust::Trusted,
        }
    }
}
