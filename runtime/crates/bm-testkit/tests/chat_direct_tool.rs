//! 直通工具对话闭环(2026-09-03 VPS 实测 P1 回归):模型发起 system__echo
//! 直通调用,同步收据 state=succeeded 且 result 内联——修复前该结果从不
//! 写入 op_results(仅异步回单/审批重放两路写入),回合轮询必等满 60s
//! 回喂「工具执行超时」;修复后立即回喂,整回合秒级落定。
//! 这也是全仓第一个覆盖对话工具轮(ToolCalls→回喂→终稿)的回合级测试
//! (此前 chat E2E 恰好绕过此断点,详见 BACKLOG 直通链路条目)。

use bm_contract::connector::{
    FinishReason, InvokeRequest, InvokeResponse, Role, ToolCallPayload, Usage,
};
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

/// 脚本连接器:第 1 次调用发起 system__echo 工具调用,第 2 次给终稿。
struct ToolLoopConnector {
    requests: Mutex<Vec<InvokeRequest>>,
}

impl ToolLoopConnector {
    fn captured(&self) -> Vec<InvokeRequest> {
        self.requests.lock().expect("锁未中毒").clone()
    }
}

#[async_trait::async_trait]
impl ModelConnector for ToolLoopConnector {
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
                    name: "system__echo".into(),
                    arguments: r#"{"m":"hi"}"#.into(),
                }],
                finish_reason: FinishReason::ToolCalls,
                usage: Usage {
                    tokens_in: 10,
                    tokens_out: 5,
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

async fn rig(dir: &std::path::Path) -> (RuntimeHandle, Arc<ToolLoopConnector>) {
    let connector = Arc::new(ToolLoopConnector {
        requests: Mutex::new(Vec::new()),
    });
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        version: "0.1.0-direct-tool".into(),
        data_dir: Some(dir.to_path_buf()),
        store: None,
        connector: connector.clone(),
        secret_store: Arc::new(MemSecretStore::with("secret:mock.model", "sk-test-123456")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
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
async fn direct_tool_round_feeds_inline_result_without_poll_timeout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (handle, connector) = rig(dir.path()).await;
    let ids = SeqIdGen::new();
    let created = handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "direct-tool".into(),
                    model_chain: vec!["mock.model".into()],
                    budget: None,
                    system_prompt: None,
                    workspace_id: None,
                },
            },
        )
        .await
        .expect("建会话");
    let input = SendInputParams {
        session_id: created.session_id.clone(),
        agent_id: created.agent_id.clone(),
        content: "调一下 echo".into(),
        model_override: None,
        workspace_override: None,
        input_trust: InputTrust::Trusted,
    };

    let started = std::time::Instant::now();
    let receipt = wait_terminal(
        &handle,
        &handle
            .send_input(ids.next_id("req"), input)
            .await
            .expect("回合发起")
            .operation_id,
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(receipt.state, OperationState::Succeeded, "{receipt:?}");
    // 修复前:直通结果不入 op_results,轮询等满 60s 才回喂(整回合 >60s);
    // 修复后:同步收据内联 result 立即回喂,整回合秒级落定。
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "直通工具轮必须秒级完成(修复前必等 60s 轮询超时): {elapsed:?}"
    );
    // 工具调用后必须以 Tool 消息回喂并重调模型(两轮请求)
    let reqs = connector.captured();
    assert_eq!(reqs.len(), 2, "工具轮必须重调模型");
    assert!(
        reqs[1]
            .messages
            .iter()
            .any(|m| m.role == Role::Tool && m.content.contains("hi")),
        "同步直通结果必须作为 Tool 消息回喂: {:?}",
        reqs[1].messages
    );

    // W9:context-log 必须落轨迹事件流(tool_call→tool_result→
    // assistant_final→turn_end),供「上下文」轨迹视图回放
    let raw = std::fs::read_to_string(dir.path().join("context-log.jsonl"))
        .expect("context-log.jsonl 必须存在");
    let kinds: Vec<String> = raw
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["kind"].as_str().map(String::from))
        .collect();
    for want in ["tool_call", "tool_result", "assistant_final", "turn_end"] {
        assert!(
            kinds.iter().any(|k| k == want),
            "缺 {want} 事件; 实际={kinds:?}"
        );
    }
    let tool_result_line = raw
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["kind"] == serde_json::json!("tool_result"))
        .expect("tool_result 行");
    assert!(
        tool_result_line["data"]["result"]
            .to_string()
            .contains("hi"),
        "工具回喂原文必须入账: {tool_result_line}"
    );
}
