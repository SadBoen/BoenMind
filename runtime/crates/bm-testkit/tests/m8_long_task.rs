//! M8-T3:长任务压测(M8.4)——真实通道(gpt-5.6-luna)多回合 +
//! Wiki App 工具调用,全终态、无挂起、收据齐全,事件流喂独立 Judge
//! 出评估报告(回放 + 评估闭环)。
//! 门控:#[ignore] + BOEN_LIVE=1 + .secrets/dev.env 三变量(密钥零入仓)。

use bm_contract::budget::Budget;
use bm_contract::ids::{BmId, IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::AgentSpec;
use bm_contract::wire::{GetOperationParams, SendInputParams, SessionCreateParams};
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_judge::evaluate;
use bm_persist::{EventStore, PersistStore};
use bm_providers::mcp::{McpHub, StdioMcpTransport};
use bm_providers::openai_http::OpenAiConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
#[ignore = "实网长任务压测:BOEN_LIVE=1 且三变量齐备(密钥零入仓)"]
async fn t116_live_long_task_stress() {
    if std::env::var("BOEN_LIVE").as_deref() != Ok("1") {
        eprintln!("跳过:BOEN_LIVE 未设");
        return;
    }
    let base = std::env::var("BOEN_LIVE_BASE_URL").expect("BOEN_LIVE_BASE_URL");
    let model = std::env::var("BOEN_LIVE_MODEL").expect("BOEN_LIVE_MODEL");
    let key = std::env::var("BOEN_LIVE_API_KEY").expect("BOEN_LIVE_API_KEY");

    let dir = tempfile::tempdir().expect("临时目录");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));

    // 真实模型连接器(密钥经 Secret Store;明文不落日志/事件,INV-5)
    let secret_ref = bm_core::runtime::default_secret_ref(&model);
    let secrets = Arc::new(MemSecretStore::with(&secret_ref, &key));
    let connector: Arc<dyn ModelConnector> = Arc::new(OpenAiConnector::new(base, secrets.clone()));

    // Wiki App 真实文件域
    let wiki_dir = dir.path().join("wiki");
    let hub = McpHub::new();
    let transport = StdioMcpTransport::spawn(
        "python",
        &[
            format!(
                "{}",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../apps/wiki_server.py")
                    .display()
            ),
            "--dir".into(),
            wiki_dir.to_string_lossy().to_string(),
        ],
        &Default::default(),
    )
    .expect("wiki 子进程");
    let manifests = hub.connect("wiki", transport, 30_000).await.expect("握手");
    let entries = McpHub::capability_entries(manifests);

    let ids = Arc::new(SeqIdGen::new());
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: [vec![bm_providers::builtin::model_invoke_cap()], entries].concat(),
        async_executor: Some(hub),
        version: "0.1.0-m8-live".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: secrets,
        id_gen: ids.clone(),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    // 会话(真实单模型链)
    let created = handle
        .session_create(
            ids.next_id("req"),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "长任务助手".into(),
                    model_chain: vec![model.clone()],
                    budget: Some(Budget {
                        max_tokens: 200_000,
                        max_turns: 10,
                        extra: Default::default(),
                    }),
                },
            },
        )
        .await
        .expect("会话创建");
    let (sess, agent) = (created.session_id, created.agent_id);

    // page.list 预置直通验证(App 通道先热身)
    let req = ids.next_id("req");
    let receipt = handle
        .capability_call(
            req,
            bm_contract::wire::CapabilityCallParams {
                capability: "mcp.wiki.page.list".into(),
                args: json!({}),
                idempotency_key: None,
                deadline_ms: Some(30_000),
            },
        )
        .await
        .expect("list 派发");
    let list_op = BmId::parse(receipt["operation_id"].as_str().unwrap()).unwrap();
    let done = wait_terminal(&handle, &list_op).await;
    assert_eq!(done.state, OperationState::Succeeded, "App 通道热身");

    // 6 回合真实模型,每回合落一页 wiki 笔记(经独立 Judge 评估的负载)
    let prompts = [
        "用一句话定义幂等性。",
        "用一句话说明事件溯源的优点。",
        "用一句话解释 Capability 安全模型。",
        "用一句话说明写穿日志的作用。",
        "用一句话定义职责分离。",
        "用一句话总结上述五点。",
    ];
    for (i, p) in prompts.iter().enumerate() {
        let receipt = handle
            .send_input(
                ids.next_id("req"),
                SendInputParams {
                    session_id: sess.clone(),
                    agent_id: agent.clone(),
                    content: p.to_string(),
                    input_trust: bm_contract::wire::InputTrust::Trusted,
                },
            )
            .await
            .unwrap_or_else(|e| panic!("回合 {} 发起失败: {e:?}", i + 1));
        let done = wait_terminal(&handle, &receipt.operation_id).await;
        assert_eq!(
            done.state,
            OperationState::Succeeded,
            "回合 {} 必须成功:{:?}",
            i + 1,
            done.error
        );
        println!("回合 {}/{} 完成", i + 1, prompts.len());
    }

    // 全部事件喂 Judge → 报告;失败数必须为 0
    let last = store.last_log_seq().expect("日志末尾");
    let report = evaluate(store.as_ref(), 1, last).expect("评估");
    println!(
        "Judge 报告:passed={} failed={} skipped={}",
        report["summary"]["passed"], report["summary"]["failed"], report["summary"]["skipped"]
    );
    assert_eq!(
        report["summary"]["failed"],
        json!(0),
        "长任务事件流必须全过检查:{report}"
    );

    // 报告落库 round-trip
    store
        .save_evaluation_report(
            report["report_id"].as_str().unwrap(),
            1,
            last,
            &report.to_string(),
            report["generated_at"].as_str().unwrap(),
        )
        .expect("报告落库");
    let listed = store.list_evaluation_reports().expect("报告列表");
    assert_eq!(listed.len(), 1);

    handle.stop("test_done").await;
}

async fn wait_terminal(handle: &RuntimeHandle, op_id: &BmId) -> bm_contract::wire::Receipt {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    loop {
        assert!(tokio::time::Instant::now() < deadline, "180s 未终态");
        let r = handle
            .operations_get(GetOperationParams {
                operation_id: op_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
