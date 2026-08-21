//! # plugin-host-tools —— 宿主文件工具插件（核心分类）。
//!
//! 把 workdir 作用域的宿主能力注册为 Agent 工具（ToolRegistry 装配面）：
//! - `host.read_file`   读 workdir 内文本文件（≤ 2MiB，utf-8）
//! - `host.write_file`  写 workdir 内文本文件（原子写，overwrite 参数）
//! - `host.list_dir`    列 workdir 内目录条目（目录优先 + 名字排序，≤ 2000 截断）
//! - `host.run_command` 在 workdir 内执行 shell 命令（Windows cmd /C，其他 sh -c；30s 超时）
//!
//! 安全语义与 web-server 的 host.* RPC 端点**同源**（共用 `host-fs::resolve_in_workdir`
//! 越界防护）：路径一律经 workdir 作用域解析，逃逸即拒；run_command 的 cwd 同样
//! 先经 resolve_in_workdir 校验。
//!
//! workdir 事实源 = [`WorkdirPort`]（bm-ports 产品契约）：外层实现并从 settings 现读，
//! 装配方（bm-assembly）经 [`set_workdir_source`] 注入全局源，工具执行时现读——
//! 设置页改 workdir 后**下一工具调用即时生效**（与 host.* RPC 的 settings 事实源同语义）。
//!
//! 接线：装配方调用 [`register_all`] 注册全部工具并 `gate.enable`，之后 plugin-loop
//! 每回合把已启用工具 schema 发给模型，工具调用经 ToolGate 执行。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bm_ports::WorkdirPort;
use kernel_contracts::tools::{
    ToolExecutionInput, ToolExecutionResult, ToolHandler, ToolSchema,
};
use kernel_contracts::ToolError;
use tokio::process::Command as AsyncCommand;

pub mod plugin;

pub use plugin::manifest;

/// 命令执行超时（跑飞命令/死循环保护）。超时 → 工具执行错误。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// 宿主工具 id 常量（前端回显/门控白名单引用）。
pub const HOST_READ_FILE: &str = "host.read_file";
pub const HOST_WRITE_FILE: &str = "host.write_file";
pub const HOST_LIST_DIR: &str = "host.list_dir";
pub const HOST_RUN_COMMAND: &str = "host.run_command";

/// 全部宿主工具名。
pub const ALL_TOOL_NAMES: [&str; 4] = [
    HOST_READ_FILE,
    HOST_WRITE_FILE,
    HOST_LIST_DIR,
    HOST_RUN_COMMAND,
];

/// 危险工具（需用户审批）：run_command 可执行任意命令；其余是 workdir 内只读/写文件，
/// 自动放行（越界由 resolve 防护）。装配 --approval 时组合根 mark_dangerous。
pub const DANGEROUS_TOOL_NAMES: [&str; 1] = [HOST_RUN_COMMAND];

/// 全局 workdir 源（装配方经 [`set_workdir_source`] 注入；工具执行时现读）。
/// Mutex 包裹：生产装配期注入一次；测试/热切换可重新 set（后设覆盖先设）。
static WORKDIR_SOURCE: Mutex<Option<Arc<dyn WorkdirPort>>> = Mutex::new(None);

/// 注入 workdir 源（bm-assembly 装配点；web-server 实现 WorkdirPort 并经组合根传入）。
pub fn set_workdir_source(src: Arc<dyn WorkdirPort>) {
    *WORKDIR_SOURCE.lock().unwrap() = Some(src);
}

/// 当前 workdir（WorkdirPort 现读；未装配/未设置 → None）。
fn workdir() -> Option<PathBuf> {
    WORKDIR_SOURCE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|p| p.current_workdir())
}

/// 全部宿主工具 schema（文档/装配可查询）。
pub fn schemas() -> Vec<ToolSchema> {
    [HOST_READ_FILE, HOST_WRITE_FILE, HOST_LIST_DIR, HOST_RUN_COMMAND]
        .iter()
        .map(|name| {
            let h: Arc<dyn ToolHandler> = match *name {
                HOST_READ_FILE => Arc::new(ReadFileTool),
                HOST_WRITE_FILE => Arc::new(WriteFileTool),
                HOST_LIST_DIR => Arc::new(ListDirTool),
                HOST_RUN_COMMAND => Arc::new(RunCommandTool),
                _ => unreachable!("known host tool name"),
            };
            ToolSchema {
                name: h.name().to_string(),
                description: h.description().to_string(),
                parameters: h.parameters(),
            }
        })
        .collect()
}

