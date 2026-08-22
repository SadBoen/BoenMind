//! 宿主能力面（万物皆插件②，2026-08-22）：把 web-server 的会话目录/回合驱动
//! 与三条下行广播通道实现为 bm-ports 端口，供下沉到插件的领域实现
//! （plugin-schedule 调度器、plugin-goal 引擎、plugin-approval 审批中心）消费。
//! web-server 从此只做「端口实现 + wire 协议层」，不再内嵌能力后端。

use std::sync::Arc;

use serde_json::{json, Value};

use bm_ports::{BroadcastPort, SessionDrivePort, TurnFinishHook};

use crate::api::AppState;

/// 宿主能力实现：薄转发 AppState（sessions 目录、三条广播通道、投影表）。
pub struct HostFace {
    state: Arc<AppState>,
}

impl std::fmt::Debug for HostFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostFace").finish_non_exhaustive()
    }
}

impl HostFace {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

impl SessionDrivePort for HostFace {
    fn session_exists(&self, session_id: &str) -> bool {
        self.state.sessions.lock().unwrap().contains_key(session_id)
    }

    fn active_session(&self) -> Option<String> {
        let sessions = self.state.sessions.lock().unwrap();
        sessions
            .iter()
            .find(|(_, h)| h.running || !h.blank)
            .map(|(id, _)| id.clone())
    }

    fn spawn_turn(
        &self,
        session_id: &str,
        prompt: &str,
        on_finish: Option<TurnFinishHook>,
    ) -> bool {
        // 锁内原子占用：忙/不存在 → false（不排队，防叠加）。
        let agent = {
            let mut sessions = self.state.sessions.lock().unwrap();
            let Some(h) = sessions.get_mut(session_id) else {
                return false;
            };
            if h.running {
                return false;
            }
            h.running = true;
            h.blank = false;
            Arc::clone(&h.agent)
        };
        let state = Arc::clone(&self.state);
        let sid = session_id.to_string();
        let text = prompt.to_string();
        state.broadcast_host(
            "host/session-status",
            json!({ "sessionId": sid, "running": true }),
        );
        tokio::spawn(async move {
            let _ = agent.run_turn(Some(&text)).await;
            if let Some(h) = state.sessions.lock().unwrap().get_mut(&sid) {
                h.running = false;
            }
            if let Some(hook) = on_finish {
                hook();
            }
            state.broadcast_host(
                "host/session-status",
                json!({ "sessionId": sid, "running": false }),
            );
        });
        true
    }
}

impl BroadcastPort for HostFace {
    fn broadcast_host(&self, method: &str, payload: Value) {
        self.state.broadcast_host(method, payload);
    }

    fn broadcast_mux(&self, rpc_id: String, method: &str, payload: Value) {
        self.state.broadcast_mux_frame(rpc_id, method, payload);
    }

    fn write_projection(&self, session_id: &str, key: &str, value: Value) {
        self.state.write_projection(session_id, key, value);
    }
}
