//! MCP 子进程 OS 级资源上限(ADR-0035 §4,issue #55)。
//!
//! 现状杠杆只有 env 白名单 + kill_on_drop + Broker 审批门;恶意/失控插件可
//! 无限占 CPU/内存。本 crate 施加**尽力而为**的 OS 级上限:
//!
//! - **Windows**:Job Object(进程内存上限 + 活动进程数 + KILL_ON_JOB_CLOSE,
//!   句柄关闭即杀子进程)。
//! - **Unix**:`setrlimit`(`RLIMIT_AS` 地址空间 / `RLIMIT_CPU`),经
//!   `pre_exec` 在 exec 前施加。
//!
//! 统一门面:spawn 前 [`pre_spawn`]、spawn 后 [`post_spawn`] 返回 [`SandboxGuard`]。
//! 调用方须持有该守卫至子进程结束(Windows KILL_ON_JOB_CLOSE 语义)。
//!
//! 纪律:crate 级 `unsafe_code = "deny"`,平台模块局部 `#[allow(unsafe_code)]`;
//! 其余 crate 维持 workspace `forbid` 不动。施加失败只告警不阻断(fail-open,
//! 与「单插件失败不中止装载」既有语义一致)。

/// 子进程资源上限。`0` = 该项不限制。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SandboxLimits {
    /// 进程地址空间上限(字节)。
    pub memory_bytes: u64,
    /// CPU 时间上限(秒;Unix `RLIMIT_CPU`,累计语义)。
    pub cpu_seconds: u64,
    /// 允许的活动进程数上限(Windows 语义;Unix 忽略)。
    pub active_processes: u32,
}

impl SandboxLimits {
    /// 三项皆不限 = 空操作,调用方可跳过施加。
    pub fn is_noop(&self) -> bool {
        self.memory_bytes == 0 && self.cpu_seconds == 0 && self.active_processes == 0
    }
}

/// 平台守卫。Windows 持有 Job Object 句柄;其余平台为空值。
/// 字段刻意不读——持有即为 Drop 语义(KILL_ON_JOB_CLOSE),故 allow(dead_code)。
pub struct SandboxGuard {
    #[cfg(windows)]
    #[allow(dead_code)] // 持有即语义:drop 时关闭 Job 句柄终止子进程
    inner: Option<imp::JobGuard>,
}

/// spawn 前施加(目前仅 Unix 的 `setrlimit` 经 `pre_exec`)。失败返回 Err,
/// 调用方告警后可继续(不加限)。
pub fn pre_spawn(
    #[cfg_attr(not(unix), allow(unused_variables))] cmd: &mut tokio::process::Command,
    #[cfg_attr(not(unix), allow(unused_variables))] limits: &SandboxLimits,
) -> Result<(), String> {
    // 平台条件表达式(非 `#[cfg]` 块 + `return`):此前 unix 分支的 `return`
    // 在 Linux/macOS 上被 clippy 判 `needless_return`(CI 红),而 Windows 因该
    // 分支被 cfg 剔除不触发——本地绿、三平台矩阵红的经典陷阱。
    #[cfg(unix)]
    {
        imp::harden_command(cmd, limits)
    }
    #[cfg(not(unix))]
    {
        Ok(())
    }
}

