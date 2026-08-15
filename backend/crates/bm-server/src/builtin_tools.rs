//! B6 — 内置工具集（bm 引擎的 `pi.tool(name, input)` 宿主执行侧）。
//!
//! 自研实现，语义对齐 legacy tools.rs 的忠实子集（pi 插件生态的调用契约）：
//! 工具名/参数名/返回形状（`{content:[{type:"text",text}], details}`）与 pi
//! 对齐，实现刻意简化——不做图片处理/模糊编辑/艺术根目录/权限档位检查
//! （后者由 host 层的 policy 裁决承担）。与 legacy 的差异点都在各工具
//! 注释里写明，插件依赖行为时以注释为准。
//!
//! 递归防护：本表只认内置工具名，未知名字直接报 `unknown tool`——**不查
//! 插件注册表**。插件的工具互调在 JS 侧（import）完成，宿主桥不代查，
//! 因此不存在「插件工具 → 宿主 → 同引擎再执行」的递归环（若未来支持
//! 插件工具经宿主执行，须在此加执行深度计数）。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt as _;

/// read 单文件上限（对齐 legacy `READ_TOOL_MAX_BYTES` = 100MB）。
const READ_TOOL_MAX_BYTES: u64 = 100 * 1024 * 1024;
/// write/edit 单文件上限（对齐 legacy `WRITE_TOOL_MAX_BYTES` = 100MB）。
const WRITE_TOOL_MAX_BYTES: usize = 100 * 1024 * 1024;
/// bash 默认超时（ms）。
const DEFAULT_BASH_TIMEOUT_MS: u64 = 60_000;
/// grep/find 默认超时（ms）：遍历尊重 .gitignore 后正常搜索亚秒级，
/// 60s 兜底防极端目录树挂死回合（M1 验收问题 2）。
const DEFAULT_GREP_TIMEOUT_MS: u64 = 60_000;
/// find/ls 默认结果数上限。
const DEFAULT_LIST_LIMIT: usize = 100;

/// 工具执行错误：`code` 对齐 legacy hostcall 错误码面（invalid_request/io/…）。
#[derive(Debug)]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl ToolError {
    fn invalid(message: impl Into<String>) -> Self {
        Self { code: "invalid_request", message: message.into() }
    }
    fn io(message: impl Into<String>) -> Self {
        Self { code: "io", message: message.into() }
    }
    fn timeout(message: impl Into<String>) -> Self {
        Self { code: "timeout", message: message.into() }
    }
}

type ToolResult = Result<serde_json::Value, ToolError>;

/// 成功输出的统一形状（对齐 pi `ToolOutput` 序列化：content[].text）。
fn text_output(text: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": text.into() }],
        "details": null,
    })
}

/// 内置工具集宿主：相对路径以 `cwd` 为根（会话工作目录）。
pub struct BuiltinTools {
    cwd: PathBuf,
}

