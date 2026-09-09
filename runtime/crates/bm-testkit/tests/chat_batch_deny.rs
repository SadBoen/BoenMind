//! issue #26:同批 tool_calls 拒绝联动——用户驳回同批中一个调用后,余下
//! 调用按策略联动取消(如实回喂,不再各自独立执行);limits
//! tool_batch_cancel_on_deny=0 可回退独立执行(对照场景)。
//! 连接器脚本:第 1 次调用发起两个工具调用(sys.gated 需审批 + sys.free
//! 免审批),第 2 次给终稿。

use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, ToolCallPayload, Usage};
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::{AgentSpec, InputTrust, SendInputParams, SessionCreateParams};
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::secret::MemSecretStore;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// 脚本连接器:第 1 次调用发起双工具调用,第 2 次给终稿。
struct TwoCallConnector {
    requests: Mutex<Vec<InvokeRequest>>,
}

impl TwoCallConnector {}

#[async_trait::async_trait]
impl ModelConnector for TwoCallConnector {
    async fn invoke(&self, req: InvokeRequest, _cancel: CancellationToken) -> InvokeResponse {
        let model_id = req.model_id.clone();
        let n = {
            let mut r = self.requests.lock().expect("锁未中毒");
            r.push(req);
            r.len()
        };
        if n == 1 {
            InvokeResponse::Completed {
                content: String::new(),
                tool_calls: vec![
                    ToolCallPayload {
                        id: "call_1".into(),
                        name: "sys.gated".into(),
                        arguments: r#"{"x":1}"#.into(),
                    },
                    ToolCallPayload {
                        id: "call_2".into(),
                        name: "sys.free".into(),
                        arguments: r#"{"y":2}"#.into(),
                    },
                ],
                finish_reason: FinishReason::ToolCalls,
                usage: Usage {
                    tokens_in: 10,
                    tokens_out: 5,
                    ..Default::default()
                },
                model_id,
                latency_ms: 5,
                stream_interrupted: false,
            }
        } else {
            InvokeResponse::Completed {
                content: "终稿".into(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: Usage {
                    tokens_in: 20,
                    tokens_out: 5,
                    ..Default::default()
                },
                model_id,
                latency_ms: 5,
                stream_interrupted: false,
            }
        }
    }
    fn provider(&self) -> &'static str {
        "mock"
    }
}

fn manifest(
    cap: &str,
    approval: &str,
    effect: &str,
) -> bm_contract::capability::CapabilityManifest {
    serde_json::from_value(serde_json::json!({
        "capability": cap, "provider": "sys", "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": effect, "idempotent": true,
        "cancellable": true, "timeout_ms": 1000, "approval": approval
    }))
    .expect("manifest 合法")
}

