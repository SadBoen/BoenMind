//! # plugin-code-runtime —— 代码执行沙箱插件（功能分类）。
//!
//! 把 workdir 作用域内的代码编译/脚本执行注册为 Agent 工具，结果回灌循环。
//! 工具集（命名沿用 `code.*`，与 DSH 工具族同构）：
//! - `code.compile`  编译单个源文件（rustc / gcc / g++ / go，按扩展名推导或显式 toolchain）
//! - `code.python`   运行 Python 脚本（python / python3，cwd = 脚本所在目录）
//! - `code.shell`    执行 shell 命令（Windows cmd /C、其他 sh -c）——`host.run_command`
//!   的**输出钱包版**：同一套超时 kill + 磁盘/内存上限，但额外防上下文撑爆
//!
//! 安全语义与 plugin-host-tools **同源**：
//! - 路径一律经 `host-fs::resolve_in_workdir` 作用域校验，逃逸即拒
//! - 命令执行 30s 超时 + kill 保护（跑飞/死循环兜底）
//! - **输出钱包**（[`MAX_OUTPUT_BYTES`]）：stdout/stderr 读取即限顶（保留 cap 内字节、
//!   丢弃超出、记总数），并发排水防止子进程写满管道死锁——即使子进程洪水输出，
//!   进程内内存也不会超过 (cap × 2 + 常数)，模型上下文不会被撑爆
//!
//! workdir 事实源 = [`WorkdirPort`]（bm-ports 产品契约），与 host-tools 同一注入模式：
//! 装配方（bm-assembly）经 [`set_workdir_source`] 注入全局源，工具执行时现读——
//! 设置页改 workdir 后**下一工具调用即时生效**。
//!
//! 接线：装配方调用 [`register_all`] 注册全部工具并 `gate.enable`（默认启用；
//! 若需更严策略可按工具名单独 `gate.enable`）。之后 plugin-loop 每回合把已启用
//! 工具 schema 发给模型，工具调用经 ToolGate 执行。

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

/// 命令执行超时（跑飞命令/死循环/洪水输出保护）。超时 → 结构化错误结果（is_error=true，
/// 循环可把"超时"事实回写给模型，模型据此调整——不吞成通用工具异常）。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// 输出钱包：单次调用的 stdout+stderr 各自最多保留的字节数（读即限顶，
/// 超出丢弃并记总数——防止洪水输出撑爆进程内存 / 模型上下文）。
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;

/// code-runtime 工具 id 常量（门控白名单/前端回显引用）。
pub const CODE_COMPILE: &str = "code.compile";
pub const CODE_PYTHON: &str = "code.python";
pub const CODE_SHELL: &str = "code.shell";

/// 全部 code-runtime 工具名。
pub const ALL_TOOL_NAMES: [&str; 3] = [CODE_COMPILE, CODE_PYTHON, CODE_SHELL];

/// 全局 workdir 源（装配方经 [`set_workdir_source`] 注入；工具执行时现读）。
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

/// 全部 code-runtime 工具 schema（文档/装配可查询）。
pub fn schemas() -> Vec<ToolSchema> {
    [CODE_COMPILE, CODE_PYTHON, CODE_SHELL]
        .iter()
        .map(|name| {
            let h: Arc<dyn ToolHandler> = match *name {
                CODE_COMPILE => Arc::new(CompileTool),
                CODE_PYTHON => Arc::new(PythonTool),
                CODE_SHELL => Arc::new(ShellTool),
                _ => unreachable!("known code tool name"),
            };
            ToolSchema {
                name: h.name().to_string(),
                description: h.description().to_string(),
                parameters: h.parameters(),
            }
        })
        .collect()
}

/// 注册全部 code-runtime 工具到注册表。
/// 调用方来自装配方（bm-assembly），传 plug-tools 的 `ToolRegistry` 具体类型。
/// 可重复调用（跳过已注册项，幂等）。
pub fn register_all(registry: &plugin_tools::ToolRegistry) -> Result<(), ToolError> {
    let handlers: Vec<Arc<dyn ToolHandler>> = vec![
        Arc::new(CompileTool),
        Arc::new(PythonTool),
        Arc::new(ShellTool),
    ];
    for h in handlers {
        if registry.get(h.name()).is_some() {
            continue; // 幂等：已注册跳过
        }
        registry.register(h)?;
    }
    Ok(())
}

/// host-fs 错误统一转工具异常（Err 语义，loop 回写 is_error=true）。
fn tool_err(e: host_fs::HostFsError) -> ToolError {
    ToolError::new(format!("tool error: {e}"))
}

fn arg_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

