//! W1(ADR-0014):OpenAI 兼容插座——`POST /v1/chat/completions` + `GET /v1/models`。
//!
//! 对话层行业标准接口:任何 OpenAI 兼容前端(含 W 系列壳子)即插即用接
//! BoenMind 自研 Agent。会话由 runtime 持有,壳子经 `X-Bm-Session` 请求头
//! 寻址续聊;历史由 runtime 侧维护,壳子只需传增量最后一条 user 消息
//! (W1 合同口径,见 milestones/W1-implementation-spec.md §4)。
//! 免鉴权 = 已登记欠账(公网部署前必须补 Bearer,沿 ADR-0009 T-13/T-14)。

use crate::AppState;
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bm_contract::events::EventType;
use bm_contract::ids::{BmId, IdGen, UlidIdGen};
use bm_contract::wire::{
    AgentSpec, InputTrust, SendInputParams, SessionCreateParams, SessionResumeParams,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn err_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": { "message": message, "type": "invalid_request_error" }
        })),
    )
        .into_response()
}

/// GET /v1/models:服务器当前配置的模型(单配置模型,W1 口径)。
pub async fn models(State(state): State<AppState>) -> Response {
    Json(serde_json::json!({
        "object": "list",
        "data": [ { "id": *state.default_model, "object": "model" } ]
    }))
    .into_response()
}

fn chunk(sid: &str, model: &str, delta: serde_json::Value, finish: Option<&str>) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        serde_json::json!({
            "id": format!("chatcmpl-{sid}"),
            "object": "chat.completion.chunk",
            "created": unix_now(),
            "model": model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish,
            }],
        })
    ))
}

