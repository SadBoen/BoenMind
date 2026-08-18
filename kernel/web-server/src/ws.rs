//! WS 下行流（契约台账 §1 面 2/3）：/api/events.mux 与 /api/events.host。
//! 均为 downlink-only：收到任意客户端消息 → close(1008, 'downlink only')。

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::response::IntoResponse;
use serde_json::json;

use crate::api::AppState;
use crate::rpc::ServerRequestFrame;

/// mux 流：连接时发 subscribed 基线（每 attached session 一帧）+ 重放仍 pending 的
/// approval/question（rpcId 原样复用），然后订阅 bus 把实时 wire 事件包成
/// session/event 帧下行 + mux_events_tx 的额外帧（approval/question 重放、projection）。
pub async fn mux_loop(mut socket: WebSocket, state: Arc<AppState>) {
    tracing::info!("mux connected");
    // Open 基线（台账 §4 断连恢复）：每 attached session 一帧 session/subscribed，
    // lastSeq = 日志 wire 长度 - 1（空日志 -1）。前端以此订阅会话事件流。
    let sessions: Vec<String> = state
        .sessions
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, h)| h.running || !h.blank)
        .map(|(id, _)| id.clone())
        .collect();
    for sid in sessions {
        let last_seq = match state.runtime.persist.load_events(&sid).await {
            Ok(Some(records)) => {
                let events: Vec<kernel_contracts::session::SessionEvent> =
                    records.into_iter().map(|r| r.event).collect();
                let wire_count = crate::events::translate_events(&events).len() as i64;
                wire_count - 1
            }
            _ => -1,
        };
        let frame = ServerRequestFrame::new(
            uuid::Uuid::new_v4().to_string(),
            "session/subscribed",
            json!({ "sessionId": sid, "lastSeq": last_seq }),
        );
        let text = serde_json::to_string(&frame).unwrap_or_default();
        if socket
            .send(Message::Text(axum::extract::ws::Utf8Bytes::from(text)))
            .await
            .is_err()
        {
            return;
        }
    }

    // 重放仍 pending 的 approval/requested 与 question/requested（rpcId 原样复用）。
    let replay: Vec<ServerRequestFrame> = {
        let reg = state.pending.lock();
        let mut frames: Vec<ServerRequestFrame> = reg
            .approvals
            .values()
            .map(|p| reg.approval_frame(p))
            .collect();
        frames.extend(reg.questions.values().map(|q| reg.question_frame(q)));
        frames
    };
    for frame in replay {
        let text = serde_json::to_string(&frame).unwrap_or_default();
        if socket
            .send(Message::Text(axum::extract::ws::Utf8Bytes::from(text)))
            .await
            .is_err()
        {
            return;
        }
    }

    let mut rx = state.events_tx.subscribe();
    let mut mux_rx = state.mux_events_tx.subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok(wire) => {
                        // wire 事件信封已带 sessionId/seq（AppState::attach_event_bus 在
                        // 发送前填入；见 api.rs）。
                        let frame = ServerRequestFrame::new(
                            uuid::Uuid::new_v4().to_string(),
                            "session/event",
                            wire,
                        );
                        let text = serde_json::to_string(&frame).unwrap_or_default();
                        if socket.send(Message::Text(axum::extract::ws::Utf8Bytes::from(text))).await.is_err() {
                            tracing::info!("mux send failed; closing");
                            return;
                        }
                    }
                    Err(_) => continue,
                }
            }
            ev = mux_rx.recv() => {
                match ev {
                    Ok(frame) => {
                        let text = serde_json::to_string(&frame).unwrap_or_default();
                        if socket.send(Message::Text(axum::extract::ws::Utf8Bytes::from(text))).await.is_err() {
                            tracing::info!("mux send failed; closing");
                            return;
                        }
                    }
                    Err(_) => continue,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("mux closed by peer");
                        return;
                    }
                    Some(Ok(_)) => {
                        let _ = socket.send(Message::Close(Some(
                            axum::extract::ws::CloseFrame {
                                code: 1008,
                                reason: "downlink only".into(),
                            }
                        ))).await;
                        tracing::info!("mux rejected uplink; closing 1008");
                        return;
                    }
                    Some(Err(_)) => {
                        tracing::info!("mux recv error");
                        return;
                    }
                    None => {
                        tracing::info!("mux socket closed");
                        return;
                    }
                }
            }
        }
    }
}

