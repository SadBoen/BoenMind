//! ADR-0030 审批裁决后台化集成测试:会话权限模式(yolo)服务端化。
//! - yolo 会话的审批类工具调用由服务端在裁决点自动批准(scope=once,
//!   审计 source=mode_auto),回合秒级完成,前端零弹卡;
//! - ask 会话维持人工裁决,批准后审计 source=user——人工与机器可区分;
//! - 模式变更落 session.mode.changed 事实事件并经物化投影持久,重启装载。

use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, ToolCallPayload, Usage};
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{AgentSpec, InputTrust, SendInputParams, SessionCreateParams};
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{RuntimeConfig, RuntimeHandle};
use bm_persist::EventStore as _;
use bm_providers::secret::MemSecretStore;
use bm_testkit::wait_terminal_handle;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// 脚本连接器:第 1 次调用发起 sys.gated(需审批)工具调用,第 2 次给终稿。
struct GatedConnector {
    requests: Mutex<Vec<InvokeRequest>>,
}

#[async_trait::async_trait]
impl ModelConnector for GatedConnector {
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
                    name: "sys.gated".into(),
                    arguments: r#"{"x":1}"#.into(),
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

fn gated_manifest() -> bm_contract::capability::CapabilityManifest {
    serde_json::from_value(serde_json::json!({
        "capability": "sys.gated", "provider": "sys", "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": "reversible-command", "idempotent": true,
        "cancellable": true, "timeout_ms": 1000, "approval": "required"
    }))
    .expect("manifest 合法")
}

type CallLog = Arc<Mutex<Vec<String>>>;

async fn rig(dir: &std::path::Path) -> (RuntimeHandle, CallLog) {
    let gated_calls: CallLog = Arc::new(Mutex::new(Vec::new()));
    let gated_view = gated_calls.clone();
    let mut capabilities = bm_providers::builtin::builtin_capability_set();
    capabilities.push((
        gated_manifest(),
        bm_core::broker::provider_fn(move |_| {
            gated_view.lock().expect("锁未中毒").push("gated".into());
            Ok(serde_json::json!({"did": "gated"}))
        }),
    ));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities,
        version: "0.1.0-session-mode".into(),
        data_dir: Some(dir.to_path_buf()),
        store: None,
        connector: Arc::new(GatedConnector {
            requests: Mutex::new(Vec::new()),
        }),
        secret_store: Arc::new(MemSecretStore::with("secret:mock.model", "sk-test-123456")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
    })
    .await;
    (handle, gated_calls)
}

async fn create_session(handle: &RuntimeHandle) -> bm_contract::wire::SessionCreateResult {
    let ids = SeqIdGen::new();
    handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "session-mode".into(),
                    model_chain: vec!["mock.model".into()],
                    budget: None,
                    system_prompt: None,
                    workspace_id: None,
                    allowed_tools: None,
                },
            },
        )
        .await
        .expect("建会话")
}

async fn session_mode_of(handle: &RuntimeHandle, sid: &str) -> String {
    handle
        .session_list()
        .await
        .into_iter()
        .find(|s| s.id == sid)
        .expect("会话在目录中")
        .permission_mode
}