/// 装配 rig:free/gated 两个提供者均记录被调用次数。
async fn rig(
    dir: &std::path::Path,
    batch_cancel: bool,
    free_calls: Arc<Mutex<Vec<String>>>,
    gated_calls: Arc<Mutex<Vec<String>>>,
) -> RuntimeHandle {
    let connector = Arc::new(TwoCallConnector {
        requests: Mutex::new(Vec::new()),
    });
    let limits = bm_core::Limits {
        tool_batch_cancel_on_deny: u32::from(batch_cancel),
        ..Default::default()
    };
    let free_view = free_calls.clone();
    let gated_view = gated_calls.clone();
    let mut capabilities = bm_providers::builtin::builtin_capability_set();
    capabilities.extend(vec![
        (
            manifest("sys.free", "not-required", "read-only"),
            bm_core::broker::provider_fn(move |_| {
                free_view.lock().expect("锁未中毒").push("free".into());
                Ok(serde_json::json!({"did": "free"}))
            }),
        ),
        (
            manifest("sys.gated", "required", "reversible-command"),
            bm_core::broker::provider_fn(move |_| {
                gated_view.lock().expect("锁未中毒").push("gated".into());
                Ok(serde_json::json!({"did": "gated"}))
            }),
        ),
    ]);
    RuntimeHandle::start(RuntimeConfig {
        capabilities,
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::new(limits),
        job_board: None,
        version: "0.1.0-batch-deny".into(),
        data_dir: Some(dir.to_path_buf()),
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:mock.model", "sk-test-123456")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await
}

#[tokio::test]
async fn batch_deny_cancels_remaining_calls_in_same_batch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let free_calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let gated_calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handle = rig(dir.path(), true, free_calls.clone(), gated_calls.clone()).await;
    let ids = SeqIdGen::new();
    let created = handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "batch-deny".into(),
                    model_chain: vec!["mock.model".into()],
                    budget: None,
                    system_prompt: None,
                    workspace_id: None,
                    allowed_tools: None,
                },
            },
        )
        .await
        .expect("建会话");
    let input = SendInputParams {
        session_id: created.session_id.clone(),
        agent_id: created.agent_id.clone(),
        content: "发起双调用".into(),
        model_override: None,
        workspace_override: None,
        input_trust: InputTrust::Trusted,
    };
    // 发起回合(不等待终态:中途要驳回审批)
    let _ = handle
        .send_input(ids.next_id("req"), input)
        .await
        .expect("回合发起");

    // 轮询等待审批单出现(gated 需审批)
    let mut approval_id = None;
    for dbg in 0..50 {
        let list = handle
            .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
            .await
            .expect("审批列表");
        if dbg == 3 {
            eprintln!("[dbg] approvals={list}");
            let evs = handle.events_all().await;
            for e in evs.iter().rev().take(8) {
                eprintln!("[dbg] ev {} {}", e.event_type.as_str(), e.payload);
            }
        }
        if let Some(item) = list["approvals"].as_array().and_then(|a| a.first()) {
            approval_id = Some(item["approval_id"].as_str().unwrap().to_string());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    let approval_id = approval_id.expect("审批单必须出现");
    handle
        .approval_respond(
            ids.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&approval_id).unwrap(),
                decision: "deny".into(),
                scope: None,
            },
        )
        .await
        .expect("驳回");

    // 等回合终态(驳回后联动取消 → 回喂 → 终稿)
    for _ in 0..200 {
        let evs = handle.events_all().await;
        if evs.iter().any(|e| {
            e.event_type == bm_contract::events::EventType::OperationStateChanged
                && e.payload["to"] == serde_json::json!("succeeded")
        }) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }

    // 联动断言:gated 未执行(停在审批),free 被联动取消,均未触达提供者
    assert!(
        gated_calls.lock().expect("锁未中毒").is_empty(),
        "被驳回的调用不得执行"
    );
    assert!(
        free_calls.lock().expect("锁未中毒").is_empty(),
        "同批余下调用必须被联动取消,不得独立执行"
    );
}

#[tokio::test]
async fn batch_deny_off_keeps_independent_execution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let free_calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let gated_calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handle = rig(dir.path(), false, free_calls.clone(), gated_calls.clone()).await;
    let ids = SeqIdGen::new();
    let created = handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "batch-deny-off".into(),
                    model_chain: vec!["mock.model".into()],
                    budget: None,
                    system_prompt: None,
                    workspace_id: None,
                    allowed_tools: None,
                },
            },
        )
        .await
        .expect("建会话");
    let input = SendInputParams {
        session_id: created.session_id.clone(),
        agent_id: created.agent_id.clone(),
        content: "发起双调用".into(),
        model_override: None,
        workspace_override: None,
        input_trust: InputTrust::Trusted,
    };
    let _ = handle.send_input(ids.next_id("req"), input).await;

    // 等审批单出现后驳回
    let mut approval_id = None;
    for _ in 0..50 {
        let list = handle
            .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
            .await
            .expect("审批列表");
        if let Some(item) = list["approvals"].as_array().and_then(|a| a.first()) {
            approval_id = Some(item["approval_id"].as_str().unwrap().to_string());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    handle
        .approval_respond(
            ids.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(approval_id.unwrap()).unwrap(),
                decision: "deny".into(),
                scope: None,
            },
        )
        .await
        .expect("驳回");

    // 等回合终态
    for _ in 0..200 {
        let evs = handle.events_all().await;
        if evs.iter().any(|e| {
            e.event_type == bm_contract::events::EventType::OperationStateChanged
                && e.payload["to"] == serde_json::json!("succeeded")
        }) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }

    // 回退口径:free 独立执行不受联动
    assert!(
        !free_calls.lock().expect("锁未中毒").is_empty(),
        "开关关闭时 free 必须独立执行"
    );
}