/// POST /v1/chat/completions:对话闭环(流式 SSE / 非流式 JSON)。
pub async fn chat_completions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let default_model = (*state.default_model).clone();

    // W6 对话级模型选择:body.model = 所选模型;"auto"/缺省 = 服务器默认。
    // 路由表非空且未知名 → 400(防静默落 mock/错网关);表空(mock 开发态)
    // 不校验,W1 行为不破。
    let requested_model: Option<String> = body["model"]
        .as_str()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != "auto")
        .map(|s| s.to_string());
    if let (Some(m), Some(routes)) = (&requested_model, state.model_routes.as_ref())
        && !routes.known_models().is_empty()
        && !routes.contains(m)
    {
        return err_response(
            StatusCode::BAD_REQUEST,
            &format!(
                "模型「{m}」不在已配置清单(设置 → 模型 里核对 id 或勾选常用);可用: {}",
                routes.known_models().join(", ")
            ),
        );
    }

    // 取最后一条 user 消息文本(content 为字符串或多模态 parts 数组两种形状)
    let Some(messages) = body["messages"].as_array() else {
        return err_response(StatusCode::BAD_REQUEST, "缺少 messages 数组");
    };
    let text = messages
        .iter()
        .rev()
        .find(|m| m["role"] == serde_json::json!("user"))
        .and_then(|m| {
            if let Some(s) = m["content"].as_str() {
                Some(s.to_string())
            } else {
                let parts: Vec<String> = m["content"]
                    .as_array()?
                    .iter()
                    .filter(|p| p["type"] == serde_json::json!("text"))
                    .filter_map(|p| p["text"].as_str().map(|s| s.to_string()))
                    .collect();
                Some(parts.join("\n"))
            }
        });
    let Some(text) = text.filter(|s| !s.trim().is_empty()) else {
        return err_response(StatusCode::BAD_REQUEST, "messages 缺少非空 user 消息");
    };

    let target_role_id = headers
        .get("x-bm-role")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // W8(ADR-0018):body 可选 workspace = 工作区注册表 id(与 model 同款
    // 对话级选择口径;空/缺省 = 不绑定不覆盖)。校验在核心(登记表为准)。
    let requested_workspace: Option<String> = body["workspace"]
        .as_str()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // 会话寻址:有 X-Bm-Session 续聊;无则新建(默认配置模型)
    let (rt_sid, rt_aid) = match headers
        .get("x-bm-session")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
    {
        Some(raw) => match BmId::parse(raw) {
            Ok(sid) => {
                // 先取克隆并让锁守卫出作用域(不得跨 await 持锁)
                let cached = state
                    .v1_sessions
                    .lock()
                    .expect("锁未中毒")
                    .get(&sid)
                    .cloned();
                match cached {
                    Some(aid) => (sid, aid),
                    None => {
                        // 重启续聊(2026-09-06):v1_sessions 是进程内寻址表,
                        // 重启即空;会话本体自持久层装载并未丢——回源
                        // session.resume 恢复寻址,旧会话继续聊,不再 400 逼重开。
                        // since_seq=MAX:寻址回源不需要补发事件
                        match state
                            .handle
                            .session_resume(
                                UlidIdGen.next_id("req"),
                                SessionResumeParams {
                                    session_id: sid.clone(),
                                    since_seq: Some(u64::MAX),
                                },
                            )
                            .await
                        {
                            Ok(r) => {
                                state
                                    .v1_sessions
                                    .lock()
                                    .expect("锁未中毒")
                                    .insert(sid.clone(), r.agent_id.clone());
                                (sid, r.agent_id)
                            }
                            Err(_) => {
                                return err_response(
                                    StatusCode::BAD_REQUEST,
                                    "未知会话:请清除界面会话记忆后重新开始",
                                );
                            }
                        }
                    }
                }
            }
            Err(_) => return err_response(StatusCode::BAD_REQUEST, "X-Bm-Session 不是合法会话 id"),
        },
        None => {
            let request_id = UlidIdGen.next_id("req");
            // W4b:允许通过 X-Bm-Role 指定角色(缺省 = active 角色);
            // system_prompt 由 bm-core::roles 统一组装(含挂载技能),
            // 空提示词传 None——交由回合侧热读,保证技能/角色后续可生效。
            let initial_system_prompt = state.data_dir.as_ref().and_then(|d| {
                bm_core::roles::compose_role_prompt(d, target_role_id.as_deref())
                    .filter(|s| !s.is_empty())
            });
            match state
                .handle
                .session_create(
                    request_id,
                    SessionCreateParams {
                        agent: AgentSpec {
                            name: "webui".to_string(),
                            // W6:对话选择了模型则以其为初始链(后续回合仍可
                            // 随消息携带 model_override 热切换)。
                            model_chain: vec![
                                requested_model
                                    .clone()
                                    .unwrap_or_else(|| default_model.clone()),
                            ],
                            budget: None,
                            system_prompt: initial_system_prompt,
                            // W8:对话选择了工作区则随会话创建绑定(校验在核心;
                            // 未登记 id 会话创建即 400,错误消息透出)。
                            workspace_id: requested_workspace.clone(),
                        },
                    },
                )
                .await
            {
                Ok(r) => {
                    state
                        .v1_sessions
                        .lock()
                        .expect("锁未中毒")
                        .insert(r.session_id.clone(), r.agent_id.clone());
                    (r.session_id, r.agent_id)
                }
                Err(e) => {
                    return err_response(
                        StatusCode::BAD_REQUEST,
                        &format!("会话创建失败: {}", e.to_wire().message),
                    );
                }
            }
        }
    };

    // 发送前取日志末位,作为本回合的事件轮询游标(空日志/首启文件未建 = 0)
    let mut cursor = state.store.last_log_seq().unwrap_or(0);
    let request_id = UlidIdGen.next_id("req");
    let sent = state
        .handle
        .send_input(
            request_id,
            SendInputParams {
                session_id: rt_sid.clone(),
                agent_id: rt_aid.clone(),
                content: text,
                input_trust: InputTrust::Trusted,
                // W6:每条消息都携带当前所选模型 → 对话中途切换下一条即生效
                model_override: requested_model,
                // W8:每条消息都携带当前所选工作区 → 中途切换下一条即生效
                workspace_override: requested_workspace,
            },
        )
        .await;
    if let Err(e) = &sent {
        // W8:校验类失败(如工作区未登记)按 400 透出,便于壳子清理本地选择
        let wire = e.to_wire();
        let status = if wire.code.get() == bm_contract::error_codes::ErrorCode::ValidationFailed {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        return err_response(status, &wire.message);
    }

    let session_header =
        HeaderValue::from_str(rt_sid.as_str()).unwrap_or(HeaderValue::from_static("invalid"));
    let stream = body["stream"].as_bool().unwrap_or(false);

    if !stream {
        // 非流式:轮询聚合到终态一次返回
        let store = state.store.clone();
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if Instant::now() > deadline {
                return err_response(StatusCode::INTERNAL_SERVER_ERROR, "回合超时");
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
            let Ok(events) = store.replay_since(cursor) else {
                continue;
            };
            let mut content: Option<String> = None;
            let mut failed = false;
            for e in events {
                cursor = cursor.max(e.event_seq);
                if e.session_id.as_ref() != Some(&rt_sid) {
                    continue;
                }
                match e.event_type {
                    EventType::ModelInvocationCompleted => {
                        content = Some(
                            e.payload["content"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                    EventType::AgentFailed | EventType::AgentCancelled => {
                        failed = true;
                    }
                    _ => {}
                }
            }
            if content.is_some() || failed {
                let body = serde_json::json!({
                    "id": format!("chatcmpl-{}", rt_sid.as_str()),
                    "object": "chat.completion",
                    "created": unix_now(),
                    "model": default_model,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": content.unwrap_or_else(|| "[回合失败或已取消]".into()),
                        },
                        "finish_reason": "stop",
                    }],
                });
                let mut response = Json(body).into_response();
                response
                    .headers_mut()
                    .insert("x-bm-session", session_header);
                return response;
            }
        }
    }

    // 流式:SSE(Role 起手 → delta → finish → [DONE])
    let store = state.store.clone();
    let sid = rt_sid.to_string();
    let body_stream = async_stream::stream! {
        let first = serde_json::json!({
            "id": format!("chatcmpl-{sid}"),
            "object": "chat.completion.chunk",
            "created": unix_now(),
            "model": default_model,
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": "" },
                "finish_reason": null,
            }],
        });
        yield Ok::<Bytes, std::io::Error>(
            Bytes::from(format!("data: {first}\n\n")),
        );

        let mut emitted: usize = 0; // 已按 delta 下发的字符数(补 completion 余量用)
        let deadline = Instant::now() + Duration::from_secs(180);
        // 静默保活(2026-09-02 修「工具调用卡死」):工具轮执行期间事件面
        // 可静默 25s+,前端看门狗(60s 无任何字节即中止)会被误杀。空闲超
        // 10s 下发一行 SSE 注释——前端按任意字节重置看门狗,注释行被解析
        // 器忽略,不污染内容。
        let mut last_byte = Instant::now();
        loop {
            if Instant::now() > deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
            if last_byte.elapsed() > Duration::from_secs(10) {
                last_byte = Instant::now();
                yield Ok::<Bytes, std::io::Error>(Bytes::from(": keepalive\n\n"));
            }
            let Ok(events) = store.replay_since(cursor) else {
                continue;
            };
            let mut finished = false;
            for e in events {
                cursor = cursor.max(e.event_seq);
                if e.session_id.as_ref() != Some(&rt_sid) {
                    continue;
                }
                match e.event_type {
                    EventType::ModelContentDelta => {
                        let delta = e.payload["delta"].as_str().unwrap_or_default();
                        if delta.is_empty() {
                            continue;
                        }
                        emitted += delta.chars().count();
                        last_byte = Instant::now();
                        yield Ok(chunk(&sid, &default_model,
                            serde_json::json!({ "content": delta }), None));
                    }
                    EventType::ModelInvocationCompleted => {
                        // 非 / 断续流连接器:completion 载荷含全文,补发未下发余量
                        let content = e.payload["content"].as_str().unwrap_or_default();
                        let remaining: String = content.chars().skip(emitted).collect();
                        if !remaining.is_empty() {
                            yield Ok(chunk(&sid, &default_model,
                                serde_json::json!({ "content": remaining }), None));
                        }
                        finished = true;
                        break;
                    }
                    EventType::AgentFailed | EventType::AgentCancelled => {
                        yield Ok(chunk(&sid, &default_model,
                            serde_json::json!({ "content": "\n[回合失败或已取消]" }), None));
                        finished = true;
                        break;
                    }
                    EventType::AgentInterrupted => {
                        finished = true;
                        break;
                    }
                    _ => {}
                }
            }
            if finished {
                break;
            }
        }
        yield Ok(Bytes::from(
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        ));
        yield Ok(Bytes::from("data: [DONE]\n\n"));
    };

    let mut response = Response::new(Body::from_stream(body_stream));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
        .headers_mut()
        .insert("x-bm-session", session_header);
    response
}