/// 命令执行结果（限顶后）。
struct Captured {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_total: usize,
    stderr_total: usize,
    timed_out: bool,
}

/// 在 workdir 内解析 cwd（相对路径；空 = 根）。返回实际目录。
fn resolve_cwd(wd: &std::path::Path, rel: &str) -> Result<PathBuf, ToolError> {
    let cwd = host_fs::resolve_in_workdir(wd, rel).map_err(tool_err)?;
    if !cwd.is_dir() {
        return Err(ToolError::new(format!("tool error: cwd {rel:?} is not a directory")));
    }
    Ok(cwd)
}

/// 在 `cwd` 内执行程序（带超时 kill + 输出钱包 + 并发排水）。
/// 超时不返回 Err（结构化回写 is_error=true，模型可见"timed out"并可调整）；
/// 真正 Err 只留给 spawn/IO 异常。
async fn exec_program(
    program: &str,
    args: &[String],
    cwd: &std::path::Path,
) -> Result<ToolExecutionResult, ToolError> {
    use std::process::Stdio;
    let mut child = AsyncCommand::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::new(format!("tool error: spawn {program} failed: {e}")))?;

    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let mut captured = Captured {
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        stdout_total: 0,
        stderr_total: 0,
        timed_out: false,
    };

    // 并发排水：wait 与 限顶读取同时进行——否则子进程写满管道会阻塞在 write，
    // 永远到不了 wait 的 EOF（洪水输出是真实攻击面，参考主机工具只 wait 后读的
    // 顺块局限，这里从根上避免死锁）。
    let drain = tokio::time::timeout(COMMAND_TIMEOUT, async {
        let wait = child.wait();
        let out_drain = drain_capped(&mut stdout, &mut captured.stdout, &mut captured.stdout_total);
        let err_drain = drain_capped(&mut stderr, &mut captured.stderr, &mut captured.stderr_total);
        let (w, o, e) = tokio::join!(wait, out_drain, err_drain);
        let status = w.map_err(|err| format!("wait failed: {err}"))?;
        captured.exit_code = status.code();
        o.map_err(|err| format!("read stdout: {err}"))?;
        e.map_err(|err| format!("read stderr: {err}"))?;
        Ok::<(), String>(())
    })
    .await;

    match drain {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(ToolError::new(format!("tool error: {e}"))),
        Err(_) => {
            // 超时：kill 子进程（best-effort），结构化回写（is_error=true，模型可见）。
            let _ = child.kill().await;
            let _ = child.wait().await;
            captured.timed_out = true;
        }
    }

    Ok(result_json(&captured))
}

/// 限顶读取：管道读到 cap 后继续**排空丢弃**（子进程不阻塞），只记总数。
async fn drain_capped<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    keep: &mut Vec<u8>,
    total: &mut usize,
) -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;
    let cap = MAX_OUTPUT_BYTES;
    let mut chunk = [0u8; 4096];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        *total += n;
        let room = cap.saturating_sub(keep.len());
        let take = room.min(n);
        if take > 0 {
            keep.extend_from_slice(&chunk[..take]);
        }
    }
    Ok(())
}

/// 组装结构化结果 JSON（文本约定同 host-tools：JSON 字符串进 output）。
fn result_json(c: &Captured) -> ToolExecutionResult {
    let v = serde_json::json!({
        "exit_code": c.exit_code,
        "timeout": c.timed_out,
        "stdout": String::from_utf8_lossy(&c.stdout),
        "stderr": String::from_utf8_lossy(&c.stderr),
        "stdout_bytes": c.stdout_total,
        "stderr_bytes": c.stderr_total,
        "stdout_truncated": c.stdout_total > c.stdout.len(),
        "stderr_truncated": c.stderr_total > c.stderr.len(),
    });
    let text = serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string());
    if c.timed_out {
        ToolExecutionResult::error(text)
    } else {
        ToolExecutionResult::ok(text)
    }
}

// ---- code.compile ----

#[derive(Debug, Clone, Copy, Default)]
struct CompileTool;

#[async_trait::async_trait]
impl ToolHandler for CompileTool {
    fn name(&self) -> &str {
        CODE_COMPILE
    }

