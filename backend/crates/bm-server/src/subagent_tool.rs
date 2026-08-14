//! subagent 内置工具：专家团队在 bm 引擎的落地。
//!
//! 忠实移植 pi 父侧 `subagents.rs`（发现角色定义 → spawn 子进程 → 摄取
//! stdout JSON 事件流 → 结构化结果回给模型）；子进程协议不变——子进程 =
//! bm-server 自身 `--mode json` 子代理入口（`subagent_child.rs`），当前仍以
//! pi SDK 跑隔离会话，**阶段化废除 pi 的第二步才把子进程也换成 bm-loop**。
//!
//! 与上游的差异（防漂移台账，对应 HANDOFF 的 UPSTREAM_PATCHES 纪律）：
//! - 并发：tokio（JoinSet + Semaphore），不引入 futures 依赖；
//! - 取消：loop 的 cancel 丢弃 execute future → tokio Child `kill_on_drop`
//!   杀子进程（上游 AgentCx checkpoint 机制的 Drop 守卫等价物）；
//! - 进度回调（ToolUpdate）省略：ToolExecutor 契约无此口，前端工具卡片走
//!   loop 的 on_tool_pre/post；`--skill` 参数省略（子进程第一版本就不消费）。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use bm_loop::engine::ToolOutcome;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader as TokioBufReader};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const MAX_PARALLEL_TASKS: usize = 8;
const DEFAULT_CONCURRENCY: usize = 4;
const MAX_SUBAGENT_DEPTH: usize = 3;
/// 子进程输出/错误流上限（对齐上游 MAX_CHILD_OUTPUT_BYTES）
const MAX_CHILD_OUTPUT_BYTES: usize = 256 * 1024;
const DEFAULT_CHILD_TOOLS: &str = "read,bash,edit,write,grep,find,ls,hashline_edit";
/// 结构化结果块字段/整块上限（对齐 BoenMind P9 补丁）
const FIELD_LIMIT: usize = 2000;
const BLOCK_LIMIT: usize = 16 * 1024;

/// 工具定义（进 ToolRegistry = 模型可见面；参数面与 pi subagent 对齐）。
pub fn tool_def() -> bm_loop::model::ToolDef {
    bm_loop::model::ToolDef::new(
        "subagent",
        "Delegate an isolated task to a named sub-agent. Supports one task, bounded parallel tasks, or a sequential chain whose tasks may reference {previous}. Agent definitions live in <pi dir>/agents/*.md or .pi/agents/*.md.",
        json!({
            "type": "object",
            "properties": {
                "agent": {"type": "string", "description": "Named agent for a single delegation."},
                "task": {"type": "string", "description": "Task for a single delegation."},
                "tasks": {"type": "array", "maxItems": MAX_PARALLEL_TASKS, "items": {"$ref": "#/definitions/task"}, "description": "Independent tasks to run in parallel."},
                "chain": {"type": "array", "maxItems": MAX_PARALLEL_TASKS, "items": {"$ref": "#/definitions/task"}, "description": "Sequential tasks; {previous} is replaced with the prior child output."},
                "concurrency": {"type": "integer", "minimum": 1, "maximum": MAX_PARALLEL_TASKS},
                "scope": {"type": "string", "enum": ["both", "user", "project"], "default": "both"}
            },
            "definitions": {
                "task": {
                    "type": "object",
                    "required": ["agent", "task"],
                    "properties": {
                        "agent": {"type": "string"},
                        "task": {"type": "string"},
                        "cwd": {"type": "string"}
                    }
                }
            },
            "additionalProperties": false
        }),
    )
}

