//! W1(ADR-0014):OpenAI 兼容插座——`POST /v1/chat/completions` + `GET /v1/models`。
//!
//! 对话层行业标准接口:任何 OpenAI 兼容前端(含 W 系列壳子)即插即用接
//! BoenMind 自研 Agent。会话由 runtime 持有,壳子经 `X-Bm-Session` 请求头
//! 寻址续聊;历史由 runtime 侧维护,壳子只需传增量最后一条 user 消息
//! (W1 合同口径;原 W1 规格 §4 溯 git 史,ADR-0027)。
//! 鉴权 = 路由层 auth::require_api_auth(issue #10 断链补立:Bearer 严格
//! 失败/门户 Cookie/本机未设墙放行;原「免鉴权欠账」就此销账)。

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
use std::time::{Duration, Instant};

fn unix_now() -> i64 {
    crate::unix_now() as i64
}

fn err_response(status: StatusCode, message: &str) -> Response {
    err_response_ext(status, None, message)
}

/// 结构化错误响应(issue #40):code 携带扩展码
/// (registry/extensions/webui.json 命名空间),前端按码分支,不再串
/// 匹配文案;None = 无扩展码,形状与旧响应逐位一致。
fn err_response_ext(status: StatusCode, code: Option<&str>, message: &str) -> Response {
    let mut err = serde_json::json!({ "message": message, "type": "invalid_request_error" });
    if let Some(code) = code {
        err["code"] = serde_json::json!(code);
    }
    (status, Json(serde_json::json!({ "error": err }))).into_response()
}

/// GET /v1/models:服务器当前配置的模型(单配置模型,W1 口径)。
pub async fn models(State(state): State<AppState>) -> Response {
    Json(serde_json::json!({
        "object": "list",
        "data": [ { "id": *state.default_model, "object": "model" } ]
    }))
    .into_response()
}

