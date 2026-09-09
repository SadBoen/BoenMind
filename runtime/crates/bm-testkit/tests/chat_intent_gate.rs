//! issue #1:对话意图硬门控——触发输入短于阈值(intent_gate_min_chars)的
//! 回合禁用一切工具派发,如实回喂;默认 0=关(零行为变更)。
//! 这是 84d1bb0 软防线(工具纪律 System 注入)之上的确定性硬补充。

use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, ToolCallPayload, Usage};
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::{AgentSpec, InputTrust, SendInputParams, SessionCreateParams};
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::secret::MemSecretStore;
use bm_testkit::wait_terminal_handle;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

struct SingleCallConnector {
    requests: Mutex<Vec<InvokeRequest>>,
}

#[async_trait::async_trait]
impl ModelConnector for SingleCallConnector {
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
                tool_calls: vec![ToolCallPayload {
                    id: "call_1".into(),
                    name: "sys.probe".into(),
                    arguments: r#"{"q":"x"}"#.into(),
                }],
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
                content: "文字回应".into(),
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

async fn rig(
    dir: &std::path::Path,
    gate_min: u32,
    calls: Arc<Mutex<Vec<String>>>,
) -> RuntimeHandle {
    let connector = Arc::new(SingleCallConnector {
        requests: Mutex::new(Vec::new()),
    });
    let limits = bm_core::Limits {
        intent_gate_min_chars: gate_min,
        ..Default::default()
    };
    let calls_view = calls.clone();
    let mut capabilities = bm_providers::builtin::builtin_capability_set();
    capabilities.push((
        serde_json::from_value::<bm_contract::capability::CapabilityManifest>(serde_json::json!({
            "capability": "sys.probe", "provider": "sys", "version": "0.1.0",
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": true,
            "cancellable": true, "timeout_ms": 1000, "approval": "not-required"
        }))
        .expect("manifest 合法"),
        bm_core::broker::provider_fn(move |_| {
            calls_view.lock().expect("锁未中毒").push("probe".into());
            Ok(serde_json::json!({"did": "probe"}))
        }),
    ));
    RuntimeHandle::start(RuntimeConfig {
        capabilities,
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::new(limits),
        job_board: None,
        version: "0.1.0-intent-gate".into(),
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

async fn turn(handle: &RuntimeHandle, ids: &SeqIdGen, content: &str) -> bm_contract::wire::Receipt {
    let created = handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "intent-gate".into(),
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
        content: content.into(),
        model_override: None,
        workspace_override: None,
        input_trust: InputTrust::Trusted,
    };
    wait_terminal_handle(
        handle,
        &handle
            .send_input(ids.next_id("req"), input)
            .await
            .expect("回合发起")
            .operation_id,
    )
    .await
}

#[tokio::test]
async fn gate_on_blocks_tools_for_short_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handle = rig(dir.path(), 4, calls.clone()).await;
    let ids = SeqIdGen::new();
    let receipt = turn(&handle, &ids, "你好").await; // 2 字符 < 4
    assert_eq!(
        receipt.state,
        bm_contract::states::OperationState::Succeeded
    );
    assert!(
        calls.lock().expect("锁未中毒").is_empty(),
        "短输入回合工具必须被门控拦截"
    );
    // 回喂:第二轮请求中包含门控拦截的 Tool 消息
    // (直读 context-log 快照验证模型可见的拦截说明)
    let raw = std::fs::read_to_string(dir.path().join("context-log.jsonl")).unwrap();
    assert!(raw.contains("意图门控"), "拦截说明必须回喂模型: {raw}");
    handle.stop("done").await;
}

#[tokio::test]
async fn gate_off_by_default_keeps_tools_working() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handle = rig(dir.path(), 0, calls.clone()).await;
    let ids = SeqIdGen::new();
    let receipt = turn(&handle, &ids, "你好").await;
    assert_eq!(
        receipt.state,
        bm_contract::states::OperationState::Succeeded
    );
    assert_eq!(
        calls.lock().expect("锁未中毒").len(),
        1,
        "默认关(0)=工具照常派发"
    );
    handle.stop("done").await;
}