/// 执行 subagent 请求；ok=false 时 output 为模型可读的错误说明。
pub async fn run(input: Value, working_dir: &Path) -> ToolOutcome {
    if current_subagent_depth() >= MAX_SUBAGENT_DEPTH {
        return err_outcome(format!(
            "Refusing nested subagent depth above {MAX_SUBAGENT_DEPTH}; child agents are isolated and do not receive the subagent tool."
        ));
    }
    let request: SubagentRequest = match serde_json::from_value(input) {
        Ok(r) => r,
        Err(e) => return err_outcome(format!("Invalid input: {e}")),
    };
    let mode = match request.mode() {
        Ok(m) => m,
        Err(e) => return err_outcome(e),
    };
    let agents = match discover_agents(&bm_core::config::agents_dir(), working_dir, request.scope) {
        Ok(a) => Arc::new(a),
        Err(e) => return err_outcome(e),
    };
    let results = match mode {
        RequestMode::Single(task) => vec![run_one(&agents, &task, None, working_dir).await],
        RequestMode::Parallel(tasks) => run_parallel(&agents, tasks, request.concurrency, working_dir).await,
        RequestMode::Chain(tasks) => run_chain(&agents, tasks, working_dir).await,
    };
    let is_error = results.iter().any(|r| r.is_error);
    // 结构化块放正文开头（P9 同款）：模型先读字段，大输出修剪保留头部时不砍掉
    let content = format!(
        "<subagent-structured-result>\n{}\n</subagent-structured-result>\n\n{}",
        structured_result_block(&results),
        render_results(&results),
    );
    ToolOutcome {
        ok: !is_error,
        output: content,
        meta: None,
    }
}

fn err_outcome(message: String) -> ToolOutcome {
    ToolOutcome {
        ok: false,
        output: message,
        meta: None,
    }
}

// ============================================================================
// 请求形状与模式校验（与 pi 对齐：单任务 / 并行 / 串行链 三选一）
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentRequest {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    tasks: Option<Vec<SubagentTask>>,
    #[serde(default)]
    chain: Option<Vec<SubagentTask>>,
    #[serde(default)]
    concurrency: Option<usize>,
    #[serde(default)]
    scope: AgentScope,
}

#[derive(Debug, Clone, Deserialize)]
struct SubagentTask {
    agent: String,
    task: String,
    #[serde(default)]
    cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AgentScope {
    User,
    Project,
    #[default]
    Both,
}

enum RequestMode {
    Single(SubagentTask),
    Parallel(Vec<SubagentTask>),
    Chain(Vec<SubagentTask>),
}

impl SubagentRequest {
    fn mode(&self) -> Result<RequestMode, String> {
        let single = self
            .agent
            .as_ref()
            .zip(self.task.as_ref())
            .map(|(agent, task)| SubagentTask {
                agent: agent.clone(),
                task: task.clone(),
                cwd: None,
            });
        let selected = usize::from(single.is_some())
            + usize::from(self.tasks.is_some())
            + usize::from(self.chain.is_some());
        if selected != 1 {
            return Err("Provide exactly one of agent+task, tasks, or chain.".into());
        }
        if self.agent.is_some() != self.task.is_some() {
            return Err("Single delegation requires both agent and task.".into());
        }
        if let Some(tasks) = &self.tasks
            && (tasks.is_empty() || tasks.len() > MAX_PARALLEL_TASKS)
        {
            return Err(format!("tasks must contain 1-{MAX_PARALLEL_TASKS} entries."));
        }
        if let Some(chain) = &self.chain
            && (chain.is_empty() || chain.len() > MAX_PARALLEL_TASKS)
        {
            return Err(format!("chain must contain 1-{MAX_PARALLEL_TASKS} entries."));
        }
        Ok(single.map_or_else(
            || {
                self.tasks.as_ref().map_or_else(
                    || RequestMode::Chain(self.chain.clone().unwrap_or_default()),
                    |tasks| RequestMode::Parallel(tasks.clone()),
                )
            },
            RequestMode::Single,
        ))
    }
}

// ============================================================================
// 角色发现（<pi dir>/agents/*.md + 项目 .pi/agents/*.md，frontmatter 解析）
// ============================================================================

#[derive(Debug, Clone)]
struct AgentDefinition {
    name: String,
    description: String,
    model: Option<String>,
    reasoning: Option<String>,
    tools: Option<Vec<String>>,
    system_prompt: String,
    source: AgentSource,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AgentSource {
    User,
    Project,
}

fn discover_agents(
    global_dir: &Path,
    cwd: &Path,
    scope: AgentScope,
) -> Result<BTreeMap<String, AgentDefinition>, String> {
    let mut agents = BTreeMap::new();
    if !matches!(scope, AgentScope::Project) {
        load_agent_dir(&global_dir.join("agents"), AgentSource::User, &mut agents)?;
    }
    if !matches!(scope, AgentScope::User)
        && let Some(project_dir) = nearest_project_agents_dir(cwd)
    {
        // 项目定义同名覆盖用户定义（与上游一致）
        load_agent_dir(&project_dir, AgentSource::Project, &mut agents)?;
    }
    Ok(agents)
}

fn nearest_project_agents_dir(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        let candidate = current.join(".pi").join("agents");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn load_agent_dir(
    directory: &Path,
    source: AgentSource,
    agents: &mut BTreeMap<String, AgentDefinition>,
) -> Result<(), String> {
    if !directory.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(directory)
        .map_err(|e| format!("Cannot read agent directory {}: {e}", directory.display()))?;
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read agent definition {}: {e}", path.display()))?;
        let (frontmatter, body) = parse_frontmatter(&raw);
        let name = required_agent_field(&frontmatter, "name", &path)?;
        let description = required_agent_field(&frontmatter, "description", &path)?;
        let tools = frontmatter.get("tools").map(|value| split_csv(value));
        agents.insert(
            name.clone(),
            AgentDefinition {
                name,
                description,
                model: frontmatter.get("model").cloned(),
                reasoning: frontmatter
                    .get("reasoning")
                    .or_else(|| frontmatter.get("thinking"))
                    .cloned(),
                tools,
                system_prompt: body,
                source,
            },
        );
    }
    Ok(())
}

fn required_agent_field(
    fields: &BTreeMap<String, String>,
    field: &str,
    path: &Path,
) -> Result<String, String> {
    fields
        .get(field)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            format!(
                "Agent definition {} requires frontmatter field {field:?}",
                path.display()
            )
        })
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_frontmatter(raw: &str) -> (BTreeMap<String, String>, String) {
    let mut lines = raw.lines();
    if !matches!(lines.next(), Some(first) if first.trim().eq("---")) {
        return (BTreeMap::new(), raw.to_string());
    }
    let mut fields = BTreeMap::new();
    let mut body = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim().eq("---") {
            closed = true;
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            if !key.is_empty() {
                fields.insert(
                    key.to_string(),
                    value.trim().trim_matches('"').trim_matches('\'').to_string(),
                );
            }
        }
    }
    if !closed {
        return (BTreeMap::new(), raw.to_string());
    }
    body.extend(lines);
    (fields, body.join("\n"))
}

