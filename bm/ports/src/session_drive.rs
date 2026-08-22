//! 会话回合驱动端口（插件消费面 / 宿主实现面）。
//!
//! 万物皆插件②（2026-08-22）：scheduler（plugin-schedule）与 goal 续跑驱动
//! （plugin-goal）需要驱动宿主侧的会话回合，但不应依赖宿主的具体状态类型。
//! 本端口把「查会话目录 + 原子占用 + 异步驱动一个回合」收拢为宿主能力面：
//! - [`SessionDrivePort::session_exists`] / [`SessionDrivePort::active_session`]：
//!   目标会话解析（指定 id 存在性 / 缺省取第一个活跃会话）。
//! - [`SessionDrivePort::spawn_turn`]：锁内原子判定（忙/不存在 → false），置
//!   running/blank、广播 host/session-status(true)、spawn run_turn、复位 running、
//!   调用 on_finish 钩子、广播 running(false)。on_finish 供调用方挂接续跑门
//!   释放等回合级收尾（scheduler 传 None——定时触发的回合不触发 goal 续跑，
//!   仅人类 prompt 完成点续跑，语义与拆分前一致）。

/// 回合完成钩子：running 复位后、结束广播前调用（一次）。
pub type TurnFinishHook = Box<dyn FnOnce() + Send + Sync + 'static>;

pub trait SessionDrivePort: Send + Sync + std::fmt::Debug {
    /// 会话是否存在（目标会话解析用）。
    fn session_exists(&self, session_id: &str) -> bool;

    /// 当前活跃会话（running 或非 blank 的第一个；无 → None）。
    fn active_session(&self) -> Option<String>;

    /// 原子占用并异步驱动一个回合。忙或不存在 → false（不排队，防叠加）；
    /// 成功 → true，回合生命周期（广播/spawn/复位）由实现承担，
    /// `on_finish` 在回合完成、running 复位后调用。
    fn spawn_turn(
        &self,
        session_id: &str,
        prompt: &str,
        on_finish: Option<TurnFinishHook>,
    ) -> bool;
}
