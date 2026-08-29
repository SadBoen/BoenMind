//! M1-GT-01 黄金轨迹回放(套件 S3,P0):
//! 场景 A = 正常回合;场景 B = 模型链超时到失败(简版,补全会话生命周期)。
//! 逐条比对事件序列 + payload 关键值;全事件流过 envelope schema;含不变量覆盖表。

use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::ids::IdGen;
use bm_contract::registries;
use bm_contract::schemas::validate_by_pointer;
use bm_contract::states::OperationState;
use bm_contract::wire::GetOperationParams;
use bm_providers::mock_model::Step;
use bm_testkit::invariants::{assert_event_stream_wellformed, assert_single_terminal};
use bm_testkit::replay::{Expected, MODEL_A, TestRig, id};

#[tokio::test]
async fn gt01_scenario_a_normal_turn() {
    let rig = TestRig::standard(vec![Step::ok(
        "幂等性是指同一操作执行多次与执行一次的效果相同。",
        412,
        58,
    )])
    .await;
    let (sess, agent) = rig.create_session().await.expect("会话创建");
    let send_req = rig.ids.next_id("req");
    let receipt = rig
        .handle
        .send_input(
            send_req,
            rig.input(&sess, &agent, "用一句话解释什么是幂等性"),
        )
        .await
        .expect("回合发起");

    // 收据断言(A2)
    assert_eq!(receipt.state, OperationState::Running);
    assert_eq!(receipt.principal, bm_contract::wire::Principal::User);
    assert_eq!(receipt.task_type, bm_contract::wire::TaskType::AgentTurn);
    assert!(receipt.completed_at.is_none());
    assert!(receipt.error.is_none());

    // 等待终态(A3/A4)
    let final_receipt: bm_contract::wire::Receipt;
    loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            final_receipt = r;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(final_receipt.state, OperationState::Succeeded);
    assert_eq!(
        final_receipt
            .result_reference
            .as_ref()
            .expect("结果引用")
            .kind,
        bm_contract::wire::ResultRefKind::ExecutionLog
    );

    // A6:关闭会话并停机
    rig.handle
        .session_close(
            rig.ids.next_id("req"),
            bm_contract::wire::SessionCloseParams {
                session_id: sess.clone(),
                reason: Some("user_request".into()),
            },
        )
        .await
        .expect("close");
    rig.handle.stop("test_done").await;

    // 逐条比对回合流事件(M5 起启动期另有 12 条 bootstrap 协调权
    // grant.created 系统事实——过滤后回合流仍恰 11 条;全流 seq 连续性
    // 由 assert_event_stream_wellformed 覆盖,INV-3 无空洞)
    let all = rig.all_events().await;
    assert_event_stream_wellformed(&all);
    let events: Vec<EventEnvelope> = all
        .iter()
        .filter(|e| e.event_type != EventType::GrantCreated)
        .cloned()
        .collect();
    assert_eq!(events.len(), 11, "GT-A:回合流恰好 11 条事件");

    let expected = vec![
        Expected {
            ty: EventType::RuntimeStarted,
            payload: vec![
                ("pid", bm_testkit::replay::PVal::Any),
                ("version", bm_testkit::replay::PVal::Str("0.1.0-m1".into())),
                ("started_at", bm_testkit::replay::PVal::Any),
            ],
        },
        Expected {
            ty: EventType::SessionCreated,
            payload: vec![
                ("session_id", id(sess.as_str())),
                ("agent_id", id(agent.as_str())),
            ],
        },
        Expected {
            ty: EventType::AgentCreated,
            payload: vec![
                ("agent_id", id(agent.as_str())),
                ("session_id", id(sess.as_str())),
                (
                    "model_chain",
                    bm_testkit::replay::PVal::Raw(serde_json::json!([
                        bm_testkit::replay::MODEL_A,
                        bm_testkit::replay::MODEL_B,
                    ])),
                ),
            ],
        },
        Expected {
            ty: EventType::AgentTurnStarted,
            payload: vec![
                ("agent_id", id(agent.as_str())),
                ("operation_id", id(receipt.operation_id.as_str())),
                ("turn_index", bm_testkit::replay::PVal::Num(1)),
            ],
        },
        Expected {
            ty: EventType::AgentWaitingModel,
            payload: vec![
                ("agent_id", id(agent.as_str())),
                ("operation_id", id(receipt.operation_id.as_str())),
                ("model_id", bm_testkit::replay::PVal::Str(MODEL_A.into())),
            ],
        },
        Expected {
            ty: EventType::ModelInvocationCompleted,
            payload: vec![
                ("operation_id", id(receipt.operation_id.as_str())),
                ("agent_id", id(agent.as_str())),
                ("model_id", bm_testkit::replay::PVal::Str(MODEL_A.into())),
                ("attempt", bm_testkit::replay::PVal::Num(1)),
                ("usage_in", bm_testkit::replay::PVal::Num(412)),
                ("usage_out", bm_testkit::replay::PVal::Num(58)),
                ("latency_ms", bm_testkit::replay::PVal::Num(1873)),
                ("stream_interrupted", bm_testkit::replay::PVal::Bool(false)),
            ],
        },
        Expected {
            ty: EventType::OperationStateChanged,
            payload: vec![
                ("operation_id", id(receipt.operation_id.as_str())),
                ("from", bm_testkit::replay::PVal::Str("running".into())),
                ("to", bm_testkit::replay::PVal::Str("succeeded".into())),
                (
                    "reason_code",
                    bm_testkit::replay::PVal::Str("result_recorded".into()),
                ),
            ],
        },
        Expected {
            ty: EventType::AgentCompleted,
            payload: vec![
                ("agent_id", id(agent.as_str())),
                ("operation_id", id(receipt.operation_id.as_str())),
                ("turn_index", bm_testkit::replay::PVal::Num(1)),
            ],
        },
        Expected {
            ty: EventType::SessionClosed,
            payload: vec![
                ("session_id", id(sess.as_str())),
                (
                    "reason",
                    bm_testkit::replay::PVal::Str("user_request".into()),
                ),
            ],
        },
        Expected {
            ty: EventType::RuntimeStopping,
            payload: vec![("reason", bm_testkit::replay::PVal::Str("test_done".into()))],
        },
        Expected {
            ty: EventType::RuntimeStopped,
            payload: vec![("uptime_ms", bm_testkit::replay::PVal::Any)],
        },
    ];
    bm_testkit::replay::assert_matches(&events, &expected);

    // 每条事件过 envelope schema(R2 镜像)
    for e in &events {
        validate_by_pointer(
            registries::ENVELOPE_SCHEMA,
            "#/event_envelope",
            &serde_json::to_value(e).expect("可序列化"),
        )
        .unwrap_or_else(|err| panic!("事件 {} 未过 schema: {err}", e.event_type));
    }

    // 轨迹不变量覆盖:场景 A 满足 INV-1..9, INV-12(10/11 属失败路径)
    assert_single_terminal(&events, receipt.operation_id.as_str());
}