impl BuiltinTools {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    /// 工作目录（事件 ctx 的 cwd 数据面）。
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// 内置工具名（模型侧可见 + 插件 pi.tool 可调）。
    pub const NAMES: [&'static str; 7] =
        ["read", "write", "edit", "grep", "find", "ls", "bash"];

    /// 内置工具的模型侧 schema（bm-loop ToolDef 形态；对齐 pi
    /// BUILTIN_TOOL_NAMES 全开语义——模型可直接调 read/write/bash）。
    pub fn definitions() -> Vec<bm_loop::model::ToolDef> {
        vec![
            bm_loop::model::ToolDef::new(
                "read",
                "Read a file's text content (relative or absolute path; offset/limit in bytes).",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file" },
                        "offset": { "type": "integer", "description": "Byte offset to start from" },
                        "limit": { "type": "integer", "description": "Max bytes to read" },
                    },
                    "required": ["path"],
                }),
            ),
            bm_loop::model::ToolDef::new(
                "write",
                "Write content to a file. Creates the file and parent directories; overwrites if it exists.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file" },
                        "content": { "type": "string", "description": "Content to write" },
                    },
                    "required": ["path", "content"],
                }),
            ),
            bm_loop::model::ToolDef::new(
                "edit",
                "Replace all occurrences of old_text with new_text in a file (literal match).",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_text": { "type": "string" },
                        "new_text": { "type": "string" },
                    },
                    "required": ["path", "old_text", "new_text"],
                }),
            ),
            bm_loop::model::ToolDef::new(
                "grep",
                "Recursively search files for a literal substring (ignore_case optional; respects .gitignore/.ignore and skips hidden files).",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path": { "type": "string", "description": "Root directory (default: working dir)" },
                        "ignore_case": { "type": "boolean" },
                        "limit": { "type": "integer" },
                        "timeout": { "type": "integer", "description": "Timeout in ms (default 60000)" },
                    },
                    "required": ["pattern"],
                }),
            ),
            bm_loop::model::ToolDef::new(
                "find",
                "Recursively find files whose name contains the pattern (respects .gitignore/.ignore and skips hidden files).",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path": { "type": "string" },
                        "limit": { "type": "integer" },
                        "timeout": { "type": "integer", "description": "Timeout in ms (default 60000)" },
                    },
                    "required": ["pattern"],
                }),
            ),
            bm_loop::model::ToolDef::new(
                "ls",
                "List a directory's entries (directories marked with trailing '/').",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "limit": { "type": "integer" },
                    },
                }),
            ),
            bm_loop::model::ToolDef::new(
                "bash",
                "Run a shell command (Windows: cmd /C; others: /bin/sh -c). Returns {stdout, stderr, code, killed}.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "cwd": { "type": "string" },
                        "timeout": { "type": "integer", "description": "Timeout in ms (default 60000)" },
                    },
                    "required": ["command"],
                }),
            ),
        ]
    }

    /// 执行内置工具。名字不在内置表 → `unknown tool`（递归防护见模块注释）。
    pub async fn execute(&self, name: &str, input: serde_json::Value) -> ToolResult {
        match name.trim() {
            "read" => self.read(&input),
            "write" => self.write(&input),
            "edit" => self.edit(&input),
            "grep" => self.grep(&input).await,
            "find" => self.find(&input).await,
            "ls" => self.ls(&input),
            "bash" => self.bash(&input).await,
            other => Err(ToolError::invalid(format!("Unknown tool: {other}"))),
        }
    }

    /// 相对路径解析到 cwd 下（绝对路径原样）。legacy 的 sandbox 越界拒绝
    /// 由 host 层 policy 裁决承担，这里不复制 cwd 圈禁。
    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }

    /// `{path(必填), offset?, limit?}` → 读文件文本。
    /// 简化：二进制文件按 UTF-8 lossy 读（legacy 有图片/二进制专门路径），
    /// 超 100MB 截断并追加标记。
    fn read(&self, input: &serde_json::Value) -> ToolResult {
        let path = input.get("path").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("read: path is required"))?;
        let offset = input.get("offset").and_then(serde_json::Value::as_i64).unwrap_or(0);
        let limit = input.get("limit").and_then(serde_json::Value::as_i64);

        let resolved = self.resolve(path);
        let bytes = std::fs::read(&resolved)
            .map_err(|e| ToolError::io(format!("read failed for {}: {e}", resolved.display())))?;

        let skip = offset.clamp(0, bytes.len() as i64) as usize;
        let take = match limit {
            Some(l) if l > 0 => (l as usize).min(bytes.len().saturating_sub(skip)),
            _ => bytes.len().saturating_sub(skip),
        };
        let truncated = (bytes.len() as u64) > READ_TOOL_MAX_BYTES;
        let slice = &bytes[skip..skip + take.min(READ_TOOL_MAX_BYTES as usize)];

        let mut text = String::from_utf8_lossy(slice).into_owned();
        if truncated {
            text.push_str("\n... [content truncated] ...");
        }
        Ok(text_output(text))
    }

    /// `{path(必填), content(必填)}` → 原子写入（创建父目录，覆盖已有）。
    /// 成功文本对齐 legacy：「Successfully wrote N bytes to PATH」（N = UTF-16 码元数）。
    fn write(&self, input: &serde_json::Value) -> ToolResult {
        let path = input.get("path").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("write: path is required"))?;
        let content = input.get("content").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("write: content is required"))?;
        if content.len() > WRITE_TOOL_MAX_BYTES {
            return Err(ToolError::invalid(format!(
                "Content size exceeds maximum allowed ({0} > {WRITE_TOOL_MAX_BYTES} bytes)",
                content.len(),
            )));
        }

        let resolved = self.resolve(path);
        let parent = resolved.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)
            .map_err(|e| ToolError::io(format!("Failed to create directories: {e}")))?;
        atomic_write(&resolved, content.as_bytes())
            .map_err(|e| ToolError::io(format!("Failed to write file: {e}")))?;

        let bytes_written = content.encode_utf16().count();
        Ok(text_output(format!("Successfully wrote {bytes_written} bytes to {path}")))
    }

    /// `{path(必填), old_text(必填), new_text(必填)}` → 字面替换。
    /// 简化：替换**全部**出现（legacy 只换第一处 + 模糊匹配）；返回替换次数。
    fn edit(&self, input: &serde_json::Value) -> ToolResult {
        let path = input.get("path").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("edit: path is required"))?;
        let old_text = input.get("old_text").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("edit: old_text is required"))?;
        let new_text = input.get("new_text").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("edit: new_text is required"))?;
        if old_text.is_empty() {
            return Err(ToolError::invalid("edit: old_text must not be empty"));
        }

        let resolved = self.resolve(path);
        let content = std::fs::read_to_string(&resolved)
            .map_err(|e| ToolError::io(format!("edit failed for {}: {e}", resolved.display())))?;
        let replaced = content.matches(old_text).count();
        let next = content.replace(old_text, new_text);
        if next.len() > WRITE_TOOL_MAX_BYTES {
            return Err(ToolError::invalid(format!(
                "Edited content exceeds maximum allowed ({WRITE_TOOL_MAX_BYTES} bytes)"
            )));
        }
        atomic_write(&resolved, next.as_bytes())
            .map_err(|e| ToolError::io(format!("Failed to write file: {e}")))?;
        Ok(text_output(format!("Successfully replaced {replaced} occurrence(s) in {path}")))
    }

    /// `{pattern(必填), path?, ignore_case?, limit?, timeout?}` → 递归搜索。
    /// 遍历尊重 .gitignore/.ignore 并跳过隐藏文件（ignore crate，ripgrep 库族）
    /// ——M1 验收问题 2：纯 std 递归把 target/ 等被忽略目录全趟一遍，单次卡 ~4 分钟。
    /// 同步遍历放阻塞线程池 + timeout 兜底：超时立即返回错误（残留遍历跑完即弃，
    /// 不再挂死回合）。literal 恒按字面子串匹配（fancy-regex 留给未来的正则档），
    /// 输出行格式 `rel/path:line:text`，命中上限默认 100。
    async fn grep(&self, input: &serde_json::Value) -> ToolResult {
        let pattern = input.get("pattern").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("grep: pattern is required"))?;
        let root = match input.get("path").and_then(serde_json::Value::as_str) {
            Some(p) => self.resolve(p),
            None => self.cwd.clone(),
        };
        let ignore_case = input.get("ignore_case").and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let limit = input.get("limit").and_then(serde_json::Value::as_u64)
            .map(|l| l as usize)
            .unwrap_or(DEFAULT_LIST_LIMIT);
        let timeout_ms = input.get("timeout").and_then(serde_json::Value::as_u64)
            .filter(|t| *t > 0)
            .unwrap_or(DEFAULT_GREP_TIMEOUT_MS);

        let (root, pattern) = (root.clone(), pattern.to_string());
        let task = tokio::task::spawn_blocking(move || grep_walk(&pattern, &root, ignore_case, limit));
        match tokio::time::timeout(Duration::from_millis(timeout_ms), task).await {
            Ok(Ok(out)) => Ok(text_output(out)),
            Ok(Err(e)) => Err(ToolError::io(format!("grep task failed: {e}"))),
            Err(_) => Err(ToolError::timeout(format!(
                "grep timed out after {timeout_ms}ms (narrow path or raise timeout)"
            ))),
        }
    }

    /// `{pattern(必填), path?, limit?, timeout?}` → 递归找文件名包含 pattern 的条目。
    /// 遍历同样尊重 .gitignore 且有超时兜底（同 grep，M1 验收问题 2）。
    /// 简化：子串匹配（legacy 支持 glob）；输出每行一个相对路径。
    async fn find(&self, input: &serde_json::Value) -> ToolResult {
        let pattern = input.get("pattern").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("find: pattern is required"))?;
        let root = match input.get("path").and_then(serde_json::Value::as_str) {
            Some(p) => self.resolve(p),
            None => self.cwd.clone(),
        };
        let limit = input.get("limit").and_then(serde_json::Value::as_u64)
            .map(|l| l as usize)
            .unwrap_or(DEFAULT_LIST_LIMIT);
        let timeout_ms = input.get("timeout").and_then(serde_json::Value::as_u64)
            .filter(|t| *t > 0)
            .unwrap_or(DEFAULT_GREP_TIMEOUT_MS);

        let (root, pattern) = (root.clone(), pattern.to_string());
        let task = tokio::task::spawn_blocking(move || find_walk(&pattern, &root, limit));
        match tokio::time::timeout(Duration::from_millis(timeout_ms), task).await {
            Ok(Ok(out)) => Ok(text_output(out)),
            Ok(Err(e)) => Err(ToolError::io(format!("find task failed: {e}"))),
            Err(_) => Err(ToolError::timeout(format!(
                "find timed out after {timeout_ms}ms (narrow path or raise timeout)"
            ))),
        }
    }

    /// `{path?, limit?}` → 列目录（目录后缀 `/`）。
    fn ls(&self, input: &serde_json::Value) -> ToolResult {
        let root = match input.get("path").and_then(serde_json::Value::as_str) {
            Some(p) => self.resolve(p),
            None => self.cwd.clone(),
        };
        let limit = input.get("limit").and_then(serde_json::Value::as_u64)
            .map(|l| l as usize)
            .unwrap_or(DEFAULT_LIST_LIMIT);

        let entries = std::fs::read_dir(&root)
            .map_err(|e| ToolError::io(format!("ls failed for {}: {e}", root.display())))?;
        let mut out = String::new();
        let mut count = 0usize;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            out.push_str(&format!("{}{}\n", name, if is_dir { "/" } else { "" }));
            count += 1;
            if count >= limit {
                break;
            }
        }
        Ok(text_output(out))
    }

    /// `{command(必填), timeout?, cwd?}` → 跑 shell 命令（Windows: cmd /C；
    /// 其余: /bin/sh -c）。输出对齐 pi.exec 形状 `{stdout, stderr, code, killed}`。
    async fn bash(&self, input: &serde_json::Value) -> ToolResult {
        let command = input.get("command").and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("bash: command is required"))?;
        let timeout_ms = input.get("timeout").and_then(serde_json::Value::as_u64)
            .filter(|t| *t > 0)
            .unwrap_or(DEFAULT_BASH_TIMEOUT_MS);
        let cwd = match input.get("cwd").and_then(serde_json::Value::as_str) {
            Some(c) => self.resolve(c),
            None => self.cwd.clone(),
        };
        let (program, arg) = shell_program();
        run_command(program, [arg, command], &cwd, timeout_ms).await
    }

    /// `pi.exec(cmd, {args, options})` 的进程执行侧（非 shell 包装，直接
    /// spawn cmd + args）。输出形状与 bash 相同 `{stdout, stderr, code, killed}`
    /// ——对齐 legacy 非流式 exec hostcall 的返回。
    pub async fn exec_cmd(
        &self,
        cmd: &str,
        args: &[String],
        cwd: Option<&str>,
        timeout_ms: u64,
    ) -> ToolResult {
        let cwd = match cwd {
            Some(c) => self.resolve(c),
            None => self.cwd.clone(),
        };
        let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
        run_command(cmd, &args_ref, &cwd, timeout_ms).await
    }
}