// ============================================================================
// 执行：spawn 子进程 → 摄取 stdout JSON 事件流（协议见 subagent_child.rs）
// ============================================================================

async fn run_parallel(
    agents: &BTreeMap<String, AgentDefinition>,
    tasks: Vec<SubagentTask>,
    concurrency: Option<usize>,
    working_dir: &Path,
) -> Vec<SubagentResult> {
    let limit = concurrency
        .unwrap_or(DEFAULT_CONCURRENCY)
        .clamp(1, MAX_PARALLEL_TASKS);
    let sem = Arc::new(Semaphore::new(limit));
    let mut set = JoinSet::new();
    for (index, task) in tasks.into_iter().enumerate() {
        // 注意 (*agents).clone()：&T 的 clone 是克隆引用本身，必须解引用深拷贝
        let agents: Arc<BTreeMap<String, AgentDefinition>> = Arc::new((*agents).clone());
        let working_dir = working_dir.to_path_buf();
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            // 闭包内显式克隆再借用：借用归属闭包自身（Arc 浅克隆，代价可忽略）
            let owned_agents = agents.clone();
            (index, run_one(&owned_agents, &task, None, &working_dir).await)
        });
    }
    let mut results = Vec::with_capacity(set.len());
    while let Some(joined) = set.join_next().await {
        results.push(joined.expect("subagent task panicked"));
    }
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

async fn run_chain(
    agents: &BTreeMap<String, AgentDefinition>,
    tasks: Vec<SubagentTask>,
    working_dir: &Path,
) -> Vec<SubagentResult> {
    let mut previous = String::new();
    let mut results = Vec::with_capacity(tasks.len());
    for (step, mut task) in tasks.into_iter().enumerate() {
        task.task = task.task.replace("{previous}", &previous);
        let result = run_one(agents, &task, Some(step + 1), working_dir).await;
        previous.clone_from(&result.output);
        let failed = result.is_error;
        results.push(result);
        if failed {
            break;
        }
    }
    results
}

