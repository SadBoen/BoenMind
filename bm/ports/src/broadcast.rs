//! 广播/投影端口（插件消费面 / 宿主实现面）。
//!
//! 万物皆插件②（2026-08-22）：goal 引擎（plugin-goal）与审批中心
//! （plugin-approval）需要向客户端广播事件、写会话投影，但不应依赖宿主的
//! 广播通道细节。本端口把宿主三条下行通道收拢：
//! - [`BroadcastPort::broadcast_host`]：host 流（会话状态等工作台事件）。
//! - [`BroadcastPort::broadcast_mux`]：mux 流额外帧（approval/question、投影）。
//! - [`BroadcastPort::write_projection`]：会话投影（seq 递增 + mux 广播）。

use serde_json::Value;

pub trait BroadcastPort: Send + Sync + std::fmt::Debug {
    /// host 流广播（HostFrame 下行）。
    fn broadcast_host(&self, method: &str, payload: Value);

    /// mux 流额外帧广播（MuxFrame 下行；rpc_id 由调用方生成/复用）。
    fn broadcast_mux(&self, rpc_id: String, method: &str, payload: Value);

    /// 会话投影写入（seq 递增；实现负责 mux 广播 session/projection 帧）。
    fn write_projection(&self, session_id: &str, key: &str, value: Value);
}