/// 进程执行公共路径：spawn → 双管道并发读 → 轮询等待（超时 kill）。
/// 返回 `{stdout, stderr, code, killed}`。
async fn run_command<I, S>(program: &str, args: I, cwd: &Path, timeout_ms: u64) -> ToolResult
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| ToolError::io(format!("command spawn failed for {program}: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::io("missing stdout pipe"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::io("missing stderr pipe"))?;

    // 非阻塞读输出：stdout/stderr 双管道串行读会互相阻塞，故并发
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(bool, Vec<u8>)>();
    let tx_out = tx.clone();
    tokio::spawn(async move {
        let mut pipe = stdout;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        let _ = tx_out.send((true, buf));
    });
    let tx_err = tx.clone();
    tokio::spawn(async move {
        let mut pipe = stderr;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        let _ = tx_err.send((false, buf));
    });
    drop(tx);

    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        if tokio::time::Instant::now() >= deadline {
            child.kill().await.ok();
            break None; // killed
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => tokio::time::sleep(Duration::from_millis(10)).await,
            Err(e) => return Err(ToolError::io(format!("command wait failed: {e}"))),
        }
    };
    let killed = status.is_none();
    let code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

    let mut stdout_text = String::new();
    let mut stderr_text = String::new();
    while let Some((is_stdout, bytes)) = rx.recv().await {
        if is_stdout {
            stdout_text.push_str(&String::from_utf8_lossy(&bytes));
        } else {
            stderr_text.push_str(&String::from_utf8_lossy(&bytes));
        }
    }

    Ok(serde_json::json!({
        "stdout": stdout_text,
        "stderr": stderr_text,
        "code": code,
        "killed": killed,
    }))
}

