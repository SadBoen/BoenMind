//! chaos-child:S4 崩溃恢复测试的被杀子进程。
//!
//! 用法:`chaos-child <dir> <run|verify>`
//! - run: 起运行时(写穿持久层)→ 建会话 → 发起长回合(60s 注入)→
//!   写 marker 文件(此刻回合事件已落盘)→ 长眠等待被 taskkill/kill。
//! - verify: 起运行时(触发启动恢复)→ 输出恢复结果 JSON → 退出。
//!
//! 父测试负责:等 marker → 硬杀(taskkill /F 或 TerminateProcess)→ 再起
//! verify 子进程 → 断言恢复面。真实进程死亡由 OS 提供,不模拟。

use bm_contract::budget::Budget;
use bm_contract::ids::{BmId, IdGen, SeqIdGen};
use bm_contract::wire::{AgentSpec, GetOperationParams, SessionCreateParams};
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::{EventStore, PersistStore};
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use std::path::PathBuf;
use std::sync::Arc;

async fn start_runtime(dir: &std::path::Path, script: Vec<Step>) -> RuntimeHandle {
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(script));
    let secrets = Arc::new(MemSecretStore::with(
        &bm_core::runtime::default_secret_ref(bm_testkit_replay::MODEL_A),
        "sk-demo-zhipu-secret-value-001",
    ));
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(PersistStore::open(dir).expect("子进程打开持久层"));
    RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-m1".into(),
        data_dir: Some(dir.to_path_buf()),
        store: Some(store),
        connector,
        secret_store: secrets,
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
    })
    .await
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(args.get(1).expect("用法: chaos-child <dir> <run|verify>"));
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("run");

    match mode {
        "run" => {
            let handle = start_runtime(&dir, vec![Step::ok_after("被杀前的长回合", 60_000)]).await;
            let created = handle
                .session_create(
                    IdGen::next_id(&SeqIdGen::new(), "req"),
                    SessionCreateParams {
                        agent: AgentSpec {
                            name: "assistant".into(),
                            model_chain: vec![bm_testkit_replay::MODEL_A.into()],
                            budget: Some(Budget {
                                max_tokens: 1_000_000,
                                max_turns: 100,
                                extra: Default::default(),
                            }),
                        },
                    },
                )
                .await
                .expect("子进程建会话");
            let receipt = handle
                .send_input(
                    IdGen::next_id(&SeqIdGen::new(), "req"),
                    bm_testkit_replay::input(&created.session_id, &created.agent_id, "长问题"),
                )
                .await
                .expect("子进程发起回合");
            // marker:写穿已保证 turn.started/waiting_model 事件落盘
            std::fs::write(dir.join("chaos_marker"), receipt.operation_id.as_str())
                .expect("写 marker");
            // 长眠等待父进程硬杀
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        }
        "verify" => {
            let handle = start_runtime(&dir, vec![Step::ok("续答", 10, 5)]).await;
            // 恢复已在 start 内完成(含 claim 重驱);重开只读连接读行(WAL 并发读)
            let store = PersistStore::open(&dir).expect("verify 打开持久层");
            let sessions = store
                .state()
                .query_rows("SELECT id, state FROM sessions", &[])
                .expect("读会话");
            let ops = store
                .state()
                .query_rows("SELECT id, state FROM operations", &[])
                .expect("读操作");
            let log = store.replay_since(0).expect("读日志");
            let recovered = log
                .iter()
                .find(|e| e.event_type == bm_contract::events::EventType::RuntimeRecovered)
                .expect("恢复事件在场");
            let interrupted_audit = log
                .iter()
                .any(|e| e.event_type == bm_contract::events::EventType::AgentInterrupted);

            let session_state = sessions
                .first()
                .map(|s| s["state"].as_str().unwrap_or("?").to_string())
                .unwrap_or_else(|| "NONE".into());

            // claim 重驱:等被杀回合到达终态(最长 30s)
            let op_id: BmId = BmId::parse(
                ops.first().expect("有 operation")["id"]
                    .as_str()
                    .expect("字符串"),
            )
            .expect("合法 op id");
            let mut final_state = "TIMEOUT".to_string();
            for _ in 0..300 {
                let receipt = handle
                    .operations_get(GetOperationParams {
                        operation_id: op_id.clone(),
                    })
                    .await
                    .expect("收据查询");
                if receipt.state.is_terminal() {
                    final_state = receipt.state.as_str().to_string();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            println!(
                "{}",
                serde_json::json!({
                    "session_state": session_state,
                    "op_state": final_state,
                    "interrupted_audit": interrupted_audit,
                    "interrupted_recovered": recovered.payload["interrupted_recovered"],
                    "replayed": recovered.payload["replayed"],
                })
            );
        }
        other => panic!("未知模式: {other}"),
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
