//! M7-T1:OpenAI 兼容 HTTP 连接器行为(离线 mock HTTP,ADR-0010)。
//! 实网验证独立 #[ignore]:BOEN_LIVE=1 且 .secrets/dev.env 环境变量齐备时启用;
//! 真实密钥永不入库(基线 4.6 / INV-5)。

use bm_contract::connector::{InvokeRequest, InvokeResponse, Message, Role};
use bm_contract::error_codes::ErrorCode;
use bm_contract::ids::BmId;
use bm_contract::timestamp::format_ts;
use bm_core::ports::{ModelConnector, SecretStore};
use bm_providers::openai_http::OpenAiConnector;
use bm_providers::secret::MemSecretStore;
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const SECRET_REF: &str = "secret:model.gpt-5.6-luna";
const MODEL: &str = "gpt-5.6-luna";

const OK_BODY: &str = r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"Hello"}}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;

fn req(deadline_secs: i64) -> InvokeRequest {
    InvokeRequest {
        model_id: MODEL.to_string(),
        messages: vec![Message {
            role: Role::User,
            content: "ping".into(),
            tool_call_id: None,
            tool_calls: None,
        }],
        tools: vec![],
        params: Default::default(),
        secret_ref: SECRET_REF.to_string(),
        budget_ctx: bm_contract::connector::BudgetCtx {
            operation_id: BmId::generate("op"),
            agent_id: BmId::generate("agt"),
            remaining_tokens: 1000,
        },
        deadline: format_ts(Utc::now() + ChronoDuration::seconds(deadline_secs)),
        attempt: 1,
    }
}

/// 单请求 mock HTTP 服务:accept 一次,延迟后回固定 status/body,随即关闭。
fn spawn_mock(status: u16, body: &'static str, delay_ms: u64) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            if delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
            let reason = match status {
                200 => "OK",
                401 => "Unauthorized",
                429 => "Too Many Requests",
                500 => "Internal Server Error",
                _ => "Error",
            };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
            let _ = s.flush();
        }
    });
    format!("http://{addr}")
}

fn store_with_key() -> Arc<dyn SecretStore> {
    Arc::new(MemSecretStore::with(SECRET_REF, "sk-test"))
}

async fn invoke(base: String, store: Arc<dyn SecretStore>, r: InvokeRequest) -> InvokeResponse {
    OpenAiConnector::new(base, store)
        .invoke(r, CancellationToken::new())
        .await
}

#[tokio::test]
async fn t110_success_maps_content_usage_model() {
    let base = spawn_mock(200, OK_BODY, 0);
    let resp = invoke(base, store_with_key(), req(30)).await;
    match resp {
        InvokeResponse::Completed {
            content,
            usage,
            model_id,
            stream_interrupted,
            ..
        } => {
            assert_eq!(content, "Hello");
            assert_eq!((usage.tokens_in, usage.tokens_out), (10, 5));
            assert_eq!(model_id, MODEL);
            assert!(!stream_interrupted);
        }
        other => panic!("应为 Completed,实为 {other:?}"),
    }
}