/// 注册全部宿主工具到注册表。
/// 调用方来自装配方（bm-assembly），传实现 `ToolRegistrarPort` 的注册表
/// （plugin-tools::ToolRegistry）；装配面在具体实现上，端口只有消费面。
/// 核心插件 host-tools 同功能插件一道经注册面端口注入，不再依赖
/// plugin-tools 具体类型。可重复调用（跳过已注册项，幂等）。
pub fn register_all(registry: &dyn bm_ports::ToolRegistrarPort) -> Result<(), ToolError> {
    let handlers: Vec<Arc<dyn ToolHandler>> = vec![
        Arc::new(ReadFileTool),
        Arc::new(WriteFileTool),
        Arc::new(ListDirTool),
        Arc::new(RunCommandTool),
    ];
    for h in handlers {
        if registry.get(h.name()).is_some() {
            continue; // 幂等：已注册跳过
        }
        registry.register(h)?;
    }
    Ok(())
}

/// 读到 workdir 相对路径。错误返回工具错误字符串（loop 回写 is_error=true）。
fn tool_err(e: host_fs::HostFsError) -> ToolError {
    ToolError::new(format!("tool error: {e}"))
}

fn arg_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

// ---- host.read_file ----

#[derive(Debug, Clone, Copy, Default)]
struct ReadFileTool;

#[async_trait::async_trait]
impl ToolHandler for ReadFileTool {
    fn name(&self) -> &str {
        HOST_READ_FILE
    }

    fn description(&self) -> &str {
        "读取工作目录（workdir）内文本文件的内容（UTF-8，≤2MiB）。路径相对 workdir 根；不支持越界/二进制。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "workdir 内相对路径，如 \"src/main.rs\"" }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(wd) = workdir() else {
            return Err(ToolError::new("tool error: workdir not configured"));
        };
        Self::execute_with_workdir(&input, &wd).await
    }
}

impl ReadFileTool {
    /// workdir 显式注入的执行体（execute 从全局源取 wd 后调这里；
    /// 测试直接传测试 wd，绕过全局源——并行测试互不干扰）。
    pub(crate) async fn execute_with_workdir(
        input: &ToolExecutionInput,
        wd: &std::path::Path,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(rel) = arg_str(&input.arguments, "path") else {
            return Err(ToolError::new("tool error: missing path"));
        };
        let target = host_fs::resolve_in_workdir(wd, &rel).map_err(tool_err)?;
        if !target.exists() {
            return Ok(ToolExecutionResult::error(format!(
                "file not found: {rel}"
            )));
        }
        if target.is_dir() {
            return Ok(ToolExecutionResult::error(format!(
                "{} is a directory, not a file",
                rel
            )));
        }
        let meta = target.metadata().map_err(|e| {
            ToolError::new(format!("tool error: io error: {e}"))
        })?;
        if meta.len() > host_fs::MAX_TEXT_BYTES {
            return Ok(ToolExecutionResult::error(format!(
                "file too large ({} bytes > {} limit)",
                meta.len(),
                host_fs::MAX_TEXT_BYTES
            )));
        }
        let bytes = std::fs::read(&target).map_err(|e| {
            ToolError::new(format!("tool error: read failed: {e}"))
        })?;
        let content = String::from_utf8(bytes).map_err(|_| {
            ToolError::new("tool error: file is not valid UTF-8 text")
        })?;
        Ok(ToolExecutionResult::ok(content))
    }
}

// ---- host.write_file ----

#[derive(Debug, Clone, Copy, Default)]
struct WriteFileTool;

#[async_trait::async_trait]
impl ToolHandler for WriteFileTool {
    fn name(&self) -> &str {
        HOST_WRITE_FILE
    }

    fn description(&self) -> &str {
        "写入（或覆盖）工作目录（workdir）内文本文件。路径相对 workdir 根；父目录须已存在；overwrite=false 时已存在的文件拒绝写入。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "workdir 内相对路径，如 \"src/main.rs\"" },
                "content": { "type": "string", "description": "要写入的文本内容" },
                "overwrite": { "type": "boolean", "description": "目标已存在时是否覆盖（默认 false）" }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(wd) = workdir() else {
            return Err(ToolError::new("tool error: workdir not configured"));
        };
        Self::execute_with_workdir(&input, &wd).await
    }
}