    fn description(&self) -> &str {
        "编译 workdir 内的单个源文件为可执行文件（toolchain=auto 按扩展名推导：.rs→rustc、.c→gcc、.cpp/.cc→g++、.go→go；可显式指定）。cwd = 源文件所在目录；产物缺省 = 源文件同名（Windows 加 .exe）。返回 JSON：{exit_code, stdout, stderr, timeout, 各流字节数与截断标记}。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file": { "type": "string", "description": "workdir 内相对源文件路径，如 \"src/main.rs\" 或 \"solver.go\"" },
                "toolchain": { "type": "string", "enum": ["auto", "rustc", "gcc", "g++", "go"], "description": "编译器；缺省 auto=按扩展名推导" },
                "args": { "type": "array", "items": { "type": "string" }, "description": "附加编译参数（追加在命令尾部，如 [\"-O2\"]）" },
                "out": { "type": "string", "description": "输出可执行文件名（相对源文件目录）；缺省 = 源文件 stem" }
            },
            "required": ["file"],
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

impl CompileTool {
    pub(crate) async fn execute_with_workdir(
        input: &ToolExecutionInput,
        wd: &std::path::Path,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(rel) = arg_str(&input.arguments, "file") else {
            return Err(ToolError::new("tool error: missing file"));
        };
        let target = host_fs::resolve_in_workdir(wd, &rel).map_err(tool_err)?;
        if !target.is_file() {
            return Ok(ToolExecutionResult::error(format!(
                "compile source not found (or not a file): {rel}"
            )));
        }
        let dir = match target.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => wd.to_path_buf(),
        };
        let file_name = target
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .ok_or_else(|| ToolError::new("tool error: invalid file name"))?;
        let ext = target
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        // toolchain 决断：显式 / 按扩展名推导 / 不支持。
        let toolchain = arg_str(&input.arguments, "toolchain").unwrap_or_else(|| "auto".to_string());
        let (program, base_args, out_suffix): (&str, Vec<String>, String) = match toolchain.as_str() {
            "rustc" => ("rustc", Vec::new(), ".exe".to_string()),
            "gcc" => ("gcc", Vec::new(), String::new()),
            "g++" => ("g++", Vec::new(), String::new()),
            "go" => ("go", vec!["build".to_string()], String::new()),
            "auto" => match ext.as_str() {
                "rs" => ("rustc", Vec::new(), ".exe".to_string()),
                "c" => ("gcc", Vec::new(), String::new()),
                "cpp" | "cc" | "cxx" => ("g++", Vec::new(), String::new()),
                "go" => ("go", vec!["build".to_string()], String::new()),
                _ => {
                    return Ok(ToolExecutionResult::error(format!(
                        "unsupported extension .{ext} for auto toolchain (use explicit toolchain: rustc/gcc/g++/go)"
                    )))
                }
            },
            other => {
                return Ok(ToolExecutionResult::error(format!(
                    "unknown toolchain: {other} (auto/rustc/gcc/g++/go)"
                )))
            }
        };

        let stem = target
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| file_name.clone());
        let out_name = arg_str(&input.arguments, "out")
            .unwrap_or_else(|| format!("{stem}{out_suffix}"));
        // 产物必须留在源文件目录（workdir 内），而非可执行目录——组装 `-o` 参数。
        let mut args = base_args;
        if toolchain == "go" {
            args.extend(["-o".to_string(), out_name]);
            args.push(file_name.clone());
        } else if program == "rustc" {
            args.push(file_name.clone());
            args.extend(["--edition".to_string(), "2021".to_string()]);
            args.push("-o".to_string());
            args.push(out_name);
        } else {
            args.push(file_name.clone());
            args.push("-o".to_string());
            args.push(out_name);
        }
        if let Some(extra) = input.arguments.get("args").and_then(serde_json::Value::as_array) {
            for v in extra {
                if let Some(s) = v.as_str() {
                    args.push(s.to_string());
                }
            }
        }

        exec_program(program, &args, &dir).await
    }
}

// ---- code.python ----

#[derive(Debug, Clone, Copy, Default)]
struct PythonTool;

#[async_trait::async_trait]
impl ToolHandler for PythonTool {
    fn name(&self) -> &str {
        CODE_PYTHON
    }

    fn description(&self) -> &str {
        "运行 workdir 内的 Python 脚本（Windows 用 python，其他平台 python3）。cwd = 脚本所在目录，脚本参数可传。超时 30 秒，输出有上限（保留 512KB，超量截断只记总数）。返回 JSON：{exit_code, stdout, stderr, timeout, 截断标记}。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file": { "type": "string", "description": "workdir 内相对脚本路径，如 \"scripts/solve.py\"" },
                "args": { "type": "array", "items": { "type": "string" }, "description": "传给脚本的命令行参数" }
            },
            "required": ["file"],
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

impl PythonTool {
    pub(crate) async fn execute_with_workdir(
        input: &ToolExecutionInput,
        wd: &std::path::Path,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(rel) = arg_str(&input.arguments, "file") else {
            return Err(ToolError::new("tool error: missing file"));
        };
        let target = host_fs::resolve_in_workdir(wd, &rel).map_err(tool_err)?;
        if !target.is_file() {
            return Ok(ToolExecutionResult::error(format!(
                "script not found (or not a file): {rel}"
            )));
        }
        let dir = match target.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => wd.to_path_buf(),
        };
        let file_name = target
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .ok_or_else(|| ToolError::new("tool error: invalid file name"))?;

