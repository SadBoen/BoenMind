//! SSE 事件流(M3.3 watch):GET /events/{session_id}?since_seq=N。
//! 服务端零订阅状态:增量自持久日志轮询补发(id = event_seq),
//! 断线重连完全由客户端 since_seq 驱动(M3 规格 §5.2)。

use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use bm_contract::ids::BmId;
use std::convert::Infallible;
use std::time::Duration;

#[derive(serde::Deserialize)]
pub struct EventsQuery {
    pub since_seq: Option<u64>,
}

/// 200ms 增量轮询 + 15s 心跳注释行(transport 合同 sse_frame)。
pub async fn events_sse(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> Response {
    let Ok(sess) = BmId::parse(session_id) else {
        return (axum::http::StatusCode::BAD_REQUEST, "非法 session_id").into_response();
    };
    let store = state.store.clone();
    let mut last = q.since_seq.unwrap_or(0);

    let stream = async_stream::stream! {
        let mut tick = tokio::time::interval(Duration::from_millis(150));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    match store.replay_since(last) {
                        Ok(events) => {
                            for e in events {
                                last = e.event_seq;
                                // 外部审计 X-02(P1):按会话过滤——只发本会话
                                // 关联事件;无会话关联的全局事件不进会话流。
                                if e.session_id.as_ref() != Some(&sess) {
                                    continue;
                                }
                                let data = serde_json::to_string(&e)
                                    .unwrap_or_else(|_| "{}".to_string());
                                yield Ok::<_, Infallible>(
                                    Event::default()
                                        .id(last.to_string())
                                        .event("envelope")
                                        .data(data),
                                );
                            }
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, session = %sess, "SSE 增量读取失败");
                        }
                    }
                }
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("keepalive"));
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