/// spawn 后施加(Windows Job Object)。失败返回空守卫并告警由调用方负责。
pub fn post_spawn(pid: u32, limits: &SandboxLimits) -> SandboxGuard {
    #[cfg(windows)]
    {
        match imp::harden_child(pid, limits) {
            Ok(g) => SandboxGuard { inner: Some(g) },
            Err(e) => {
                eprintln!("[sandbox] 子进程 {pid} Job Object 施加失败(继续,不加限): {e}");
                SandboxGuard { inner: None }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, limits);
        SandboxGuard {}
    }
}

// ---- Windows:Job Object -----------------------------------------------------

#[cfg(windows)]
#[allow(unsafe_code)] // ADR-0035:unsafe 仅限本平台模块
mod imp {
    use super::SandboxLimits;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// Job Object 句柄守卫。`KILL_ON_JOB_CLOSE` 下 drop 即关闭句柄并终止子进程。
    pub struct JobGuard {
        job: HANDLE,
    }

    // SAFETY:HANDLE 是内核对象句柄(值语义),跨线程移动安全;仅本守卫关闭。
    unsafe impl Send for JobGuard {}
    unsafe impl Sync for JobGuard {}

    impl Drop for JobGuard {
        fn drop(&mut self) {
            if !self.job.is_null() {
                // SAFETY:句柄由 CreateJobObjectW 成功返回,且仅在此处关闭一次。
                unsafe {
                    CloseHandle(self.job);
                }
            }
        }
    }

    /// 把已 spawn 的子进程(pid)纳入 Job Object 并施加限额。
    pub fn harden_child(pid: u32, limits: &SandboxLimits) -> Result<JobGuard, String> {
        if limits.is_noop() {
            return Ok(JobGuard {
                job: std::ptr::null_mut(),
            });
        }
        // SAFETY:全部为 Win32 直接调用;句柄在失败分支配对释放。
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(format!(
                    "CreateJobObject 失败(err={})",
                    std::io::Error::last_os_error()
                ));
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            let mut flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if limits.memory_bytes > 0 {
                flags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.ProcessMemoryLimit = limits.memory_bytes as usize;
            }
            if limits.active_processes > 0 {
                flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
                info.BasicLimitInformation.ActiveProcessLimit = limits.active_processes;
            }
            info.BasicLimitInformation.LimitFlags = flags;
            let set_ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set_ok == FALSE {
                let e = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("SetInformationJobObject 失败(err={e})"));
            }

            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, FALSE, pid);
            if process.is_null() {
                let e = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("OpenProcess({pid}) 失败(err={e})"));
            }
            let assign_ok = AssignProcessToJobObject(job, process);
            CloseHandle(process);
            if assign_ok == FALSE {
                let e = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("AssignProcessToJobObject 失败(err={e})"));
            }
            Ok(JobGuard { job })
        }
    }
}

// ---- Unix:setrlimit(pre_exec)-----------------------------------------------

#[cfg(unix)]
#[allow(unsafe_code)] // ADR-0035:unsafe 仅限本平台模块
mod imp {
    use super::SandboxLimits;

    /// 在命令 exec 前施加 `setrlimit`。
    pub fn harden_command(
        cmd: &mut tokio::process::Command,
        limits: &SandboxLimits,
    ) -> Result<(), String> {
        if limits.is_noop() {
            return Ok(());
        }
        let memory = limits.memory_bytes;
        let cpu = limits.cpu_seconds;
        // SAFETY:pre_exec 闭包在 fork 后 exec 前运行,只可做 async-signal-safe
        // 操作;闭包内仅调用 setrlimit,不分配、不加锁。
        unsafe {
            cmd.pre_exec(move || {
                if memory > 0 {
                    let lim = libc::rlimit {
                        rlim_cur: memory,
                        rlim_max: memory,
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &lim) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                if cpu > 0 {
                    let lim = libc::rlimit {
                        rlim_cur: cpu,
                        rlim_max: cpu,
                    };
                    if libc::setrlimit(libc::RLIMIT_CPU, &lim) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_limits_is_noop() {
        assert!(SandboxLimits::default().is_noop());
        assert!(
            !SandboxLimits {
                memory_bytes: 1,
                ..Default::default()
            }
            .is_noop()
        );
        assert!(
            !SandboxLimits {
                active_processes: 1,
                ..Default::default()
            }
            .is_noop()
        );
    }

    /// Windows:对一个真实短命子进程施加 Job Object 限额应成功;drop 守卫
    /// 触发 KILL_ON_JOB_CLOSE 终止子进程。
    #[cfg(windows)]
    #[test]
    fn post_spawn_applies_job_object() {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.args(["/C", "ping -n 5 127.0.0.1 > NUL"]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut child = cmd.spawn().expect("spawn");
            let limits = SandboxLimits {
                memory_bytes: 512 * 1024 * 1024,
                cpu_seconds: 0,
                active_processes: 16,
            };
            let guard = post_spawn(child.id().expect("pid"), &limits);
            drop(guard);
            let _ = child.wait().await;
        });
    }
}
