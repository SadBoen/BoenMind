//! WS 下行流（契约台账 §1 面 2/3）：/api/events.mux 与 /api/events.host。
//! 均为 downlink-only：收到任意客户端消息 → close(1008, 'downlink only')。

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::response::IntoResponse;
use serde_json::json;

use crate::api::AppState;
use crate::rpc::ServerRequestFrame;

/// mux 流：订阅 bus，把实时 wire 事件包成 session/event 帧下行。
pub async fn mux_loop(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.events_tx.subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok(wire) => {
                        let frame = ServerRequestFrame::new(
                            uuid::Uuid::new_v4().to_string(),
                            "session/event",
                            json!({
                                "sessionId": "",
                                "event": wire,
                            }),
                        );
                        let text = serde_json::to_string(&frame).unwrap_or_default();
                        if socket.send(Message::Text(axum::extract::ws::Utf8Bytes::from(text))).await.is_err() {
                            return;
                        }
                    }
                    Err(_) => continue,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) => return,
                    Some(Ok(_)) => {
                        let _ = socket.send(Message::Close(Some(
                            axum::extract::ws::CloseFrame {
                                code: 1008,
                                reason: "downlink only".into(),
                            }
                        ))).await;
                        return;
                    }
                    Some(Err(_)) => return,
                    None => return,
                }
            }
        }
    }
}

/// host 流：连接保持（recv 挂起），收到任意上行消息 → close(1008, 'downlink only')。
pub async fn host_loop(mut socket: WebSocket, _state: Arc<AppState>) {
    match socket.recv().await {
        None | Some(Ok(Message::Close(_))) | Some(Err(_)) => {}
        Some(Ok(_)) => {
            let _ = socket
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 1008,
                    reason: "downlink only".into(),
                })))
                .await;
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
                    let frame = ServerRequestFrame::new(
                        uuid::Uuid::new_v4().to_string(),
                        "session/event",
                        json!({ "sessionId": "", "event": wire }),
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
