//! W8(ADR-0018):会话工作区绑定与回合 system prompt 注入。
//! ① 会话创建绑定未登记工作区 → validation_failed;
//! ② 绑定已登记工作区 → 回合 system prompt 携带目录;
//! ③ send_input workspace_override 切换 → 下一条消息即换目录;
//! ④ 未绑定会话 → 不注入。

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
            content: "答复".into(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: Usage {
                tokens_in: 100,
                tokens_out: 10,
                ..Default::default()
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
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-w8".into(),
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

/// 在数据目录登记两个工作区(default 与 ws_proj)。
fn seed_workspaces(dir: &std::path::Path, proj_path: &str) {
    let cfg = dir.join("config");
    std::fs::create_dir_all(&cfg).expect("config 目录");
    std::fs::write(
        cfg.join("workspaces.json"),
        serde_json::json!({
            "workspaces": [
                {"id": "default", "name": "默认工作区", "path": dir.join("ws").display().to_string()},
                {"id": "ws_proj", "name": "项目甲", "path": proj_path}
            ]
        })
        .to_string(),
    )
    .expect("写注册表");
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

fn spec(workspace_id: Option<&str>) -> SessionCreateParams {
    SessionCreateParams {
        agent: AgentSpec {
            name: "w8".into(),
            model_chain: vec!["mock.model".into()],
            budget: None,
            system_prompt: None,
            workspace_id: workspace_id.map(str::to_string),
        },
    }
}

fn input(sess: &BmId, agent: &BmId, content: &str, workspace: Option<&str>) -> SendInputParams {
    SendInputParams {
        session_id: sess.clone(),
        agent_id: agent.clone(),
        content: content.into(),
        model_override: None,
        workspace_override: workspace.map(str::to_string),
        input_trust: InputTrust::Trusted,
    }
}

#[tokio::test]
async fn unregistered_workspace_rejects_session_create() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (handle, _connector) = rig(dir.path()).await;
    let ids = SeqIdGen::new();
    let err = handle
        .session_create(ids.next_id("req"), spec(Some("ws_ghost")))
        .await
        .expect_err("未登记工作区必须拒绝");
    match err {
        bm_core::CoreError::Semantic(code, msg) => {
            assert_eq!(code.as_str(), "validation_failed");
            assert!(msg.contains("ws_ghost"), "{msg}");
        }
        other => panic!("期望语义错误,得到 {other:?}"),
    }
}

#[tokio::test]
async fn bound_workspace_injects_directory_into_system_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let proj = tempfile::tempdir().expect("项目目录");
    seed_workspaces(dir.path(), proj.path().display().to_string().as_str());
    let (handle, connector) = rig(dir.path()).await;
    let ids = SeqIdGen::new();
    let created = handle
        .session_create(ids.next_id("req"), spec(Some("ws_proj")))
        .await
        .expect("已登记工作区应通过");
    let r = wait_terminal(
        &handle,
        &handle
            .send_input(
                ids.next_id("req"),
                input(&created.session_id, &created.agent_id, "看看项目", None),
            )
            .await
            .expect("回合发起")
            .operation_id,
    )
    .await;
    assert_eq!(r.state, OperationState::Succeeded);
    let reqs = connector.captured();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].messages[0].role, Role::System);
    assert!(
        reqs[0].messages[0]
            .content
            .contains(proj.path().display().to_string().as_str()),
        "system prompt 必须携带项目目录:{}",
        reqs[0].messages[0].content
    );
}

#[tokio::test]
async fn workspace_override_switches_directory_next_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let proj_a = tempfile::tempdir().expect("项目A");
    let proj_b = tempfile::tempdir().expect("项目B");
    seed_workspaces(dir.path(), proj_a.path().display().to_string().as_str());
    // 补登项目 B
    let cfg = dir.path().join("config");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cfg.join("workspaces.json")).unwrap())
            .unwrap();
    v["workspaces"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "ws_b", "name": "项目乙", "path": proj_b.path().display().to_string()
        }));
    std::fs::write(cfg.join("workspaces.json"), v.to_string()).unwrap();

    let (handle, connector) = rig(dir.path()).await;
    let ids = SeqIdGen::new();
    let created = handle
        .session_create(ids.next_id("req"), spec(Some("ws_proj")))
        .await
        .expect("绑定项目甲");
    wait_terminal(
        &handle,
        &handle
            .send_input(
                ids.next_id("req"),
                input(&created.session_id, &created.agent_id, "第一轮", None),
            )
            .await
            .expect("回合1")
            .operation_id,
    )
    .await;
    // 未登记覆盖拒绝
    let err = handle
        .send_input(
            ids.next_id("req"),
            input(
                &created.session_id,
                &created.agent_id,
                "第二轮",
                Some("ws_ghost"),
            ),
        )
        .await
        .expect_err("未登记覆盖必须拒绝");
    assert!(matches!(err, bm_core::CoreError::Semantic(..)));
    // 换到项目乙:下一条即生效
    wait_terminal(
        &handle,
        &handle
            .send_input(
                ids.next_id("req"),
                input(
                    &created.session_id,
                    &created.agent_id,
                    "第三轮",
                    Some("ws_b"),
                ),
            )
            .await
            .expect("回合3")
            .operation_id,
    )
    .await;

    let reqs = connector.captured();
    assert_eq!(reqs.len(), 2, "被拒回合不产生模型调用");
    assert!(
        !reqs[0].messages[0].content.contains("项目甲")
            && reqs[0].messages[0]
                .content
                .contains(proj_a.path().display().to_string().as_str()),
        "第一轮携带项目甲目录"
    );
    assert!(
        reqs[1].messages[0]
            .content
            .contains(proj_b.path().display().to_string().as_str()),
        "第三轮切换为项目乙目录:{}",
        reqs[1].messages[0].content
    );
}

#[tokio::test]
async fn unbound_session_has_no_workspace_injection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let proj = tempfile::tempdir().expect("项目目录");
    seed_workspaces(dir.path(), proj.path().display().to_string().as_str());
    let (handle, connector) = rig(dir.path()).await;
    let ids = SeqIdGen::new();
    let created = handle
        .session_create(ids.next_id("req"), spec(None))
        .await
        .expect("不绑定也应通过");
    wait_terminal(
        &handle,
        &handle
            .send_input(
                ids.next_id("req"),
                input(&created.session_id, &created.agent_id, "无目录", None),
            )
            .await
            .expect("回合发起")
            .operation_id,
    )
    .await;
    let reqs = connector.captured();
    assert_eq!(reqs.len(), 1);
    assert!(
        !reqs[0].messages[0].content.contains("[工作目录]"),
        "未绑定会话不得注入:{reqs:?}"
    );
}
