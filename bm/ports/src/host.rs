//! 宿主能力端口（产品契约层扩展）。
//!
//! 宿主文件工具（plugin-host-tools）的 workdir 事实源在宿主（web-server 的
//! AppState.settings），但工具插件不能依赖 L0、也不能直接读 settings 文件。
//! 故把"当前工作目录"抽象为端口：外层（web-server）实现并从 settings 现读，
//! 装配方（bm-assembly）注入；工具执行时经端口现读当前值——设置页改 workdir
//! 后**下一工具调用即时生效**（与 host.* RPC 的 settings 事实源同语义）。
//!
//! 依赖纪律同 compactor：本 crate 只依赖 kernel-contracts（纯契约）。

use std::path::PathBuf;

/// 当前工作目录源（宿主工具执行时现读；None = 未配置 workdir）。
pub trait WorkdirPort: Send + Sync + std::fmt::Debug {
    /// 当前 workdir 绝对路径（settings host.workdir 的投影；None = 未设置）。
    fn current_workdir(&self) -> Option<PathBuf>;
}