/// host 流（契约台账 §3.1 HostFrame）：连接时发基线帧（workspace-changed 全量 +
/// 每 session 一帧 session-added + running 状态），然后订阅 host_events_tx 转发实时帧。
/// 收到任意上行消息 → close(1008, 'downlink only')。
pub async fn host_loop(mut socket: WebSocket, state: Arc<AppState>) {
    tracing::info!("host connected");

    // Open 基线（台账 §4：host 流无持久化基线，重连侧由前端 workspace.list 打底；
    // 这里提供完整快照帧，前端可据此重建会话列表/工作区状态）。
    let snapshot = state.workspace_snapshot();
    let frame = ServerRequestFrame::new(
        uuid::Uuid::new_v4().to_string(),
        "host/workspace-changed",
        snapshot,
    );
    let text = serde_json::to_string(&frame).unwrap_or_default();
    if socket
        .send(Message::Text(axum::extract::ws::Utf8Bytes::from(text)))
        .await
        .is_err()
    {
        return;
    }

    // 每 session 一帧 session-added（blank + running 状态）。
    let sessions: Vec<(String, bool, bool)> = state
        .sessions
        .lock()
        .unwrap()
        .iter()
        .map(|(id, h)| (id.clone(), h.blank, h.running))
        .collect();
    tracing::info!("host baseline sessions: {:?}", sessions);
    for (sid, blank, running) in sessions {
        let frame = ServerRequestFrame::new(
            uuid::Uuid::new_v4().to_string(),
            "host/session-added",
            json!({ "sessionId": sid, "blank": blank }),
        );
        let text = serde_json::to_string(&frame).unwrap_or_default();
        if socket
            .send(Message::Text(axum::extract::ws::Utf8Bytes::from(text)))
            .await
            .is_err()
        {
            return;
        }
        let frame = ServerRequestFrame::new(
            uuid::Uuid::new_v4().to_string(),
            "host/session-status",
            json!({ "sessionId": sid, "running": running }),
        );
        let text = serde_json::to_string(&frame).unwrap_or_default();
        if socket
            .send(Message::Text(axum::extract::ws::Utf8Bytes::from(text)))
            .await
            .is_err()
        {
            return;
        }
    }

    let mut rx = state.host_events_tx.subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok((method, payload)) => {
                        let frame = ServerRequestFrame::new(
                            uuid::Uuid::new_v4().to_string(),
                            method,
                            payload,
                        );
                        let text = serde_json::to_string(&frame).unwrap_or_default();
                        if socket.send(Message::Text(axum::extract::ws::Utf8Bytes::from(text))).await.is_err() {
                            tracing::info!("host send failed; closing");
                            return;
                        }
                    }
                    Err(_) => continue,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("host closed by peer");
                        return;
                    }
                    Some(Ok(_)) => {
                        let _ = socket.send(Message::Close(Some(
                            axum::extract::ws::CloseFrame {
                                code: 1008,
                                reason: "downlink only".into(),
                            }
                        ))).await;
                        tracing::info!("host rejected uplink; closing 1008");
                        return;
                    }
                    Some(Err(_)) => {
                        tracing::info!("host recv error");
                        return;
                    }
                    None => {
                        tracing::info!("host socket closed");
                        return;
                    }
                }
            }
        }
    }
}

/// SSE 备选（面 9 简版）：开流注释 + 订阅 bus 转发 data 帧。
pub async fn handle_mux_sse(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> impl IntoResponse {
    use axum::body::Body;
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
    use axum::http::{HeaderMap, StatusCode};

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    headers.insert(CACHE_CONTROL, "no-cache".parse().unwrap());

    let (body_tx, body_rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(64);
    let mut rx = state.events_tx.subscribe();
    let _ = body_tx.send(Ok(": connected\n\n".to_string())).await;

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(wire) => {
                    // wire 已是 {sessionId, event} 完整帧 payload。
                    let frame = ServerRequestFrame::new(
                        uuid::Uuid::new_v4().to_string(),
                        "session/event",
                        wire,
                    );
                    let text = format!("data: {}\n\n", serde_json::to_string(&frame).unwrap_or_default());
                    if body_tx.send(Ok(text)).await.is_err() {
                        return;
                    }
                }
                Err(_) => continue,
            }
        }
    });

    (
        StatusCode::OK,
        headers,
        Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(body_rx)),
    )
        .into_response()
}

/// SSE 备选 host 流（简版）：连接保持，只发注释行。
pub async fn handle_host_sse() -> impl IntoResponse {
    use axum::body::Body;
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
    use axum::http::{HeaderMap, StatusCode};

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, "text/event-stream".parse().unwrap());
    headers.insert(CACHE_CONTROL, "no-cache".parse().unwrap());
    let stream = futures::stream::iter(vec![Ok::<_, std::io::Error>(": connected\n\n".to_string())]);
    (StatusCode::OK, headers, Body::from_stream(stream)).into_response()
}

/// 非升级 GET → 426 upgrade required（台账 §1 面 3）。
pub fn upgrade_required() -> impl IntoResponse {
    use axum::http::{HeaderMap, StatusCode};
    let mut headers = HeaderMap::new();
    headers.insert("connection", "Upgrade".parse().unwrap());
    headers.insert("upgrade", "websocket".parse().unwrap());
    (StatusCode::UPGRADE_REQUIRED, headers)
}