#[tokio::test]
async fn t111_server_5xx_and_429_are_retryable_unavailable() {
    for status in [429, 500, 503] {
        let base = spawn_mock(status, r#"{"error":{"message":"x"}}"#, 0);
        let resp = invoke(base, store_with_key(), req(30)).await;
        match resp {
            InvokeResponse::Failed {
                error_code,
                retryable,
                ..
            } => {
                assert_eq!(error_code, ErrorCode::Unavailable, "status {status}");
                assert!(retryable, "status {status} 应可重试");
            }
            other => panic!("status {status} 应为 Failed,实为 {other:?}"),
        }
    }
}

#[tokio::test]
async fn t112_auth_4xx_is_permission_denied_not_retryable() {
    // P1(第四轮评审)修订:401/403 归 PermissionDenied(配置错,不烧熔断),
    // 不再是 Unavailable。
    let base = spawn_mock(401, r#"{"error":{"message":"bad key"}}"#, 0);
    let resp = invoke(base, store_with_key(), req(30)).await;
    match resp {
        InvokeResponse::Failed {
            error_code,
            retryable,
            ..
        } => {
            assert_eq!(error_code, ErrorCode::PermissionDenied);
            assert!(!retryable, "鉴权失败不应盲目重试");
        }
        other => panic!("应为 Failed,实为 {other:?}"),
    }
}

#[tokio::test]
async fn t113_malformed_body_maps_internal_without_leaking() {
    let base = spawn_mock(200, "<html>not json</html>", 0);
    let resp = invoke(base, store_with_key(), req(30)).await;
    match resp {
        InvokeResponse::Failed {
            error_code,
            retryable,
            ..
        } => {
            assert_eq!(error_code, ErrorCode::Internal);
            assert!(!retryable);
        }
        other => panic!("应为 Failed,实为 {other:?}"),
    }
}

#[tokio::test]
async fn t114_missing_secret_fails_fast_unavailable() {
    let base = spawn_mock(200, OK_BODY, 0);
    let empty = Arc::new(MemSecretStore::new());
    let resp = invoke(base, empty, req(30)).await;
    match resp {
        InvokeResponse::Failed {
            error_code,
            retryable,
            ..
        } => {
            assert_eq!(error_code, ErrorCode::Unavailable);
            assert!(retryable, "密钥缺失是可修复配置问题,标可重试");
        }
        other => panic!("应为 Failed,实为 {other:?}"),
    }
}

#[tokio::test]
async fn t115_deadline_timeout_fails_within_budget() {
    let base = spawn_mock(200, OK_BODY, 1500);
    let started = Instant::now();
    let resp = invoke(base, store_with_key(), req(0)).await; // deadline ≈ 现在
    match resp {
        InvokeResponse::Failed {
            error_code,
            retryable,
            ..
        } => {
            assert_eq!(error_code, ErrorCode::Unavailable);
            assert!(retryable, "超时可重试");
        }
        other => panic!("应为 Failed,实为 {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_millis(1200),
        "应在 deadline 预算内快速失败,实耗 {:?}",
        started.elapsed()
    );
}

// ---- 实网验证(ADR-0010 条件 3)--------------------------------------------

#[tokio::test]
#[ignore = "实网验证:BOEN_LIVE=1 且设 BOEN_LIVE_BASE_URL/BOEN_LIVE_MODEL/BOEN_LIVE_API_KEY"]
async fn t116_live_relay_chat_completion() {
    if std::env::var("BOEN_LIVE").as_deref() != Ok("1") {
        eprintln!("跳过:BOEN_LIVE 未设(离线套件不含实网)");
        return;
    }
    let base = std::env::var("BOEN_LIVE_BASE_URL").expect("BOEN_LIVE_BASE_URL");
    let model = std::env::var("BOEN_LIVE_MODEL").expect("BOEN_LIVE_MODEL");
    let key = std::env::var("BOEN_LIVE_API_KEY").expect("BOEN_LIVE_API_KEY");
    let secret_ref = bm_core::runtime::default_secret_ref(&model);
    let store = Arc::new(MemSecretStore::with(&secret_ref, &key));
    let conn = OpenAiConnector::new(base, store);
    let r = InvokeRequest {
        model_id: model.clone(),
        messages: vec![Message {
            role: Role::User,
            content: "请用一个短句确认连接成功。".into(),
            tool_call_id: None,
            tool_calls: None,
        }],
        tools: vec![],
        params: Default::default(),
        secret_ref,
        budget_ctx: bm_contract::connector::BudgetCtx {
            operation_id: BmId::generate("op"),
            agent_id: BmId::generate("agt"),
            remaining_tokens: 100000,
        },
        deadline: format_ts(Utc::now() + ChronoDuration::seconds(90)),
        attempt: 1,
    };
    let started = Instant::now();
    let resp = conn.invoke(r, CancellationToken::new()).await;
    match resp {
        InvokeResponse::Completed { content, usage, .. } => {
            println!(
                "实网成功:content={content:?} usage={}in/{}out latency={:?}",
                usage.tokens_in,
                usage.tokens_out,
                started.elapsed()
            );
            assert!(!content.trim().is_empty(), "实网返回不应为空");
        }
        InvokeResponse::Failed { error_code, .. } => {
            panic!("实网调用失败:{error_code:?}");
        }
    }
}