impl WriteFileTool {
    pub(crate) async fn execute_with_workdir(
        input: &ToolExecutionInput,
        wd: &std::path::Path,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(rel) = arg_str(&input.arguments, "path") else {
            return Err(ToolError::new("tool error: missing path"));
        };
        let Some(content) = arg_str(&input.arguments, "content") else {
            return Err(ToolError::new("tool error: missing content"));
        };
        let overwrite = input
            .arguments
            .get("overwrite")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if (content.len() as u64) > host_fs::MAX_TEXT_BYTES {
            return Ok(ToolExecutionResult::error(format!(
                "content too large ({} bytes > {} limit)",
                content.len(),
                host_fs::MAX_TEXT_BYTES
            )));
        }
        let target = host_fs::resolve_in_workdir(wd, &rel).map_err(tool_err)?;
        // atomic_write 失败分类：AlreadyExists = 业务失败（ok+is_error），
        // 其他 IO 错误 = 工具异常（Err）。
        match host_fs::atomic_write(&target, content.as_bytes(), overwrite) {
            Ok(()) => Ok(ToolExecutionResult::ok(format!(
                "wrote {rel} (overwrite={overwrite})"
            ))),
            Err(host_fs::HostFsError::AlreadyExists(_)) => Ok(ToolExecutionResult::error(format!(
                "file already exists: {rel} (pass overwrite=true to replace)"
            ))),
            Err(e) => Err(tool_err(e)),
        }
    }
}

// ---- host.list_dir ----

#[derive(Debug, Clone, Copy, Default)]
struct ListDirTool;

#[async_trait::async_trait]
impl ToolHandler for ListDirTool {
    fn name(&self) -> &str {
        HOST_LIST_DIR
    }

    fn description(&self) -> &str {
        "列出工作目录（workdir）内某目录的条目（目录优先 + 名字排序）。路径相对 workdir 根；空 = 列根目录。返回 JSON：{path, entries:[{name,path,isDir,size,hidden}]}。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "workdir 内相对目录路径；空字符串 = workdir 根" }
            },
            "required": [],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(wd) = workdir() else {
            return Err(ToolError::new("tool error: workdir not configured"));
        };
        Self::execute_with_workdir(&input, &wd).await
    }
}

