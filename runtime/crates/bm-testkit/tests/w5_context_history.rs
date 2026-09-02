//! W5:会话历史回喂 + 上下文快照(context-log.jsonl)。
//! ① 同会话第二轮请求必须携带第一轮 user/assistant 消息(此前每轮从零
//!    组装,模型对同会话前情失忆——2026-09-02 用户反馈轮修复);
//! ② 每次模型调用落一条快照(请求侧 messages/tools + 结果侧 status/usage)。

use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, Role, Usage};
use bm_contract::ids::{BmId, IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{
    AgentSpec, GetOperationParams, InputTrust, SendInputParams, SessionCreateParams,
};
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::secret::MemSecretStore;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// 捕获请求的脚本连接器:记录每次 InvokeRequest,恒回固定成功回复。
struct CaptureConnector {
    requests: Mutex<Vec<InvokeRequest>>,
}

impl CaptureConnector {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }
    fn captured(&self) -> Vec<InvokeRequest> {
        self.requests.lock().expect("锁未中毒").clone()
    }
}

#[async_trait::async_trait]
impl ModelConnector for CaptureConnector {
    async fn invoke(&self, req: InvokeRequest, _cancel: CancellationToken) -> InvokeResponse {
        let model_id = req.model_id.clone();
        self.requests.lock().expect("锁未中毒").push(req);
        InvokeResponse::Completed {
            content: "第一轮答复".into(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: Usage {
                tokens_in: 100,
                tokens_out: 10,
            },
            model_id,
            latency_ms: 5,
            stream_interrupted: false,
        }
    }
    fn provider(&self) -> &'static str {
        "mock"
    }
}

async fn rig(dir: &std::path::Path) -> (RuntimeHandle, Arc<CaptureConnector>) {
    let connector = Arc::new(CaptureConnector::new());
    let handle = RuntimeHandle::start(RuntimeConfig {
        // ADR-0006 权力显式化:model.invoke 未注册即不存在,授权裁决必拒
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-w5".into(),
        data_dir: Some(dir.to_path_buf()),
        store: None,
        connector: connector.clone(),
        secret_store: Arc::new(MemSecretStore::with("secret:mock.model", "sk-test-123456")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    })
    .await;
    (handle, connector)
}

async fn wait_terminal(handle: &RuntimeHandle, op: &BmId) -> bm_contract::wire::Receipt {
    loop {
        let r = handle
            .operations_get(GetOperationParams {
                operation_id: op.clone(),
            })
            .await
            .expect("收据查询");
        if r.state.is_terminal() {
            return r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn second_turn_request_carries_first_turn_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (handle, connector) = rig(dir.path()).await;
    let ids = SeqIdGen::new();
    let created = handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "w5".into(),
                    model_chain: vec!["mock.model".into()],
                    budget: None,
                    system_prompt: None,
                },
            },
        )
        .await
        .expect("会话创建");
    let (sess, agent) = (created.session_id, created.agent_id);

    let r1 = wait_terminal(
        &handle,
        &handle
            .send_input(
                ids.next_id("req"),
                SendInputParams {
                    session_id: sess.clone(),
                    agent_id: agent.clone(),
                    content: "第一轮".into(),
                    model_override: None,
                    input_trust: InputTrust::Trusted,
                },
            )
            .await
            .expect("回合1发起")
            .operation_id,
    )
    .await;
    assert_eq!(r1.state, OperationState::Succeeded);
    let r2 = wait_terminal(
        &handle,
        &handle
            .send_input(
                ids.next_id("req"),
                SendInputParams {
                    session_id: sess.clone(),
                    agent_id: agent.clone(),
                    content: "第二轮".into(),
                    model_override: None,
                    input_trust: InputTrust::Trusted,
                },
            )
            .await
            .expect("回合2发起")
            .operation_id,
    )
    .await;
    assert_eq!(r2.state, OperationState::Succeeded);

    let reqs = connector.captured();
    assert_eq!(reqs.len(), 2);
    // 第一轮:仅本轮输入(数据目录无 config/roles.json → 无 system prompt)
    assert_eq!(reqs[0].messages.len(), 1);
    assert_eq!(reqs[0].messages[0].content, "第一轮");
    // 第二轮:历史对回喂(user/assistant)+ 本轮输入
    assert_eq!(reqs[1].messages.len(), 3);
    assert_eq!(reqs[1].messages[0].role, Role::User);
    assert_eq!(reqs[1].messages[0].content, "第一轮");
    assert_eq!(reqs[1].messages[1].role, Role::Assistant);
    assert_eq!(reqs[1].messages[1].content, "第一轮答复");
    assert_eq!(reqs[1].messages[2].content, "第二轮");

    // 上下文快照:每次调用一条(请求侧+结果侧)
    let raw = std::fs::read_to_string(dir.path().join("context-log.jsonl")).expect("快照文件存在");
    let lines: Vec<serde_json::Value> = raw
        .lines()
        .map(|l| serde_json::from_str(l).expect("快照行可解析"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["messages"].as_array().unwrap().len(), 1);
    assert_eq!(lines[1]["messages"].as_array().unwrap().len(), 3);
    assert_eq!(lines[1]["status"], serde_json::json!("ok"));
    assert_eq!(lines[1]["tokens_in"], serde_json::json!(100));
    assert_eq!(lines[1]["tokens_out"], serde_json::json!(10));
    assert_eq!(lines[1]["session_id"], serde_json::json!(sess.as_str()));
    assert_eq!(lines[1]["step"], serde_json::json!(1));
    assert_eq!(lines[1]["attempt"], serde_json::json!(1));
}