/// completion 载荷正文的补发判定(非流式连接器收尾)。
/// `model.content.delta` 现只承载连接器的流式正文(ADR-0055 起内核不再把
/// `[调用 …]`/`[工具完成 …]` 标记混入 delta):正文若已随 delta 下发(流式),
/// 则不再补发(防重复);未出现(非流式连接器只在 completion 带全文),则补发。
/// 历史上标记会撑大「已下发字符数」而把真正文整段丢弃,故判定改按「已下发
/// 文本是否已含全文」——标记既已移除,该判定保持稳健即可。
fn should_backfill_content(sent: &str, content: &str) -> bool {
    !content.is_empty() && !sent.contains(content)
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

 // 断连免疫():
 // 「会话寻址/建会话 + 消息派发」放进独立任务——
 // 遇客户端掉线(axum 掐掉 handler future),会话已建而 send_input 再未发出,
 // 用户刚发出的消息被静默吞掉。任务化后掉线只丢响应,回合照常
 // 诞生、照常完成、落历史。
    let prepared = tokio::spawn(resolve_and_dispatch(
        state.handle.clone(),
        state.v1_sessions.clone(),
        state.store.clone(),
        state.data_dir.clone(),
        default_model.clone(),
        requested_model.clone(),
        requested_workspace.clone(),
        target_role_id.clone(),
        text.clone(),
        headers.clone(),
    ));
 // rt_aid 已随派发写进 v1_sessions 寻址表,响应面只用 sid
    let (rt_sid, _rt_aid, mut cursor) = match prepared.await {
        Ok(Prepared::Ok { sid, aid, cursor }) => (sid, aid, cursor),
        Ok(Prepared::Err(resp)) => return resp,
        Err(join_err) => {
            return err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("回合派发任务失败: {join_err}"),
            );
        }
    };

    let session_header =
        HeaderValue::from_str(rt_sid.as_str()).unwrap_or(HeaderValue::from_static("invalid"));
    let stream = body["stream"].as_bool().unwrap_or(false);

    if !stream {
 // 非流式:轮询聚合到终态一次返回
        let store = state.store.clone();
 // W10(ADR-0024):非流式聚合等待走 limits;ADR-0028:0 = 不限时。
        let wait_ms = state.limits.get().nonstream_wait_ms;
        let deadline = (wait_ms > 0).then(|| Instant::now() + Duration::from_millis(wait_ms));
        loop {
            if deadline.is_some_and(|dl| Instant::now() > dl) {
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

 // 已按 delta 下发的文本(判定 completion 余量用)。
 // `content.chars().skip(emitted)`:内核注入的 `[调用 …]`/`[工具完成 …]`
 // 标记也走 model.content.delta,会把计数撑大,而 completion 载荷的
 // content 只含正文——凡非流式上游 + 工具调用的回合,最终正文被整段
 // skip 丢弃。改按「已下发文本是否已含全文」判定,不依赖字符计数。
        let mut sent = String::new();
 // 流生命周期与回合解耦():原 180s 硬顶会在
 // 长工具阶段中途掐断交互流——此后审批标记再无下发通道(YOLO 失效、
 // ask 无卡片),界面误显「完成」而后端仍在跑。改 900s;keepalive
 // 每 10s 保活前端看门狗,空闲不中断。
 // W10(ADR-0024):流式硬顶走 limits(v0.0.11 起 900s 默认);
 // ADR-0028:0 = 不设硬顶,流与回合同寿。
        let hard_cap_ms = state.limits.get().stream_hard_cap_ms;
        let deadline = (hard_cap_ms > 0).then(|| Instant::now() + Duration::from_millis(hard_cap_ms));
 // 静默保活():工具轮执行期间事件面
 // 可静默 25s+,前端看门狗(60s 无任何字节即中止)会被误杀。空闲超
 // 10s 下发一行 SSE 注释——前端按任意字节重置看门狗,注释行被解析
 // 器忽略,不污染内容。
        let mut last_byte = Instant::now();
 // P1-11():流的真实结局——
 // finish_reason:stop + [DONE],硬顶超时/失败被客户端误认为正常完成。
        let mut outcome = "finished";
        loop {
            if deadline.is_some_and(|dl| Instant::now() > dl) {
                outcome = "timeout";
                break;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
            if last_byte.elapsed() > Duration::from_millis(state.limits.get().stream_keepalive_ms) {
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
                        sent.push_str(delta);
                        last_byte = Instant::now();
                        yield Ok(chunk(&sid, &default_model,
                            serde_json::json!({ "content": delta }), None));
                    }
                    EventType::ModelInvocationCompleted => {
 // 连接器分两态:流式连接器已把正文按 delta 下发(标记也
 // 混在同流中);非流式连接器只在 completion 载荷带全文,
 // 正文从未走过 delta。以「已下发文本是否已含全文」判定:
 // 已含则正文已送达,不补发(流式);未含则补发全文(非流式)。
                        let content = e.payload["content"].as_str().unwrap_or_default();
                        if should_backfill_content(&sent, content) {
                            yield Ok(chunk(&sid, &default_model,
                                serde_json::json!({ "content": content }), None));
                        }
                        finished = true;
                        break;
                    }
                    EventType::AgentFailed | EventType::AgentCancelled => {
                        yield Ok(chunk(&sid, &default_model,
                            serde_json::json!({ "content": "\n[回合失败或已取消]" }), None));
                        outcome = if e.event_type == EventType::AgentCancelled {
                            "cancelled"
                        } else {
                            "failed"
                        };
                        finished = true;
                        break;
                    }
                    EventType::AgentInterrupted => {
                        outcome = "interrupted";
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
 // 收尾按真实结局分路:仅正常完成才发 stop + [DONE];失败/取消/中断/
 // 超时发 OpenAI 兼容错误帧后原样断流(不发 [DONE] 谎报完成)。
        if outcome == "finished" {
            yield Ok(Bytes::from(
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            ));
            yield Ok(Bytes::from("data: [DONE]\n\n"));
        } else {
            let (message, code) = match outcome {
                "cancelled" => ("回合已被用户取消".to_string(), "turn_cancelled".to_string()),
                "failed" => ("回合执行失败".to_string(), "turn_failed".to_string()),
                "interrupted" => (
                    "回合被中断(服务重启恢复边界)".to_string(),
                    "turn_interrupted".to_string(),
                ),
                _ => (
                    format!(
                        "流式硬顶({}ms)到时断流,回合可能仍在后台执行",
                        state.limits.get().stream_hard_cap_ms
                    ),
                    "stream_hard_cap_exceeded".to_string(),
                ),
            };
            let err = serde_json::json!({
                "error": {
                    "message": message,
                    "type": "server_error",
                    "param": serde_json::Value::Null,
                    "code": code,
                }
            });
            yield Ok(Bytes::from(format!("data: {err}\n\n")));
        }
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

/// 会话寻址(续聊 / 新建)+ 消息派发。独立任务:客户端掉线只丢响应,回合照常
/// 诞生、完成、落历史()。
/// 自 `chat_completions` 抽出(消 462 行巨型 handler;纯机械移动,零行为变更)。
#[allow(clippy::too_many_arguments)]
async fn resolve_and_dispatch(
    handle: bm_core::runtime::RuntimeHandle,
    v1_sessions: std::sync::Arc<std::sync::Mutex<crate::V1SessionMap>>,
    store: std::sync::Arc<dyn bm_core::ports::persist::EventStore>,
    data_dir: Option<std::path::PathBuf>,
    default_model: String,
    requested_model: Option<String>,
    requested_workspace: Option<String>,
    target_role_id: Option<String>,
    text: String,
    headers: axum::http::HeaderMap,
) -> Prepared {
 // 会话寻址:有 X-Bm-Session 续聊;无则新建(默认配置模型)
    let resolved: Result<(BmId, BmId), Response> = match headers
        .get("x-bm-session")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
    {
        Some(raw) => match BmId::parse(raw) {
            Ok(sid) => {
 // 先取克隆并让锁守卫出作用域(不得跨 await 持锁)
                let cached = v1_sessions.lock().expect("锁未中毒").get(&sid).cloned();
                match cached {
                    Some(aid) => Ok((sid, aid)),
                    None => {
 // 重启续聊():v1_sessions 是进程内寻址表,
 // 重启即空;会话本体自持久层装载并未丢——回源
 // session.resume 恢复寻址,旧会话继续聊,不再 400 逼重开。
 // since_seq=MAX:寻址回源不需要补发事件
                        match handle
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
                                v1_sessions
                                    .lock()
                                    .expect("锁未中毒")
                                    .insert(sid.clone(), r.agent_id.clone());
                                Ok((sid, r.agent_id))
                            }
                            Err(_) => Err(err_response_ext(
                                StatusCode::BAD_REQUEST,
                                Some("webui.session_unknown"),
                                "未知会话:请清除界面会话记忆后重新开始",
                            )),
                        }
                    }
                }
            }
            Err(_) => Err(err_response(
                StatusCode::BAD_REQUEST,
                "X-Bm-Session 不是合法会话 id",
            )),
        },
        None => {
            let request_id = UlidIdGen.next_id("req");
 // W4b:允许通过 X-Bm-Role 指定角色(缺省 = active 角色);
 // system_prompt 由 bm-core::roles 统一组装(含挂载技能),
 // 空提示词传 None——交由回合侧热读,保证技能/角色后续可生效。
            let initial_system_prompt = data_dir.as_ref().and_then(|d| {
                bm_core::roles::compose_role_prompt(d, target_role_id.as_deref())
                    .filter(|s| !s.is_empty())
            });
 // F1(ADR-0022 后续批):角色工具白名单随角色烤入会话;未声明 =
 // None(全量挂载,向后兼容)。
            let initial_allowed_tools = data_dir
                .as_ref()
                .and_then(|d| bm_core::roles::allowed_tools_for(d, target_role_id.as_deref()));
            match handle
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
                            allowed_tools: initial_allowed_tools,
                            workspace_id: requested_workspace.clone(),
                        },
                    },
                )
                .await
            {
                Ok(r) => {
                    v1_sessions
                        .lock()
                        .expect("锁未中毒")
                        .insert(r.session_id.clone(), r.agent_id.clone());
                    Ok((r.session_id, r.agent_id))
                }
                Err(e) => Err(err_response_ext(
                    StatusCode::BAD_REQUEST,
                    e.ext_code(),
                    &format!("会话创建失败: {}", e.to_wire().message),
                )),
            }
        }
    };
    let (rt_sid, rt_aid) = match resolved {
        Ok(pair) => pair,
        Err(resp) => return Prepared::Err(resp),
    };

 // 发送前取日志末位,作为本回合的事件轮询游标(空日志/首启文件未建 = 0)
    let cursor = store.last_log_seq().unwrap_or(0);
    let request_id = UlidIdGen.next_id("req");
    let sent = handle
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
    match sent {
        Ok(_) => Prepared::Ok {
            sid: rt_sid,
            aid: rt_aid,
            cursor,
        },
 // W8:校验类失败(如工作区未登记)按 400 透出,便于壳子清理本地选择;
 // 扩展码(issue #40)随 error.code 透出,前端按码精确分支
        Err(e) => {
            let wire = e.to_wire();
            let status = if wire.code.get() == bm_contract::error_codes::ErrorCode::ValidationFailed
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            Prepared::Err(err_response_ext(status, e.ext_code(), &wire.message))
        }
    }
}

enum Prepared {
    Ok { sid: BmId, aid: BmId, cursor: u64 },
    Err(Response),
}

#[cfg(test)]
mod backfill_tests {
    use super::should_backfill_content;

 #[test]
    fn backfill_only_when_content_not_yet_sent() {
 // 流式:正文已随 delta 下发(标记混在同流)→ 不补发,防重复
        assert!(!should_backfill_content(
            "\n[调用 fs.read a.txt]\n正文一\n正文二",
            "正文一\n正文二"
        ));
 // 非流式 + 工具回合:已下发仅标记,正文未出现 → 补发
        assert!(should_backfill_content(
            "\n[调用 skill.skill_demo.echo]\n\n[工具完成 skill.skill_demo.echo 耗时 430ms]\n",
            "技能执行完成(e2e 验收)。"
        ));
 // 空正文不补发
        assert!(!should_backfill_content("任意已下发", ""));
    }
}