impl ListDirTool {
    pub(crate) async fn execute_with_workdir(
        input: &ToolExecutionInput,
        wd: &std::path::Path,
    ) -> Result<ToolExecutionResult, ToolError> {
        let rel = arg_str(&input.arguments, "path").unwrap_or_default();
        let target = host_fs::resolve_in_workdir(wd, &rel).map_err(tool_err)?;
        if !target.is_dir() {
            return Ok(ToolExecutionResult::error(format!(
                "{} is not a directory",
                rel
            )));
        }
        let read = std::fs::read_dir(&target).map_err(|e| {
            ToolError::new(format!("tool error: cannot read directory: {e}"))
        })?;
        let mut entries: Vec<serde_json::Value> = Vec::new();
        for item in read.flatten() {
            let path = item.path();
            let is_dir = path.is_dir();
            let size = path.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push(serde_json::json!({
                "name": item.file_name().to_string_lossy(),
                "path": rel_of(wd, &path),
                "isDir": is_dir,
                "size": size,
                "hidden": host_fs::is_hidden_path(&path),
            }));
            if entries.len() >= host_fs::MAX_ENTRIES_PER_DIR {
                break;
            }
        }
        entries.sort_by(|a, b| {
            let ad = a["isDir"].as_bool().unwrap_or(false);
            let bd = b["isDir"].as_bool().unwrap_or(false);
            if ad != bd {
                return bd.cmp(&ad);
            }
            let an = a["name"].as_str().unwrap_or("");
            let bn = b["name"].as_str().unwrap_or("");
            an.to_lowercase().cmp(&bn.to_lowercase())
        });
        Ok(ToolExecutionResult::ok(
            serde_json::to_string(&serde_json::json!({
                "path": rel,
                "entries": entries,
                "truncated": entries.len() >= host_fs::MAX_ENTRIES_PER_DIR,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        ))
    }
}

// ---- host.run_command ----

#[derive(Debug, Clone, Copy, Default)]
struct RunCommandTool;

#[async_trait::async_trait]
impl ToolHandler for RunCommandTool {
    fn name(&self) -> &str {
        HOST_RUN_COMMAND
    }

    fn description(&self) -> &str {
        "在工作目录（workdir）内执行一条 shell 命令（Windows 用 cmd /C，其他平台 sh -c）。cwd 可指定 workdir 内相对子目录；超时 30 秒。返回 JSON：{exit_code, stdout, stderr}。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "cmd": { "type": "string", "description": "要执行的 shell 命令文本" },
                "cwd": { "type": "string", "description": "workdir 内相对子目录；缺省 = workdir 根" }
            },
            "required": ["cmd"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(wd) = workdir() else {
            return Err(ToolError::new("tool error: workdir not configured"));
        };
        Self::execute_with_workdir(&input, &wd).await
    }
}

impl RunCommandTool {
    pub(crate) async fn execute_with_workdir(
        input: &ToolExecutionInput,
        wd: &std::path::Path,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(cmd) = arg_str(&input.arguments, "cmd") else {
            return Err(ToolError::new("tool error: missing cmd"));
        };
        let cwd_rel = arg_str(&input.arguments, "cwd").unwrap_or_default();
        let cwd = host_fs::resolve_in_workdir(wd, &cwd_rel).map_err(tool_err)?;
        if !cwd.is_dir() {
            return Ok(ToolExecutionResult::error(format!(
                "cwd {} is not a directory",
                cwd_rel
            )));
        }
        let output = run_shell(&cmd, &cwd).await.map_err(|e| {
            ToolError::new(format!("tool error: {e}"))
        })?;
        Ok(ToolExecutionResult::ok(
            serde_json::to_string(&serde_json::json!({
                "exit_code": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        ))
    }
}

/// 在 cwd 内执行 shell 命令（Windows cmd /C；其他 sh -c）。带超时（kill 保护）。
async fn run_shell(cmd: &str, cwd: &PathBuf) -> Result<std::process::Output, String> {
    use std::process::Stdio;
    #[cfg(windows)]
    let mut command = {
        let mut c = AsyncCommand::new("cmd");
        c.args(["/C", cmd]);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = AsyncCommand::new("sh");
        c.args(["-c", cmd]);
        c
    };
    let mut child = command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;

    wait_result(&mut child).await
}

/// 等子进程（带超时；超时 kill 保护）。`wait()` 是 `&mut self`，`kill()` 也是；
/// 同一 `&mut Child` 顺序用，无 move 问题。
async fn wait_result(child: &mut tokio::process::Child) -> Result<std::process::Output, String> {
    use tokio::io::AsyncReadExt;
    // 先 take 句柄（wait 之前），wait() 是 &mut self。
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let result = tokio::time::timeout(COMMAND_TIMEOUT, async {
        let status = child.wait().await.map_err(|e| format!("wait failed: {e}"))?;
        let mut out = Vec::new();
        let mut err = Vec::new();
        stdout.read_to_end(&mut out).await.map_err(|e| format!("read stdout: {e}"))?;
        stderr.read_to_end(&mut err).await.map_err(|e| format!("read stderr: {e}"))?;
        Ok::<std::process::Output, String>(std::process::Output {
            status,
            stdout: out,
            stderr: err,
        })
    })
    .await;

    match result {
        Ok(res) => res,
        Err(_) => {
            // 超时：kill 子进程（best-effort）。
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(format!("command timed out after {COMMAND_TIMEOUT:?}"))
        }
    }
}

/// 目标相对 workdir 的 POSIX 风格路径（与 web-server host.rs rel_from_workdir 同逻辑；
/// Windows 下 canonical 路径带 `\\?\` 前缀 → 归一化去前缀后按字符串比）。
fn rel_of(wd: &std::path::Path, target: &std::path::Path) -> String {
    let norm = |s: &std::path::Path| {
        s.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string()
    };
    // 统一去掉 Windows extended-length 前缀（`\\?\` → `//?/`）。
    let strip_win_prefix = |s: String| {
        if let Some(rest) = s.strip_prefix("//?/") {
            rest.to_string()
        } else {
            s
        }
    };
    let wd_s = strip_win_prefix(norm(wd));
    let target_s = strip_win_prefix(norm(target));
    target_s
        .strip_prefix(&wd_s)
        .and_then(|r| r.strip_prefix('/'))
        .unwrap_or(&target_s)
        .to_string()
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试的独立临时 workdir（uuid 目录，并行安全——execute_with_workdir
    /// 显式传 wd，测试间完全隔离，不共享任何全局状态）。
    fn tmp_workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bm-pht-{tag}-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn input(name: &str, args: serde_json::Value) -> ToolExecutionInput {
        ToolExecutionInput {
            name: name.to_string(),
            arguments: args,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_write_roundtrip() {
        let wd = tmp_workdir("rw");
        std::fs::create_dir_all(wd.join("sub")).unwrap();

        let w = WriteFileTool::execute_with_workdir(
            &input(HOST_WRITE_FILE, serde_json::json!({
                "path": "sub/hello.txt", "content": "hello 世界"
            })),
            &wd,
        )
        .await
        .unwrap();
        assert!(!w.is_error);
        assert!(w.output.contains("wrote sub/hello.txt"));

        let r = ReadFileTool::execute_with_workdir(
            &input(HOST_READ_FILE, serde_json::json!({ "path": "sub/hello.txt" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(!r.is_error);
        assert_eq!(r.output, "hello 世界");

        // 越界路径拒绝（路径逃逸 = 工具异常 Err）
        let esc = ReadFileTool::execute_with_workdir(
            &input(HOST_READ_FILE, serde_json::json!({ "path": "../secret.txt" })),
            &wd,
        )
        .await
        .expect_err("escape must be a tool error, not an output");
        assert!(esc.0.contains("not inside") || esc.0.contains("invalid path"));

        // 读不存在（业务失败 = ok + is_error）
        let nofile = ReadFileTool::execute_with_workdir(
            &input(HOST_READ_FILE, serde_json::json!({ "path": "nope.txt" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(nofile.is_error);
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_overwrite_semantics() {
        let wd = tmp_workdir("ow");
        WriteFileTool::execute_with_workdir(
            &input(HOST_WRITE_FILE, serde_json::json!({ "path": "a.txt", "content": "one" })),
            &wd,
        )
        .await
        .unwrap();

        // overwrite 缺省 false → 拒绝覆盖（ok + is_error）
        let w2 = WriteFileTool::execute_with_workdir(
            &input(HOST_WRITE_FILE, serde_json::json!({ "path": "a.txt", "content": "two" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(w2.is_error);
        assert_eq!(std::fs::read_to_string(wd.join("a.txt")).unwrap(), "one");

        // overwrite=true → 覆盖
        let w3 = WriteFileTool::execute_with_workdir(
            &input(HOST_WRITE_FILE, serde_json::json!({
                "path": "a.txt", "content": "two", "overwrite": true
            })),
            &wd,
        )
        .await
        .unwrap();
        assert!(!w3.is_error);
        assert_eq!(std::fs::read_to_string(wd.join("a.txt")).unwrap(), "two");
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_dir_returns_sorted_entries() {
        let wd = tmp_workdir("list");
        std::fs::write(wd.join("b.txt"), "b").unwrap();
        std::fs::create_dir(wd.join("adir")).unwrap();
        std::fs::write(wd.join("adir").join("inner.txt"), "i").unwrap();

        let res = ListDirTool::execute_with_workdir(
            &input(HOST_LIST_DIR, serde_json::json!({ "path": "" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(!res.is_error);
        let v: serde_json::Value = serde_json::from_str(&res.output).unwrap();
        let entries = v["entries"].as_array().unwrap();
        // 目录在前 + 名字排序
        assert_eq!(entries[0]["isDir"], true);
        assert_eq!(entries[0]["name"], "adir");
        assert_eq!(entries[1]["name"], "b.txt");
        assert_eq!(entries[1]["path"], "b.txt");

        let sub = ListDirTool::execute_with_workdir(
            &input(HOST_LIST_DIR, serde_json::json!({ "path": "adir" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(!sub.is_error);
        let sv: serde_json::Value = serde_json::from_str(&sub.output).unwrap();
        assert_eq!(sv["entries"][0]["name"], "inner.txt");
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_command_echo() {
        let wd = tmp_workdir("cmd");
        let res = RunCommandTool::execute_with_workdir(
            &input(HOST_RUN_COMMAND, serde_json::json!({ "cmd": "echo hi" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(!res.is_error);
        let v: serde_json::Value = serde_json::from_str(&res.output).unwrap();
        assert_eq!(v["exit_code"], 0);
        assert!(v["stdout"].as_str().unwrap_or("").contains("hi"));
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn manifest_and_schemas_are_consistent() {
        use kernel_contracts::plugin::PluginCategory;
        let m = manifest();
        assert_eq!(m.id, "plugin-host-tools");
        assert_eq!(m.category, PluginCategory::Core);
        let schemas = schemas();
        assert_eq!(schemas.len(), 4);
        for s in &schemas {
            assert!(ALL_TOOL_NAMES.contains(&s.name.as_str()), "unexpected schema {}", s.name);
        }
    }
}
