//! bm-runtime:进程内组装入口(M1)。无 CLI、无网络监听(M3 起);
//! 本二进制演示一次完整的单 Agent 回合(mock 模型),作为可运行冒烟。

use bm_contract::ids::SeqIdGen;
use bm_contract::wire::{AgentSpec, GetOperationParams, SessionCreateParams};
use bm_core::CoreResult;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use std::sync::Arc;

#[tokio::main]
async fn main() -> CoreResult<()> {
    println!(
        "boenmind runtime {}(M1 单 Agent 闭环冒烟)",
        env!("CARGO_PKG_VERSION")
    );

    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![Step::ok(
        "幂等性是指同一操作执行多次与执行一次的效果相同。",
        412,
        58,
    )]));
    let secrets = Arc::new(MemSecretStore::with(
        &bm_core::runtime::default_secret_ref(bm_testkit_replay::MODEL_A),
        "sk-demo-zhipu-secret-value-001",
    ));

    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        async_executor: None,
        model_streaming: std::env::var("BOEN_MODEL_STREAM").as_deref() == Ok("1"),
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: secrets,
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    let created = handle
        .session_create(
            next_req(),
            SessionCreateParams {
                agent: AgentSpec {
                    name: "assistant".into(),
                    model_chain: vec![bm_testkit_replay::MODEL_A.into()],
                    budget: Some(bm_contract::budget::Budget {
                        max_tokens: 50_000,
                        max_turns: 10,
                        extra: Default::default(),
                    }),
                    system_prompt: None,
                },
            },
        )
        .await?;
    println!("会话 {} + Agent {}", created.session_id, created.agent_id);

    let receipt = handle
        .send_input(
            next_req(),
            bm_testkit_replay::input(
                &created.session_id,
                &created.agent_id,
                "用一句话解释什么是幂等性",
            ),
        )
        .await?;
    println!(
        "回合发起:operation={} state={}",
        receipt.operation_id, receipt.state
    );

    let mut final_receipt = receipt.clone();
    for _ in 0..200 {
        let r = handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await?;
        if r.state.is_terminal() {
            final_receipt = r;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    println!(
        "回合终态:{} 摘要:{}",
        final_receipt.state, final_receipt.action_summary
    );

    handle.stop("demo_done").await;
    println!("停机完成");
    Ok(())
}

fn next_req() -> bm_contract::ids::BmId {
    static IDS: std::sync::OnceLock<SeqIdGen> = std::sync::OnceLock::new();
    let ids = IDS.get_or_init(SeqIdGen::new);
    bm_contract::ids::IdGen::next_id(ids, "req")
}

/// 复用 testkit 的常量与输入构造,避免 demo 与测试漂移。
mod bm_testkit_replay {
    // bm-runtime 不依赖 bm-testkit(测试专用 crate 不进生产),此处仅保留常量。
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