async fn run_one(
    agents: &BTreeMap<String, AgentDefinition>,
    task: &SubagentTask,
    step: Option<usize>,
    working_dir: &Path,
) -> SubagentResult {
    let Some(agent) = agents.get(&task.agent) else {
        return SubagentResult::unknown(task, step);
    };
    let cwd = task.cwd.clone().unwrap_or_else(|| working_dir.to_path_buf());
    if !cwd.is_dir() {
        return SubagentResult::failed(
            agent,
            step,
            format!("Working directory does not exist: {}", cwd.display()),
        );
    }
    let mut result = SubagentResult::starting(agent, step);
    let args = child_args(agent, &task.task);
    let binary = std::env::var_os("PI_SUBAGENT_PI_BINARY")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("<current executable unavailable>"));
    result.binary = binary.clone();

    let global_dir = bm_core::config::agents_dir();
    let mut command = Command::new(&binary);
    command
        .args(&args)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PI_CODING_AGENT_DIR", &global_dir)
        .env("PI_SUBAGENT_PARENT_PID", std::process::id().to_string())
        .env("PI_SUBAGENT_DEPTH", (current_subagent_depth() + 1).to_string())
        // 取消传播：父侧 cancel 丢弃 execute future → Child drop → 子进程被杀
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            result.fail(format!("Failed to launch {}: {error}", binary.display()));
            return result;
        }
    };
    result.pid = child.id();
    result.status = SubagentStatus::Running;

    // 两流并发逐行摄取；EOF 后 wait 拿退出码（流 EOF ≠ 进程已收尾）
    let mut stdout_lines = child
        .stdout
        .take()
        .map(|stdout| TokioBufReader::new(stdout).lines());
    let mut stderr_lines = child
        .stderr
        .take()
        .map(|stderr| TokioBufReader::new(stderr).lines());
    let mut stdout_done = stdout_lines.is_none();
    let mut stderr_done = stderr_lines.is_none();
    loop {
        tokio::select! {
            line = next_stdout_line(&mut stdout_lines), if !stdout_done => match line {
                Some(Ok(line)) => ingest_child_event(&line, &mut result),
                _ => stdout_done = true,
            },
            line = next_stderr_line(&mut stderr_lines), if !stderr_done => match line {
                Some(Ok(line)) => append_bounded_line(&mut result.stderr, &line),
                _ => stderr_done = true,
            },
        }
        if stdout_done && stderr_done {
            break;
        }
    }
    match child.wait().await {
        Ok(status) => result.exit_code = status.code(),
        Err(error) => result.fail(format!("Failed while waiting for child: {error}")),
    }
    if result.exit_code == Some(0) && !result.is_error {
        result.status = SubagentStatus::Completed;
    } else if !result.is_error {
        result.fail(format!(
            "Child exited with code {}.",
            result.exit_code.unwrap_or(-1)
        ));
    }
    result
}

async fn next_stdout_line(
    lines: &mut Option<tokio::io::Lines<TokioBufReader<tokio::process::ChildStdout>>>,
) -> Option<std::io::Result<String>> {
    lines.as_mut()?.next_line().await.transpose()
}

async fn next_stderr_line(
    lines: &mut Option<tokio::io::Lines<TokioBufReader<tokio::process::ChildStderr>>>,
) -> Option<std::io::Result<String>> {
    lines.as_mut()?.next_line().await.transpose()
}

fn child_args(agent: &AgentDefinition, task: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--mode".into(),
        "json".into(),
        "--print".into(),
        "--no-session".into(),
        "--tools".into(),
        agent
            .tools
            .as_ref()
            .map_or_else(|| DEFAULT_CHILD_TOOLS.to_string(), |tools| tools.join(","))
            .into(),
    ];
    if let Some(model) = &agent.model {
        args.extend(["--model".into(), model.clone().into()]);
    }
    if let Some(reasoning) = &agent.reasoning {
        args.extend(["--thinking".into(), reasoning.clone().into()]);
    }
    if !agent.system_prompt.trim().is_empty() {
        args.extend([
            "--append-system-prompt".into(),
            agent.system_prompt.clone().into(),
        ]);
    }
    args.push(format!("Task: {task}").into());
    args
}

fn current_subagent_depth() -> usize {
    std::env::var("PI_SUBAGENT_DEPTH")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default()
}