/// 平台 shell：Windows 用 cmd /C，其余 /bin/sh -c。
fn shell_program() -> (&'static str, &'static str) {
    #[cfg(windows)]
    {
        ("cmd", "/C")
    }
    #[cfg(not(windows))]
    {
        ("/bin/sh", "-c")
    }
}

/// 相对路径输出统一正斜杠（跨平台稳定，对齐 pi 的相对路径语义）。
fn rel_slashes(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// 递归遍历文件（忽略遍历错误，深度优先）：尊重 .gitignore/.ignore、
/// 跳过隐藏文件、不跟随符号链接（ignore crate WalkBuilder 默认行为，与
/// ripgrep 一致）——M1 验收问题 2 修复：纯 std 递归会把 target/ 等被忽略
/// 目录全趟一遍（单次卡 ~4 分钟），ignore 过滤后正常仓库搜索亚秒级。
fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root).build() {
        let Ok(entry) = entry else { continue };
        // 不跟随符号链接：file_type() 对 symlink 为 None，与旧实现一致跳过
        if entry.file_type().is_some_and(|t| t.is_file()) {
            out.push(entry.into_path());
        }
    }
    out
}

/// grep 同步搜索体（spawn_blocking 内执行，见 BuiltinTools::grep）。
/// 输出行格式 `rel/path:line:text`，命中上限 limit。
fn grep_walk(pattern: &str, root: &Path, ignore_case: bool, limit: usize) -> String {
    let haystack = if ignore_case { pattern.to_lowercase() } else { pattern.to_string() };
    let mut out = String::new();
    let mut hits = 0usize;
    for entry in walk_files(root) {
        if hits >= limit {
            break;
        }
        let Ok(text) = std::fs::read_to_string(&entry) else { continue };
        for (idx, line) in text.lines().enumerate() {
            let candidate = if ignore_case { line.to_lowercase() } else { line.to_string() };
            if candidate.contains(&haystack) {
                let rel = entry.strip_prefix(root).unwrap_or(&entry);
                out.push_str(&format!("{}:{}:{}\n", rel_slashes(rel), idx + 1, line));
                hits += 1;
                if hits >= limit {
                    break;
                }
            }
        }
    }
    out
}

