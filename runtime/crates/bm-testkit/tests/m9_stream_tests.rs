//! M9-S2 模型真流式:t140 默认关(零回归)/ t141 开启后增量序列与聚合一致 /
//! t142 流中取消(已发增量保留,终态 cancelled)/ t144 实网连通(门控)。

use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, Usage};
use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{CancelParams, SendInputParams, SessionCreateParams};
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// 测试连接器:按块延迟吐增量(默认 invoke 退化为整段,供 t140 对照)。
struct ChunkedConnector {
    chunks: Vec<String>,
    delay_ms: u64,
}

#[async_trait::async_trait]
impl ModelConnector for ChunkedConnector {
    fn provider(&self) -> &'static str {
        "chunked-test"
    }

    async fn invoke(&self, _req: InvokeRequest, _cancel: CancellationToken) -> InvokeResponse {
        InvokeResponse::Completed {
            tool_calls: Vec::new(),
            content: self.chunks.concat(),
            finish_reason: FinishReason::Stop,
            usage: Usage {
                tokens_in: 1,
                tokens_out: 2,
            },
            model_id: "m1".into(),
            latency_ms: 0,
            stream_interrupted: false,
        }
    }

    async fn invoke_stream(
        &self,
        _req: InvokeRequest,
        cancel: CancellationToken,
        mut on_delta: Box<dyn for<'a> FnMut(&'a str) + Send + 'static>,
    ) -> InvokeResponse {
        for c in &self.chunks {
            tokio::select! {
                _ = cancel.cancelled() => {
                    return InvokeResponse::Failed {
                        error_code: ErrorCode::Cancelled,
                        retryable: false,
                        attempt: 1,
                        detail_ref: None,
                    };
                }
                _ = tokio::time::sleep(Duration::from_millis(self.delay_ms)) => {
                    (on_delta)(c.as_str());
                }
            }
        }
        InvokeResponse::Completed {
            tool_calls: Vec::new(),
            content: self.chunks.concat(),
            finish_reason: FinishReason::Stop,
            usage: Usage {
                tokens_in: 1,
                tokens_out: 2,
            },
            model_id: "m1".into(),
            latency_ms: 0,
            stream_interrupted: false,
        }
    }
}

async fn rig_streaming(
    on: bool,
    chunks: Vec<String>,
    delay_ms: u64,
) -> (RuntimeHandle, Arc<SeqIdGen>) {
    let ids = Arc::new(SeqIdGen::new());
    let config = RuntimeConfig {
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-m9".into(),
        data_dir: None,
        store: None,
        connector: Arc::new(ChunkedConnector { chunks, delay_ms }),
        secret_store: Arc::new(MemSecretStore::with("secret:model.m1", "sk")),
        id_gen: ids.clone(),
        clock: Arc::new(bm_core::clock::MockClock::at_ms(1_788_000_000_000)),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: on,
    };
    (RuntimeHandle::start(config).await, ids)
}

struct TurnCtx {
    session_id: bm_contract::ids::BmId,
    agent_id: bm_contract::ids::BmId,
    operation_id: bm_contract::ids::BmId,
}

async fn one_turn(handle: &RuntimeHandle, ids: &Arc<SeqIdGen>, model: &str) -> TurnCtx {
    let created = handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: agent_spec(model),
            },
        )
        .await
        .expect("会话");
    let sent = handle
        .send_input(
            ids.next_id("req"),
            SendInputParams {
                session_id: created.session_id.clone(),
                agent_id: created.agent_id.clone(),
                content: "你好".into(),
                model_override: None,
                input_trust: bm_contract::wire::InputTrust::Trusted,
            },
        )
        .await
        .expect("回合");
    TurnCtx {
        session_id: created.session_id,
        agent_id: created.agent_id,
        operation_id: sent.operation_id,
    }
}

// AgentSpec 构造的小封装(字段随合同版本演进只改这里)。
fn agent_spec(model: &str) -> bm_contract::wire::AgentSpec {
    bm_contract::wire::AgentSpec {
        system_prompt: None,
        name: "tester".into(),
        model_chain: vec![model.to_string()],
        budget: None,
    }
}

