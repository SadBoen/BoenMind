//! 基础设施端口：Fs / Shell / SessionPersist / PluginRuntime。
//!
//! fail-loud 纪律：端口是可探测能力，未装配时返回
//! `PluginRuntimeAvailability::Unavailable{reason}`，调用方必须显式处理。

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::error::{PortError, PortResult};
use crate::session::{SessionEvent, SessionHeader};

/// 文件系统端口（工作区路径守卫：resolve 越界即 PermissionDenied）。
#[async_trait]
pub trait FsPort: Send + Sync {
    fn workspace_root(&self) -> &Path;

    /// 把用户提供的路径解析为工作区内绝对路径；越界返回 PermissionDenied。
    fn resolve(&self, path: &str) -> PortResult<PathBuf>;

    async fn read_text(&self, path: &Path) -> PortResult<String>;
    async fn write_text(&self, path: &Path, content: &str) -> PortResult<()>;
    async fn exists(&self, path: &Path) -> PortResult<bool>;
}

/// 子进程/Shell 执行请求。
#[derive(Debug, Clone)]
pub struct ShellRequest {
    pub command: String,
    pub cwd: Option<PathBuf>,
    pub timeout: Option<std::time::Duration>,
}

/// 子进程/Shell 执行结果。
#[derive(Debug, Clone)]
pub struct ShellResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Shell/子进程端口。
#[async_trait]
pub trait ShellPort: Send + Sync {
    async fn exec(&self, request: ShellRequest) -> PortResult<ShellResult>;
}

/// 会话持久化端口（事件日志 = 唯一事实源；sessions/messages 为投影）。
#[async_trait]
pub trait SessionPersistPort: Send + Sync {
    /// 原子追加事件批次（fsync 后发布；批次内 seq 连续）。
    async fn append_events(
        &self,
        session_id: &str,
        events: &[SessionEvent],
    ) -> PortResult<()>;

    /// 创建会话（写 header 事件）。
    async fn create_session(&self, header: &SessionHeader) -> PortResult<()>;

    /// 加载会话完整事件日志（按 seq 升序）；不存在返回 None。
    async fn load_events(&self, session_id: &str) -> PortResult<Option<Vec<SessionEvent>>>;

    /// 全量重写会话事件日志（事务内 DELETE + INSERT）。
    /// 用于 interrupted-turn 修复落盘：kill -9 恢复时把修剪后的完整日志写回。
    /// 默认实现：能力未注册（fail-loud）。
    async fn rewrite_events(
        &self,
        _session_id: &str,
        _events: &[SessionEvent],
    ) -> PortResult<()> {
        Err(PortError::not_available("rewrite_events"))
    }

    /// 列出全部会话 id（按最近活跃排序）。
    async fn list_sessions(&self) -> PortResult<Vec<String>>;

    /// 删除会话。
    async fn delete_session(&self, session_id: &str) -> PortResult<()>;
}

/// 插件运行时可探针性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRuntimeAvailability {
    Unavailable { reason: String },
    Ready,
}

/// 插件运行时端口（M1 为 fail-loud 探针；M3 接 supervisor 完整实现）。
#[async_trait]
pub trait PluginRuntimePort: Send + Sync {
    fn availability(&self) -> PluginRuntimeAvailability;
}

/// 默认未装配插件运行时：任何调用方拿到 Unavailable 必须显式报错。
pub struct UnavailablePluginRuntime;

#[async_trait]
impl PluginRuntimePort for UnavailablePluginRuntime {
    fn availability(&self) -> PluginRuntimeAvailability {
        PluginRuntimeAvailability::Unavailable {
            reason: "plugin runtime is not registered in this delivery profile".into(),
        }
    }
}
