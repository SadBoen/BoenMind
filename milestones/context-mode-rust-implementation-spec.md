# context-mode Rust MCP 实现规格

## 1. 目标与交付形态

新增独立 Cargo 项目 `plugins/mcp/context-mode`，release 产物为 Linux x86_64 单文件 `context-mode`。运行时通过 `--self-describe` 被现有 MCP 插件扫描器发现，用户批准后写入 `mcp.json`，reload 后上线。插件不进入 `runtime` workspace。

首版能力：

- `ctx_index`：在允许根目录内索引 UTF-8 文本文件；
- `ctx_search`：SQLite FTS5/BM25 检索并返回来源、片段和不可信标记；
- `ctx_session_append`：追加会话消息；
- `ctx_session_snapshot`：确定性保留最近消息并写快照；
- `ctx_session_restore`：恢复快照或完整事件；
- `ctx_execute`：结构化宿主程序执行；
- `ctx_execute_file`：执行允许范围内的文件；
- `ctx_batch_execute`：最多 16 项顺序执行，逐项返回状态。

不在首版：客户端 hooks/skills、网络抓取、模型摘要、跨客户端注入、WASM 沙箱、多语言 runtime 打包。

## 2. 数据目录与配置

`--config <path>` 指向插件配置 JSON；批准流程使用 `{config_file}` 替换为 `<data-dir>/config/mcp-context_mode.json`。配置：

| key | 类型 | 默认 | 约束 |
|---|---|---:|---|
| `data_dir` | string | 配置文件父目录下 `context-mode` | 必须是目录路径 |
| `allowed_roots` | string[] | `[data_dir]` | 所有索引/执行 cwd 必须落在其下 |
| `max_file_bytes` | integer | 1 MiB | 1 KiB..16 MiB |
| `max_files` | integer | 5000 | 1..5000 |
| `max_output_bytes` | integer | 256 KiB | 1 KiB..2 MiB |
| `default_timeout_ms` | integer | 30000 | 100..600000 |
| `execution_enabled` | boolean/string | false | 必须显式开启；仍受 Broker 审批，插件不是操作系统级沙箱 |
数据库为独立 `context-mode.sqlite3`，启动时幂等创建：

- `documents(path PRIMARY KEY, content, bytes, modified_ms, indexed_at)`；
- `documents_fts`：FTS5 `path UNINDEXED, content`；
- `sessions(id PRIMARY KEY, created_at, updated_at)`；
- `session_events(id INTEGER PRIMARY KEY, session_id, seq, role, content, created_at, UNIQUE(session_id, seq))`；
- `snapshots(session_id PRIMARY KEY, upto_seq, messages_json, created_at)`。

## 3. MCP 与风险

协议为 MCP `2024-11-05`、逐行 JSON-RPC 2.0。必须支持 `initialize`、`ping`、`notifications/initialized`、`tools/list`、`tools/call`；通知无响应；未知方法 `-32601`；未知工具/非法参数 `-32602`。`--self-describe` 输出一行 JSON，server name 固定 `context_mode`。

- `ctx_search`：`readOnlyHint=true`；
- `ctx_index`、session append/snapshot、所有 execute：`destructiveHint=true`，默认由 Broker 要求审批；
- 工具输出使用 `content[].text` + `structuredContent`，失败结果带 `isError=true` 或明确结构化错误。

插件内部不接触 BoenMind Secret Store；子进程环境使用白名单，stdout/stderr 有硬上限，超时 kill 并回收。外部结果不提升信任级别。

## 4. 测试矩阵

- 协议：initialize/ping/tools/list/通知/未知方法/未知工具/self-describe；
- 配置：默认值、范围钳制、坏文件回退；
- 索引：扩展名/路径边界/文件上限/幂等重建/FTS5 搜索/snippet；
- 会话：追加序列、快照裁剪、恢复保序；
- 执行：参数拒绝、cwd 越界、超时、输出截断、环境不泄漏、批量逐项失败；
- 黑盒：真实 release 二进制 stdio 握手，临时目录完成索引→搜索和会话恢复，不访问外网。

## 5. 验收门

1. 插件 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 和 release build 全绿；
2. `python boenmind-contracts/scripts/validate.py` 与 runtime 全量回归全绿；
3. 发布 tar 中存在可执行 `plugins/context-mode`，且 README/许可证 notices 同包；
4. 浏览器设置页可完成扫描、批准、reload、工具发现；
5. 不打 tag、不创建 GitHub Release；由用户另行明确发版。