async fn wait_done(handle: &RuntimeHandle, op: &bm_contract::ids::BmId) -> OperationState {
    for _ in 0..300 {
        let r = handle
            .operations_get(get_op_params(op.clone()))
            .await
            .expect("收据");
        if matches!(
            r.state,
            OperationState::Succeeded | OperationState::Failed | OperationState::Cancelled
        ) {
            return r.state;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("回合 300 轮内未终态");
}

fn get_op_params(op: bm_contract::ids::BmId) -> bm_contract::wire::GetOperationParams {
    bm_contract::wire::GetOperationParams { operation_id: op }
}

async fn deltas_of(handle: &RuntimeHandle) -> Vec<(u64, String)> {
    let events = handle.events_all().await;
    events
        .iter()
        .filter(|e| e.event_type == EventType::ModelContentDelta)
        .map(|e| {
            (
                e.payload["index"].as_u64().unwrap(),
                e.payload["delta"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// t140:默认开关关闭 → 回合无任何 delta 事件(既有路径零回归)。
#[tokio::test]
async fn t140_streaming_off_no_delta_events() {
    let (handle, ids) = rig_streaming(false, vec!["你".into(), "好".into()], 5).await;
    let model = "m1";
    let ctx = one_turn(&handle, &ids, model).await;
    assert_eq!(
        wait_done(&handle, &ctx.operation_id).await,
        OperationState::Succeeded
    );
    assert!(deltas_of(&handle).await.is_empty(), "关态不得有 delta 事件");
}

/// t141:开启后 delta index 连续(0 起),聚合 == completed 全量 content。
#[tokio::test]
async fn t141_streaming_delta_sequence_matches_completed() {
    let (handle, ids) = rig_streaming(true, vec!["你".into(), "好".into(), "世界".into()], 5).await;
    let model = "m1";
    let ctx = one_turn(&handle, &ids, model).await;
    assert_eq!(
        wait_done(&handle, &ctx.operation_id).await,
        OperationState::Succeeded
    );
    let deltas = deltas_of(&handle).await;
    assert_eq!(
        deltas.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let joined: String = deltas.iter().map(|(_, d)| d.as_str()).collect();
    assert_eq!(joined, "你好世界");
    let events = handle.events_all().await;
    let completed = events
        .iter()
        .find(|e| e.event_type == EventType::ModelInvocationCompleted)
        .expect("completed 事件");
    assert_eq!(completed.payload["content"], json!("你好世界"));
}

/// t142:流中取消 → 已发增量保留,终态 cancelled,无 completed 事件。
#[tokio::test]
async fn t142_cancel_mid_stream_keeps_deltas_no_completed() {
    let (handle, ids) = rig_streaming(true, vec!["a".into(), "b".into(), "c".into()], 60).await;
    let model = "m1";
    let ctx = one_turn(&handle, &ids, model).await;
    let op = ctx.operation_id.clone();
    // 等第一个 delta 落事件后取消
    for _ in 0..100 {
        if !deltas_of(&handle).await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let _ = handle
        .agent_cancel(CancelParams {
            session_id: ctx.session_id,
            agent_id: ctx.agent_id,
            operation_id: op.clone(),
        })
        .await;
    let state = wait_done(&handle, &op).await;
    assert_eq!(state, OperationState::Cancelled);
    assert!(!deltas_of(&handle).await.is_empty(), "已发增量不得回滚");
    let events = handle.events_all().await;
    assert!(
        !events
            .iter()
            .any(|e| e.event_type == EventType::ModelInvocationCompleted),
        "取消后不得有 completed 事件"
    );
}

/// t144:实网流式连通(门控:BOEN_LIVE=1 + 三变量;密钥零入仓)。
#[tokio::test]
#[ignore = "实网流式:BOEN_LIVE=1 且三变量齐备(密钥零入仓)"]
async fn t144_live_streaming_one_turn() {
    if std::env::var("BOEN_LIVE").as_deref() != Ok("1") {
        eprintln!("跳过:BOEN_LIVE 未设");
        return;
    }
    let base = std::env::var("BOEN_LIVE_BASE_URL").expect("BOEN_LIVE_BASE_URL");
    let model = std::env::var("BOEN_LIVE_MODEL").expect("BOEN_LIVE_MODEL");
    let key = std::env::var("BOEN_LIVE_API_KEY").expect("BOEN_LIVE_API_KEY");

    let ids = Arc::new(SeqIdGen::new());
    let secret_ref = bm_core::runtime::default_secret_ref(&model);
    let connector: Arc<dyn ModelConnector> =
        Arc::new(bm_providers::openai_http::OpenAiConnector::new(
            base,
            Arc::new(MemSecretStore::with(&secret_ref, &key)),
        ));
    let config = RuntimeConfig {
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-m9".into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with(&secret_ref, &key)),
        id_gen: ids.clone(),
        clock: Arc::new(bm_core::clock::SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: true,
    };
    let handle = RuntimeHandle::start(config).await;
    let ctx = one_turn(&handle, &ids, &model).await;
    // 实网窗:真实模型延迟可达数十秒,给 60s(等 300×10ms 的 mock 窗不够)
    let mut state = OperationState::Failed;
    for _ in 0..6000 {
        let r = handle
            .operations_get(get_op_params(ctx.operation_id.clone()))
            .await
            .expect("收据");
        if matches!(
            r.state,
            OperationState::Succeeded | OperationState::Failed | OperationState::Cancelled
        ) {
            state = r.state;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if state != OperationState::Succeeded {
        let r = handle
            .operations_get(get_op_params(ctx.operation_id.clone()))
            .await
            .expect("收据");
        panic!("实网回合未成功:{r:?}");
    }
    let deltas = deltas_of(&handle).await;
    assert!(!deltas.is_empty(), "实网流式应产生增量");
    let joined: String = deltas.iter().map(|(_, d)| d.as_str()).collect();
    let events = handle.events_all().await;
    let completed = events
        .iter()
        .find(|e| e.event_type == EventType::ModelInvocationCompleted)
        .expect("completed")
        .payload
        .clone();
    assert_eq!(joined, completed["content"].as_str().unwrap_or_default());
    eprintln!(
        "t144 实网流式:delta 块数={} 聚合字节数={}",
        deltas.len(),
        joined.len()
    );
}
