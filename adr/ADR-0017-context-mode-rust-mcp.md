# ADR-0017: context-mode Rust MCP 官方插件

- 状态: Accepted（用户 2026-09-03 授权实施）
- 日期: 2026-09-03
- 关联: ADR-0005（万物皆插件）、ADR-0006（权限以合同显式化）、ADR-0011（MCP Server 接入）、ADR-0016（脚本执行面）

## 背景

上游 `mksglu/context-mode` 将上下文裁剪、全文检索、会话连续性和代码执行暴露为 MCP 工具，并同时附带多个客户端专用 hooks/plugins。BoenMind 需要可发布、可审计、无需 Node/Python 的 Linux x86_64 单文件实现，但不能把客户端 hooks 或任意执行通道变成 Runtime 内核特权。

## 决策

1. **归属**：context-mode Rust 版是普通外部 MCP Provider，源码置于 `plugins/mcp/context-mode/`，不加入 `runtime` workspace，不修改 C4 或既有 MCP 合同。
2. **发布**：随 Linux x86_64 发布包提供一个 `plugins/context-mode` 单一可执行文件；它是官方可选插件，默认不写入 `mcp.json`、不启动，仍须设置页扫描→用户批准→reload。
3. **协议**：实现 MCP `2024-11-05` 的 newline-delimited JSON-RPC stdio，以及 BoenMind 插件扫描所需的 `--self-describe` 单行声明。能力由既有 MCP Hub 注册为 `mcp:context_mode.<tool>`，调用继续经过 Broker、审批、超时、取消和审计。
4. **首版范围**：迁移确定性上下文存储/恢复、SQLite FTS5/BM25 索引检索、受限宿主命令执行和批量执行。上游客户端 hooks、skills、网络抓取和客户端路由注入不迁移。
5. **安全边界**：索引只能访问配置的允许根目录；执行工具使用结构化程序与参数而非隐式 shell 拼接，清空继承环境、限制 cwd/超时/输出、回收子进程；索引/会话结果按不可信外部内容处理。执行、写入和批量工具以 MCP annotations 声明副作用，不能绕过 Broker。
6. **与 ADR-0016 的边界**：context-mode 的宿主命令执行是兼容性工具面，不等价于 Skill v0.2 的 WASM 脚本沙箱；它不接管 Skill 脚本，也不获得 WASI 权限。后续若要把任意脚本纳入 BoenMind，仍必须走 ADR-0016 的 wasmtime/Broker 七步管线。
7. **许可证**：本实现为 Rust 独立重写，不复制上游实现代码；仓库发布保留上游项目链接和 `upstream-notices/` 中的 Elastic License 2.0 说明。若未来再分发上游代码或完整客户端适配器，必须另行进行许可证审查。

## 后果

- 发布包继续保持核心服务器无需 Node/Python；执行工具若调用 Python/Node，仅依赖用户自行安装的宿主命令，并在缺失时返回 `runtime_unavailable`。
- 插件拥有独立 SQLite 文件，不读写 BoenMind 的 `state.db`、事件日志、密钥和配置；升级/移除插件不会改变 Runtime 状态模型。
- 首版不承诺上游全部客户端集成，也不承诺任意语言安全沙箱；这些遗留登记在 BACKLOG，不伪装为已迁移。