#[tokio::test]
async fn gt01_scenario_b_timeout_chain() {
    // 前置:同 A0/A1;模型链两次尝试全部超时(合同上限内 max_attempts=链长=2)
    let rig = TestRig::standard(vec![Step::timeout(), Step::timeout()]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建");
    let receipt = rig
        .send(&sess, &agent, "会超时的问题")
        .await
        .expect("回合发起");

    let final_receipt = loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            break r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };

    // 错误信封(GT-B):code=timeout, retryable=false
    assert_eq!(final_receipt.state, OperationState::Failed);
    let err = final_receipt.error.expect("错误信封");
    assert_eq!(err.code.get(), bm_contract::error_codes::ErrorCode::Timeout);
    assert!(!err.retryable);

    // 补全会话生命周期后比对
    rig.handle
        .session_close(
            rig.ids.next_id("req"),
            bm_contract::wire::SessionCloseParams {
                session_id: sess.clone(),
                reason: Some("user_request".into()),
            },
        )
        .await
        .expect("close");
    rig.handle.stop("test_done").await;

    let all = rig.all_events().await;
    assert_event_stream_wellformed(&all);
    let events: Vec<EventEnvelope> = all
        .iter()
        .filter(|e| e.event_type != EventType::GrantCreated)
        .cloned()
        .collect();
    let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
    assert_eq!(
        types,
        vec![
            EventType::RuntimeStarted,
            EventType::SessionCreated,
            EventType::AgentCreated,
            EventType::AgentTurnStarted,
            EventType::AgentWaitingModel,
            EventType::ModelInvocationFailed, // attempt 1: zhipu
            EventType::ModelInvocationFailed, // attempt 2: openai(降级)
            EventType::OperationStateChanged, // running→failed(guard: no external effect)
            EventType::AgentFailed,
            EventType::SessionClosed,
            EventType::RuntimeStopping,
            EventType::RuntimeStopped,
        ],
        "GT 场景 B(简版补全)事件序列"
    );
    assert_eq!(events[5].payload["attempt"], 1);
    assert_eq!(events[5].payload["model_id"], bm_testkit::replay::MODEL_A);
    assert_eq!(events[6].payload["attempt"], 2);
    assert_eq!(events[6].payload["model_id"], bm_testkit::replay::MODEL_B);
    assert_eq!(events[7].payload["from"], "running");
    assert_eq!(events[7].payload["to"], "failed");
    assert_eq!(events[8].payload["error_code"], "timeout");

    // 场景 B 的关键对照:不出现 outcome_unknown
    assert!(
        !events
            .iter()
            .any(|e| e.payload["to"].as_str() == Some("outcome_unknown"))
    );
    // 覆盖表:场景 B 满足 INV-2/3/9/10/11/12——本处直接断言 11(单终态为 failed)
    assert_single_terminal(&events, receipt.operation_id.as_str());
}