async fn send_turn(
    handle: &RuntimeHandle,
    created: &bm_contract::wire::SessionCreateResult,
) -> bm_contract::wire::Receipt {
    let ids = SeqIdGen::new();
    let input = SendInputParams {
        session_id: created.session_id.clone(),
        agent_id: created.agent_id.clone(),
        content: "跑一次 gated".into(),
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

/// 轮询待裁决队列并对最新审批单批准 once(模拟用户在网页点同意)。
async fn respond_first_waiting(handle: RuntimeHandle) -> String {
    let ids = SeqIdGen::new();
    for _ in 0..100 {
        let list = handle
            .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
            .await
            .expect("审批列表可查");
        if let Some(aid) = list["approvals"][0]["approval_id"].as_str() {
            let approval_id = bm_contract::ids::BmId::parse(aid).unwrap();
            handle
                .approval_respond(
                    ids.next_id("req"),
                    bm_contract::wire::ApprovalRespondParams {
                        approval_id,
                        decision: "approve".into(),
                        scope: Some("once".into()),
                    },
                )
                .await
                .expect("人工批准");
            return aid.to_string();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("10s 内未出现待裁决审批单");
}

#[tokio::test]
async fn yolo_session_auto_approves_tool_round_with_mode_auto_audit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (handle, gated_calls) = rig(dir.path()).await;
    let created = create_session(&handle).await;
    // 新会话默认 ask(ADR-0030 决策 1)
    assert_eq!(
        session_mode_of(&handle, created.session_id.as_str()).await,
        "ask"
    );

    // 切 yolo:经管理面同款核心命令(handle.session_set_mode)
    handle
        .session_set_mode(
            created.session_id.clone(),
            bm_contract::wire::PermissionMode::Yolo,
        )
        .await
        .expect("切 yolo");
    assert_eq!(
        session_mode_of(&handle, created.session_id.as_str()).await,
        "yolo"
    );

    let started = std::time::Instant::now();
    let receipt = send_turn(&handle, &created).await;
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "yolo 会话审批类调用必须服务端即时放行,不得滞留等待: {:?}",
        started.elapsed()
    );
    assert_eq!(receipt.state, OperationState::Succeeded, "{receipt:?}");
    assert_eq!(
        gated_calls.lock().expect("锁未中毒").len(),
        1,
        "yolo 自动批准后工具必须真实执行"
    );

    // 审计:审批对象已批准且 source=mode_auto(与人工可区分)
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams {
            state_filter: Some("approved".into()),
        })
        .await
        .expect("列表可查");
    assert_eq!(
        list["approvals"][0]["resolved_source"],
        serde_json::json!("mode_auto"),
        "yolo 自动批准必须落 mode_auto 审计: {list}"
    );
}

#[tokio::test]
async fn ask_session_still_waits_for_user_and_audits_user_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (handle, gated_calls) = rig(dir.path()).await;
    let created = create_session(&handle).await;

    // ask(默认):回合会停在等待审批;并发模拟用户批准
    let responder = tokio::spawn(respond_first_waiting(handle.clone()));
    let receipt = send_turn(&handle, &created).await;
    let _aid = responder.await.expect("批准任务");
    assert_eq!(receipt.state, OperationState::Succeeded, "{receipt:?}");
    assert_eq!(
        gated_calls.lock().expect("锁未中毒").len(),
        1,
        "人工批准后工具执行"
    );

    // 审计:人工批准 source=user
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams {
            state_filter: Some("approved".into()),
        })
        .await
        .expect("列表可查");
    assert_eq!(
        list["approvals"][0]["resolved_source"],
        serde_json::json!("user"),
        "人工批准必须落 user 审计: {list}"
    );
    // ask 会话模式未被本流程改动
    assert_eq!(
        session_mode_of(&handle, created.session_id.as_str()).await,
        "ask"
    );
}

#[tokio::test]
async fn mode_change_persists_via_materialized_row_and_restarts() {
    let dir = tempfile::tempdir().expect("临时目录");
    std::fs::create_dir_all(dir.path().join("data")).expect("建数据目录");
    let store =
        Arc::new(bm_persist::PersistStore::open(&dir.path().join("data")).expect("打开持久层"));
    let connector: Arc<dyn ModelConnector> = Arc::new(GatedConnector {
        requests: Mutex::new(Vec::new()),
    });
    let config = |store: Arc<dyn bm_persist::EventStore>| RuntimeConfig {
        capabilities: vec![(
            gated_manifest(),
            bm_core::broker::provider_fn(|_| Ok(serde_json::json!({"did": "gated"}))),
        )],
        version: "0.1.0-session-mode".into(),
        data_dir: Some(dir.path().join("data")),
        store: Some(store),
        connector: connector.clone(),
        secret_store: Arc::new(MemSecretStore::with("secret:mock.model", "sk-test-123456")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
    };
    let handle1 = RuntimeHandle::start(config(store.clone())).await;
    let created = create_session(&handle1).await;
    handle1
        .session_set_mode(
            created.session_id.clone(),
            bm_contract::wire::PermissionMode::Yolo,
        )
        .await
        .expect("切 yolo");

    // 物化投影:sessions 行 permission_mode 已随事实事件落列
    let rows = store.load_rows().expect("行装配");
    let row = rows
        .sessions
        .iter()
        .find(|s| s.id == created.session_id.as_str())
        .expect("会话行");
    assert_eq!(
        row.permission_mode.as_deref(),
        Some("yolo"),
        "session.mode.changed 必须物化进 sessions 行: {:?}",
        row
    );
    drop(handle1);

    // 重启装载:内存会话模式自持久行恢复(ADR-0030 决策 1:服务端权威)
    let handle2 = RuntimeHandle::start(config(store)).await;
    assert_eq!(
        session_mode_of(&handle2, created.session_id.as_str()).await,
        "yolo",
        "重启后权限模式必须自持久层恢复"
    );
}
