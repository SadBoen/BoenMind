//! Task 族处理器与参数(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

mod autorun;
mod create;
mod lifecycle;
mod members;
mod persist;
mod query;

pub(crate) use autorun::{
    AutorunState, autorun_note_completed, autorun_pump, handle_task_autorun_start,
};

pub(crate) use create::handle_task_create;
pub(crate) use lifecycle::{
    handle_task_budget_increase, handle_task_lifecycle, handle_task_report_completion,
    watchdog_scan_run,
};
pub(crate) use members::{
    handle_butler_revoke, handle_task_collect, handle_task_remove_member, handle_task_spawn_member,
    handle_task_spawn_subtask, handle_worker_call,
};
pub use persist::{RemoveMemberParams, SpawnMemberParams, SpawnSubtaskParams, WorkerCallParams};
pub(crate) use persist::{persist_task, task_contract_json};

pub(crate) use query::{handle_task_get, handle_task_list, task_error_to_core};

impl World {
    /// 到期自动扫描(核心循环节拍)。
    pub(crate) fn maybe_watchdog_scan(&mut self) {
        let now = self.config.clock.now();
        if !self.watchdog.due(now) {
            return;
        }
        watchdog_scan_run(self);
        self.watchdog.schedule_next(now);
    }

    /// 手动扫描(诊断/测试入口):忽略节拍,直接执行并重排下次。
    pub(crate) fn watchdog_scan_now(&mut self) -> usize {
        let now = self.config.clock.now();
        let n = watchdog_scan_run(self);
        self.watchdog.schedule_next(now);
        n
    }
}
pub(crate) fn emit_autorun(
    w: &mut World,
    task_id: &BmId,
    phase: &str,
    turn: u64,
    reason: Option<&str>,
) {
    w.emit(
        EventType::TaskAutorunStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "phase": phase,
            "turn": turn,
            "reason": reason,
        }),
    );
}