/// find 同步搜索体（spawn_blocking 内执行，见 BuiltinTools::find）。
/// 输出每行一个相对路径，上限 limit。
fn find_walk(pattern: &str, root: &Path, limit: usize) -> String {
    let mut out = String::new();
    let mut hits = 0usize;
    for entry in walk_files(root) {
        if hits >= limit {
            break;
        }
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if !name.contains(pattern) {
            continue;
        }
        let rel = entry.strip_prefix(root).unwrap_or(&entry);
        out.push_str(&format!("{}\n", rel_slashes(rel)));
        hits += 1;
    }
    out
}

/// 临时文件 + rename 原子落盘（同目录 rename 保证原子性）。
fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write as _;
    tmp.write_all(content)?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(dir: &std::path::Path) -> BuiltinTools {
        BuiltinTools::new(dir.to_path_buf())
    }

    /// 取工具输出的 content[0].text（统一成功形状）。
    fn text_of(v: &serde_json::Value) -> &str {
        v["content"][0]["text"].as_str().unwrap_or("")
    }

    #[test]
    fn read_writes_and_reads_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());

        let w = t.write(&serde_json::json!({ "path": "a/b.txt", "content": "你好\nBoen" })).unwrap();
        assert!(text_of(&w).contains("Successfully wrote"), "{w}");
        assert!(text_of(&w).contains("a/b.txt"), "{w}");

        let r = t.read(&serde_json::json!({ "path": "a/b.txt" })).unwrap();
        // 输出形状 = {content:[{type:text,text}]}（对齐 pi ToolOutput 序列化）
        assert_eq!(r["content"][0]["type"], "text");
        assert_eq!(r["content"][0]["text"], "你好\nBoen");
        // offset/limit 按字节截取（3 字节处=「好」，取 4 字节=「好\n」）
        let r2 = t.read(&serde_json::json!({ "path": "a/b.txt", "offset": 3, "limit": 4 })).unwrap();
        assert_eq!(r2["content"][0]["text"], "好\n");
    }

    #[test]
    fn read_requires_path() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        let err = t.read(&serde_json::json!({})).unwrap_err();
        assert_eq!(err.code, "invalid_request");
    }

    #[test]
    fn edit_replaces_all_occurrences() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        std::fs::write(dir.path().join("f.txt"), "axa axa").unwrap();
        let out = t.edit(&serde_json::json!({
            "path": "f.txt", "old_text": "axa", "new_text": "b"
        })).unwrap();
        assert!(text_of(&out).contains("2 occurrence"), "{out}");
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "b b");
    }

    #[tokio::test]
    async fn grep_finds_lines_with_path_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/x.txt"), "hello\nworld\nhello again").unwrap();
        let out = t.grep(&serde_json::json!({ "pattern": "hello" })).await.unwrap();
        let text = out["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("sub/x.txt:1:hello"), "{text}");
        assert!(text.contains("sub/x.txt:3:hello again"), "{text}");
        // 大小写不敏感
        let out2 = t.grep(&serde_json::json!({ "pattern": "WORLD", "ignore_case": true })).await.unwrap();
        assert!(out2["content"][0]["text"].as_str().unwrap().contains(":2:world"));
    }

    #[tokio::test]
    async fn grep_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        // ignore crate 语义对齐 ripgrep：.gitignore 只在 git 仓库内生效
        // （仓库外仅 .ignore 生效）；空 .git 目录即标记为仓库
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir_all(dir.path().join("ignored")).unwrap();
        std::fs::write(dir.path().join("ignored/secret.txt"), "needle").unwrap();
        std::fs::write(dir.path().join("visible.txt"), "needle").unwrap();
        let out = t.grep(&serde_json::json!({ "pattern": "needle" })).await.unwrap();
        let text = out["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("visible.txt"), "{text}");
        assert!(!text.contains("secret.txt"), "{text}");
    }

    #[tokio::test]
    async fn grep_times_out_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        // 预放 2000 个文件：grep 需逐个 read_to_string，1ms 超时必然触发
        for i in 0..2000 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "x").unwrap();
        }
        let err = t.grep(&serde_json::json!({ "pattern": "needle", "timeout": 1 })).await.unwrap_err();
        assert_eq!(err.code, "timeout", "{err}");
    }

    #[tokio::test]
    async fn find_and_ls_scope_to_root() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        std::fs::create_dir_all(dir.path().join("sub/deep")).unwrap();
        std::fs::write(dir.path().join("sub/deep/needle.ts"), "x").unwrap();
        std::fs::write(dir.path().join("other.txt"), "x").unwrap();

        let out = t.find(&serde_json::json!({ "pattern": "needle" })).await.unwrap();
        let text = out["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("needle.ts"), "{text}");
        assert!(!text.contains("other.txt"), "{text}");

        let out = t.ls(&serde_json::json!({ "path": "sub" })).unwrap();
        assert!(out["content"][0]["text"].as_str().unwrap().contains("deep/"));
    }

    #[tokio::test]
    async fn unknown_tool_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        let err = t.execute("not-builtin", serde_json::json!({})).await.unwrap_err();
        assert_eq!(err.code, "invalid_request");
        assert!(err.message.contains("Unknown tool"), "{err}");
    }

    #[tokio::test]
    async fn bash_runs_shell_and_captures() {
        let dir = tempfile::tempdir().unwrap();
        let t = tools(dir.path());
        #[cfg(windows)]
        let command = "echo hello-boen";
        #[cfg(not(windows))]
        let command = "echo hello-boen";
        let out = t.bash(&serde_json::json!({ "command": command })).await.unwrap();
        assert_eq!(out["code"], 0, "{out}");
        assert!(out["stdout"].as_str().unwrap().contains("hello-boen"), "{out}");
        assert!(!out["killed"].as_bool().unwrap_or(true), "{out}");
    }
}