        let mut args = vec![file_name];
        if let Some(extra) = input.arguments.get("args").and_then(serde_json::Value::as_array) {
            for v in extra {
                if let Some(s) = v.as_str() {
                    args.push(s.to_string());
                }
            }
        }
        // Windows 用 python（py launcher 存在但慢启动；python 优先与标准提示一致），
        // 其他平台 python3。
        #[cfg(windows)]
        let program = "python";
        #[cfg(not(windows))]
        let program = "python3";

        exec_program(program, &args, &dir).await
    }
}

// ---- code.shell ----

#[derive(Debug, Clone, Copy, Default)]
struct ShellTool;

#[async_trait::async_trait]
impl ToolHandler for ShellTool {
    fn name(&self) -> &str {
        CODE_SHELL
    }

    fn description(&self) -> &str {
        "在 workdir 内执行一条 shell 命令（Windows cmd /C，其他 sh -c）——与 host.run_command 相同的超时与作用域，但输出有上限（512KB 截断只记总数），适合长输出命令。返回 JSON：{exit_code, stdout, stderr, timeout, 截断标记}。"
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

impl ShellTool {
    pub(crate) async fn execute_with_workdir(
        input: &ToolExecutionInput,
        wd: &std::path::Path,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(cmd) = arg_str(&input.arguments, "cmd") else {
            return Err(ToolError::new("tool error: missing cmd"));
        };
        let cwd_rel = arg_str(&input.arguments, "cwd").unwrap_or_default();
        let cwd = resolve_cwd(wd, &cwd_rel)?;

        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C".to_string(), cmd]);
        #[cfg(not(windows))]
        let (program, args) = ("sh", vec!["-c".to_string(), cmd]);

        exec_program(program, &args, &cwd).await
    }
}

// ---- 测试 ----

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试的独立临时 workdir（uuid 目录，并行安全）。
    fn tmp_workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bm-crt-{tag}-{}", uuid::Uuid::new_v4()));
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
    async fn shell_runs_with_wallet_and_schema() {
        let wd = tmp_workdir("shell");
        let res = ShellTool::execute_with_workdir(
            &input(CODE_SHELL, serde_json::json!({ "cmd": "echo hello", "cwd": "" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(!res.is_error, "output: {}", res.output);
        let v: serde_json::Value = serde_json::from_str(&res.output).unwrap();
        assert_eq!(v["exit_code"], 0);
        assert!(v["stdout"].as_str().unwrap_or("").contains("hello"));
        assert_eq!(v["timeout"], false);
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn python_tool_missing_script_is_business_error() {
        let wd = tmp_workdir("py404");
        let res = PythonTool::execute_with_workdir(
            &input(CODE_PYTHON, serde_json::json!({ "file": "nope.py" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(res.is_error);
        assert!(res.output.contains("not found"));
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn escape_paths_rejected() {
        let wd = tmp_workdir("esc");
        let res = PythonTool::execute_with_workdir(
            &input(CODE_PYTHON, serde_json::json!({ "file": "../evil.py" })),
            &wd,
        )
        .await
        .expect_err("escape must be a tool error, not an output");
        assert!(res.0.contains("not inside") || res.0.contains("invalid path"));
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compile_auto_unsupported_extension_is_business_error() {
        let wd = tmp_workdir("ext");
        std::fs::write(wd.join("code.xyz"), "x").unwrap();
        let res = CompileTool::execute_with_workdir(
            &input(CODE_COMPILE, serde_json::json!({ "file": "code.xyz" })),
            &wd,
        )
        .await
        .unwrap();
        assert!(res.is_error);
        assert!(res.output.contains("unsupported extension"));
        let _ = std::fs::remove_dir_all(&wd);
    }

    #[test]
    fn manifest_and_schemas_are_consistent() {
        use kernel_contracts::plugin::PluginCategory;
        let m = manifest();
        assert_eq!(m.id, "plugin-code-runtime");
        assert_eq!(m.category, PluginCategory::Feature);
        let schemas = schemas();
        assert_eq!(schemas.len(), 3);
        for s in &schemas {
            assert!(ALL_TOOL_NAMES.contains(&s.name.as_str()), "unexpected schema {}", s.name);
        }
    }
}