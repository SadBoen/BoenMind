//! bm-core:M1 Runtime Core(L2)。Event Bus 单写者、Session/Agent/Operation
//! 三台状态机、模型调用降级链、Execution Log、预算三强制点。
//!
//! 契约:事件只在核心循环内发射(event_seq 单调由单写者分配,INV-3);
//! 连接器/Secret Store 是可替换端口(基线 5.4);预算拒绝不创建 operation
//! (规格 §8.2);`not_started→running` 由收据承载、不发事件(规格 §8.1)。

pub mod approval;
pub mod broker;
pub mod budget;
pub mod bus;
pub mod butler;
pub mod clock;
pub mod context_log;
pub mod coordinator;
pub mod error;
pub mod exec_log;
pub mod memory;
pub mod observation;
pub mod ports;
pub mod registry;
pub mod roles;
pub mod runtime;
pub mod state;
pub mod task;
pub mod team;
pub mod watchdog;
pub mod workspace;

pub use approval::{ApprovalError, ApprovalManager, OpenApproval, RespondDecision};
pub use broker::{
    Broker, CallContext, CallCredential, CallOutcome, Decision, DenyReason, GrantLedger, Lease,
    LeaseError,
};
pub use bus::EventBus;
pub use butler::{
    BOOTSTRAP_ISSUER, BUTLER_PRINCIPAL, CoordinationClass, bootstrap_grant, bootstrap_parent_hash,
    materialize_missing, verb_class,
};
pub use clock::{Clock, MockClock, SystemClock};
pub use coordinator::{COORDINATOR_PRINCIPAL, WORKER_PRINCIPAL, intersection_grants};
pub use error::{CoreError, CoreResult};
pub use memory::memory_capabilities;
pub use observation::{ObservationEntry, expect_satisfied};
pub use registry::{BindingStatus, CapabilityDiscovery, CapabilityProvider, CapabilityRegistry};
pub use runtime::{RuntimeConfig, RuntimeHandle};
pub use task::{MemberRole, Task, TaskBoard, TaskBoardEntry, TaskError, TaskMember};
pub use team::{
    MAX_CONCURRENT_WORKERS, MAX_DELEGATION_DEPTH, authorization_subset, budget_ok, coord_principal,
    depth_ok, max_tool_calls_of, worker_principal,
};
pub use watchdog::{REPEAT_THRESHOLD, ScanDecision, TaskWatch, WATCHDOG_TICK_MS, WatchdogState};