// ============================================================================
// 结果与渲染（结构化块 = BoenMind P9 补丁同款；模型先读字段后读正文）
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SubagentStatus {
    Starting,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
struct SubagentResult {
    agent: String,
    description: Option<String>,
    step: Option<usize>,
    source: Option<AgentSource>,
    model: Option<String>,
    reasoning: Option<String>,
    tools: Vec<String>,
    binary: PathBuf,
    pid: Option<u32>,
    status: SubagentStatus,
    exit_code: Option<i32>,
    output: String,
    stderr: String,
    error: Option<String>,
    is_error: bool,
}

impl SubagentResult {
    fn starting(agent: &AgentDefinition, step: Option<usize>) -> Self {
        Self {
            agent: agent.name.clone(),
            description: Some(agent.description.clone()),
            step,
            source: Some(agent.source),
            model: agent.model.clone(),
            reasoning: agent.reasoning.clone(),
            tools: agent
                .tools
                .clone()
                .unwrap_or_else(|| split_csv(DEFAULT_CHILD_TOOLS)),
            binary: PathBuf::new(),
            pid: None,
            status: SubagentStatus::Starting,
            exit_code: None,
            output: String::new(),
            stderr: String::new(),
            error: None,
            is_error: false,
        }
    }

    fn unknown(task: &SubagentTask, step: Option<usize>) -> Self {
        Self {
            agent: task.agent.clone(),
            description: None,
            step,
            source: None,
            model: None,
            reasoning: None,
            tools: Vec::new(),
            binary: PathBuf::new(),
            pid: None,
            status: SubagentStatus::Failed,
            exit_code: None,
            output: String::new(),
            stderr: String::new(),
            error: Some(format!("Unknown agent: {}", task.agent)),
            is_error: true,
        }
    }

    fn failed(agent: &AgentDefinition, step: Option<usize>, error: String) -> Self {
        let mut result = Self::starting(agent, step);
        result.fail(error);
        result
    }

    fn fail(&mut self, error: String) {
        self.status = SubagentStatus::Failed;
        self.error = Some(error);
        self.is_error = true;
    }
}

fn render_results(results: &[SubagentResult]) -> String {
    results
        .iter()
        .map(|result| {
            let heading = result
                .step
                .map_or_else(|| result.agent.clone(), |step| format!("step {step}: {}", result.agent));
            let body = if result.output.trim().is_empty() {
                result.error.as_deref().unwrap_or("(no output)")
            } else {
                result.output.trim()
            };
            format!("## {heading}\n{body}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 紧凑 JSON 块（模型可直接读取的结构化字段）；output/stderr 截断 2000 字符、
/// 整块上限 16KB，避免大输出进模型上下文。
fn structured_result_block(results: &[SubagentResult]) -> String {
    let compact: Vec<serde_json::Value> = results
        .iter()
        .map(|result| {
            serde_json::json!({
                "agent": result.agent,
                "description": result.description,
                "step": result.step,
                "source": result.source,
                "model": result.model,
                "reasoning": result.reasoning,
                "tools": result.tools,
                "binary": result.binary,
                "pid": result.pid,
                "status": result.status,
                "exitCode": result.exit_code,
                "output": truncate_owned(&result.output, FIELD_LIMIT),
                "stderr": truncate_owned(&result.stderr, FIELD_LIMIT),
                "error": result.error,
            })
        })
        .collect();
    let mut json = serde_json::to_string(&compact).unwrap_or_else(|_| "[]".to_string());
    if json.len() > BLOCK_LIMIT {
        json.truncate(BLOCK_LIMIT);
        json.push('…');
    }
    json
}

fn truncate_owned(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_string()
    } else {
        let mut cut = text[..limit].to_string();
        cut.push('…');
        cut
    }
}

fn append_bounded(target: &mut String, value: &str) {
    if target.len() >= MAX_CHILD_OUTPUT_BYTES {
        return;
    }
    let remaining = MAX_CHILD_OUTPUT_BYTES - target.len();
    if value.len() <= remaining {
        target.push_str(value);
    } else {
        target.push_str(&value[..remaining]);
    }
}

fn append_bounded_line(target: &mut String, value: &str) {
    append_bounded(target, value);
    if target.len() < MAX_CHILD_OUTPUT_BYTES {
        target.push('\n');
    }
}

/// stdout 协议行 → 结果（协议见 subagent_child.rs：message_update 正文增量 /
/// message_end 权威内容 / agent_end messages 兜底；其余忽略）。
fn ingest_child_event(line: &str, result: &mut SubagentResult) {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        append_bounded_line(&mut result.stderr, line);
        return;
    };
    match event.get("type").and_then(Value::as_str) {
        Some("message_update") => {
            if let Some(delta) = event
                .pointer("/assistantMessageEvent/delta")
                .and_then(Value::as_str)
            {
                append_bounded(&mut result.output, delta);
            }
        }
        Some("message_end") => {
            if let Some(text) = assistant_text(event.get("message"))
                && result.output.is_empty()
            {
                append_bounded_line(&mut result.output, &text);
            }
        }
        Some("agent_end") => {
            if result.output.is_empty()
                && let Some(messages) = event.get("messages").and_then(Value::as_array)
            {
                for message in messages.iter().rev() {
                    if let Some(text) = assistant_text(Some(message)) {
                        append_bounded_line(&mut result.output, &text);
                        break;
                    }
                }
            }
        }
        _ => {}
    }
}

fn assistant_text(message: Option<&Value>) -> Option<String> {
    let message = message?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let content = message.get("content")?.as_array()?;
    content.iter().find_map(|block| {
        (block.get("type").and_then(Value::as_str) == Some("text")).then(|| {
            block
                .get("text")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .flatten()
    })
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> AgentDefinition {
        AgentDefinition {
            name: "researcher".into(),
            description: "调研员".into(),
            model: Some("deepseek-chat".into()),
            reasoning: Some("low".into()),
            tools: Some(vec!["read".into(), "bash".into()]),
            system_prompt: "你是调研员。".into(),
            source: AgentSource::User,
        }
    }

    fn task(agent_name: &str, text: &str) -> SubagentTask {
        SubagentTask {
            agent: agent_name.into(),
            task: text.into(),
            cwd: None,
        }
    }

    #[test]
    fn frontmatter_parses_fields_and_body() {
        let raw = "---\nname: researcher\ndescription: 调研员\ntools: read, bash\nmodel: deepseek-chat\n---\n你是调研员。\n多行正文。";
        let (fields, body) = parse_frontmatter(raw);
        assert_eq!(fields.get("name").map(String::as_str), Some("researcher"));
        assert_eq!(fields.get("tools").map(String::as_str), Some("read, bash"));
        assert_eq!(body, "你是调研员。\n多行正文。");
    }

    #[test]
    fn frontmatter_missing_or_unclosed_returns_raw_as_body() {
        let (fields, body) = parse_frontmatter("没有 frontmatter\n正文");
        assert!(fields.is_empty());
        assert_eq!(body, "没有 frontmatter\n正文");
        // 未闭合：整篇当 body
        let (fields, body) = parse_frontmatter("---\nname: x\n");
        assert!(fields.is_empty());
        assert_eq!(body, "---\nname: x\n");
    }

    #[test]
    fn required_field_rejects_missing() {
        let mut fields = BTreeMap::new();
        fields.insert("name".to_string(), "  ".to_string());
        let err =
            required_agent_field(&fields, "description", Path::new("a.md")).unwrap_err();
        assert!(err.contains("description"));
    }

    #[test]
    fn request_mode_selects_exactly_one_shape() {
        // 单任务
        let r: SubagentRequest = serde_json::from_value(json!({"agent": "researcher", "task": "调研"}))
            .unwrap();
        assert!(matches!(r.mode().unwrap(), RequestMode::Single(_)));
        // 并行
        let r: SubagentRequest = serde_json::from_value(json!({
            "tasks": [{"agent": "a", "task": "1"}, {"agent": "b", "task": "2"}]
        }))
        .unwrap();
        assert!(matches!(r.mode().unwrap(), RequestMode::Parallel(t) if t.len() == 2));
        // 链
        let r: SubagentRequest =
            serde_json::from_value(json!({"chain": [{"agent": "a", "task": "1"}]})).unwrap();
        assert!(matches!(r.mode().unwrap(), RequestMode::Chain(t) if t.len() == 1));
        // 冲突：两种形态同给 → 拒绝
        let r: SubagentRequest = serde_json::from_value(json!({
            "agent": "a", "task": "1", "tasks": []
        }))
        .unwrap();
        assert!(r.mode().is_err());
        // 只有 agent 没有 task → 拒绝
        let r: SubagentRequest = serde_json::from_value(json!({"agent": "a"})).unwrap();
        assert!(r.mode().is_err());
    }

    #[test]
    fn child_args_shape_matches_child_parser() {
        let args = child_args(&agent(), "调研 X");
        let args: Vec<String> = args.into_iter().map(|a| a.into_string().unwrap()).collect();
        assert_eq!(
            args,
            vec![
                "--mode", "json", "--print", "--no-session",
                "--tools", "read,bash",
                "--model", "deepseek-chat",
                "--thinking", "low",
                "--append-system-prompt", "你是调研员。",
                "Task: 调研 X",
            ]
        );
        // 缺省 tools → DEFAULT_CHILD_TOOLS；空 system prompt 不带 append 旗标
        let bare = AgentDefinition {
            tools: None,
            system_prompt: "  ".into(),
            ..agent()
        };
        let args = child_args(&bare, "t");
        let joined: Vec<String> = args.into_iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(joined.contains(&DEFAULT_CHILD_TOOLS.to_string()));
        assert!(!joined.iter().any(|a| a == "--append-system-prompt"));
    }

    #[test]
    fn ingest_accumulates_protocol_events() {
        let mut r = SubagentResult::starting(&agent(), None);
        // 增量
        ingest_child_event(
            r#"{"type":"message_update","assistantMessageEvent":{"delta":"你"}}"#,
            &mut r,
        );
        ingest_child_event(
            r#"{"type":"message_update","assistantMessageEvent":{"delta":"好"}}"#,
            &mut r,
        );
        assert_eq!(r.output, "你好");
        // message_end 权威内容：已有增量时不覆盖
        ingest_child_event(
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"完整版"}]}}"#,
            &mut r,
        );
        assert_eq!(r.output, "你好");
        // agent_end messages 兜底：空输出时回读
        let mut empty = SubagentResult::starting(&agent(), None);
        ingest_child_event(
            r#"{"type":"agent_end","messages":[{"role":"assistant","content":[{"type":"text","text":"兜底结果"}]}]}"#,
            &mut empty,
        );
        assert_eq!(empty.output, "兜底结果\n");
        // 非 JSON 行 → stderr
        let mut r2 = SubagentResult::starting(&agent(), None);
        ingest_child_event("不是 JSON", &mut r2);
        assert_eq!(r2.stderr, "不是 JSON\n");
    }

    #[test]
    fn structured_block_truncates_fields() {
        let mut r = SubagentResult::starting(&agent(), None);
        r.output = "x".repeat(FIELD_LIMIT + 100);
        r.status = SubagentStatus::Completed;
        r.exit_code = Some(0);
        let block = structured_result_block(&[r]);
        assert!(block.contains("\"status\":\"completed\""));
        assert!(block.contains("…"), "超限字段截断");
        assert!(block.len() <= BLOCK_LIMIT + 1, "整块上限");
    }

    #[test]
    fn discovery_loads_user_and_project_agents() {
        let base = std::env::temp_dir().join(format!("bm-subagent-test-{}", std::process::id()));
        let user_dir = base.join("user").join("agents");
        let project = base.join("proj");
        let project_dir = project.join(".pi").join("agents");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            user_dir.join("default.md"),
            "---\nname: default\ndescription: 通用\ntools: read\n---\nU",
        )
        .unwrap();
        std::fs::write(
            project_dir.join("default.md"),
            "---\nname: default\ndescription: 项目通用\ntools: bash\n---\nP",
        )
        .unwrap();
        std::fs::write(
            project_dir.join("reviewer.md"),
            "---\nname: reviewer\ndescription: 评审\n---\nR",
        )
        .unwrap();

        // both：项目定义同名覆盖用户定义
        let agents =
            discover_agents(&base.join("user"), &project, AgentScope::Both).unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents["default"].description, "项目通用");
        // user：只看用户目录
        let agents =
            discover_agents(&base.join("user"), &project, AgentScope::User).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents["default"].description, "通用");
        // project：只看项目目录
        let agents =
            discover_agents(&base.join("user"), &project, AgentScope::Project).unwrap();
        assert_eq!(agents.len(), 2);
        assert!(agents.contains_key("reviewer"));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn nearest_project_dir_walks_upward() {
        let base = std::env::temp_dir().join(format!("bm-subagent-up-{}", std::process::id()));
        let project = base.join("a").join("b");
        let agents_dir = project.join(".pi").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        assert_eq!(nearest_project_agents_dir(&project), Some(agents_dir.clone()));
        assert_eq!(nearest_project_agents_dir(&project.join("c")), Some(agents_dir));
        assert_eq!(nearest_project_agents_dir(Path::new("Z:/不存在的路径/深处")), None);
        std::fs::remove_dir_all(&base).ok();
    }
}
