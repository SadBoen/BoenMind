# 计划：MCP 官方插件（bm-mcp）

日期：2026-08-16 ｜ 状态：**阶段 0-3 + 前端设置页 + 阶段 4b 反向 MCP server 全部完成**（阶段 4c OAuth 2.1 后置待续）

一句话：BoenMind 内置一个默认官方 MCP 插件，作为 MCP client 接入任意外部 MCP server（stdio / Streamable HTTP），工具进入模型工具面；**协议目标 2026-07-28（MCP 2.0）+ dual-era 兼容存量 server；兼容性第一**——配置格式、工具命名、环境变量展开与主流生态一致，并自动读取主流 agent 的 MCP 配置。

---

## 一、调研结论摘要（四路独立调研，2026-08-16）

### 1. MCP 协议现状（"2.0"= 2026-07-28 修订版）

- 协议版本是**日期制**，官方无 "1.0/2.0" 编号。最新正式修订 **2026-07-28**（社区俗称 "MCP 2.0"，RC 2026-05-29，2026-07-28 转正 stable）；上一版 2025-11-25；**2026 年内无更新的正式修订**。
- 2.0 = **"无状态核心"**：移除 `initialize` 握手与 `Mcp-Session-Id`，每个请求在 `_meta` 自带协议版本与能力；新增强制 RPC `server/discover`；**MRTR** 取代 server 主动回调（server 返回 `input_required`，client 带状态重试）；`subscriptions/listen` 统一变更通知；扩展框架（Tasks / MCP Apps / Skills 独立版本化）；响应缓存 `ttlMs`；错误码规范分配（`-32022 UnsupportedProtocolVersionError`）。
- **已弃用**（12 个月窗口，最早 2027-07-28 移除）：Roots、Sampling、Logging、HTTP+SSE 传输、RFC 7591 动态注册。**新实现不应做这些**。
- **兼容性铁律**：Legacy client ↔ Modern server = 失败，无握手协商；官方推荐 **dual-era**（同一进程同时服务 2025 时代与 2026-07-28 时代），官方 TS/Python SDK v2 均默认如此。
- **Rust SDK**：rmcp（官方 modelcontextprotocol/rust-sdk）v3.1.2（2026-08-07）已支持无状态规范，月下载约 350 万，MIT，`client/server/macros/auth` features，stdio + Streamable HTTP。
- **认证**：本地 stdio / localhost 默认零 token（规范明确 stdio SHOULD NOT 走 OAuth）；只有远程受保护 HTTP server 需要 OAuth 2.1（PKCE、RFC 8707 resource 指示符、AS 发现、`iss` 校验）。
- 生态现状：Claude Code 已上线 2026-07-28；Codex CLI 以 `mcp_2026_07_28` opt-in（**默认仍 legacy**）；Cursor/Windsurf/Zed/OpenCode/Gemini CLI/Cline/Roo/Kilo/Warp 均查不到 2.0 公开声明 → **存量 server 绝大多数仍是 legacy，dual-era 是兼容性必答题**。

### 2. dsh（deepseek-ai/deepseek-harness）的 MCP 适配——官方内置插件模式

- 官方一等公民插件包 **`@deepseek-ai/dsh-mcp-client`**（仓库 `packages/mcp/mcp-client/`），依赖官方 `@modelcontextprotocol/sdk` ^1.12（非自研协议栈）；每 server 一个插件实例。
- **工具直注册**：模型侧公开名 `mcp__<serverName>__<rawName>`（与 Claude Code 同构），64 字符契约 + 12 位十六进制确定性哈希防撞（名字稳定、顺序无关）；参数 schema 直接透传；**只桥 tools，不做 resources/prompts**。
- 生命周期：自动重连（指数退避 500ms→30s、每故障 10 次上限、崩溃循环熔断）；`tools/list_changed` 触发全量重同步（失败保留上一代、冲突整代回滚）；工具超时默认 60s（可配）；env 凭据清洗。
- 权限：MCP 工具与本地工具走同一条注册表与管线（allow/deny/ask + 审批服务）。
- 传输：stdio + streamable-http 两种，无独立 SSE。

### 3. pi 生态的 MCP 适配——生态扩展插件模式（对比参照）

- pi 核心**故意不做 MCP**（官方文档明言 "intentionally does not include built-in MCP"）；pi_agent_rust（Dicklesworthstone）无任何 MCP crate。**注：pi_agent_rust 已于 2026-08-15 从我们仓库删除**，引擎由 bm-loop 接替。
- 社区事实标准 **`nicobailon/pi-mcp-adapter`**（1.2k star）：**单代理工具 + 懒连接**——不把全部 MCP 工具注册成原生工具（省 10k+ token），只暴露一个 `mcp({search})` 代理工具（约 200 token），按需发现/启动 server。
- **兼容性杀手锏（必吸收）**：自动读取 `.mcp.json`、`~/.config/mcp/mcp.json`、`~/.agents/mcp.json` 等标准配置文件；`/mcp setup` 可**导入 Cursor / Claude Code / Codex 的主机配置**；协议版本协商 + 401/404/405/406/415 兼容降级。
- 传输：stdio + Streamable HTTP + 传统 SSE + 自定义 UnixSocket。

### 4. 主流 coding agent 的兼容性惯例（"兼容性"的落点）

- **配置格式收敛**：绝大多数用 `mcpServers` JSON 对象（Claude Code / Cursor / Windsurf / Gemini / Warp / Kilo / Roo / Cline），项目级 + 用户/全局级双文件；Claude Code 三作用域（local/project/user）不合并字段整条覆盖。
- **工具命名事实标准**：`mcp__<server>__<tool>`（Claude Code 系全员沿用）。
- **传输**：stdio 全支持；远程主流 Streamable HTTP；SSE 普遍标记 deprecated。
- **环境变量展开**：Claude Code `${VAR}` / `${VAR:-default}` 语法（支持 url/headers/command/args/env 字段）；Cursor `${env:NAME}`、`${workspaceFolder}`。
- 值得吸收：Cline 的 MCP Marketplace（一键装 server）；Warp 跨 agent 配置自动采纳（Claude Code / Codex / `~/.agents/.mcp.json`）；Claude Code 的 `mcp serve` 反向暴露、每服务器 timeout/alwaysLoad/输出上限控制。

### 5. 我们本地现状（Explore 实读代码）

- **MCP 注册面已埋好、连接面完全缺失**：
  - `pi.registerMcpServer(name, spec)` 已实现（`backend/crates/bm-compat/src/extensions_js.rs:20307-20375`），登记进 `__pi_mcp_server_index` 与 `ext.mcpServers`——TS 插件可以声明 MCP server；
  - 但 `@modelcontextprotocol/sdk` 模块是**空壳 stub**（`extensions_js.rs:9984-10045`）：`Client.connect/listTools/callTool` 全部返回空，无真实连接、无工具合并；
  - manifest 快照输出 `mcp_servers` 数组（`:20798`）但**无任何消费方**。
- 架构文档已把 "MCP client 插件" 定位为协议层一等公民（`docs/everything-is-plugin-architecture.md` Z4），与现架构无冲突。
- 现成可复用：
  - 工具注入点：`build_loop_agent`（`bm-server/src/bm_engine.rs:320-364`）顺序注册内置工具/subagent/todo/插件工具；
  - 统一分派口：`QuickJsToolExecutor::execute`（`compat_engine.rs:1016`）是全部工具的分派口，MCP 工具加一条分支即可；
  - 权限三件套：`PermissionStore`（`~/.boenmind/extension-permissions.json` 决策记忆）+ `ask_capability` 询问链（60s fail-closed）+ `BuiltinGate`（`builtin_gate.rs`，permissive 直放）；
  - 出厂插件机制：`BUILTIN_PLUGINS`（`bm-core/src/plugins.rs:20-45`）+ manifest schema 驱动设置页；
  - 线程模式样板：`CompatEngine` 专用线程 + 命令通道（`compat_engine.rs:571-838`）；
  - 测试模式：InMemoryStore + tokio 集成测试（`backend/tests/event_log`），长程验收走 artifacts/ 脚本。

---

## 二、目标与非目标

### 目标（本期）
1. **默认官方 MCP 插件**：随 BoenMind 出厂（BUILTIN_PLUGINS 增加 `mcp`），可接入任意 MCP server，工具进入模型工具面并全链路可审计（tool_call/tool_result 事件日志与本地工具一致）。
2. **协议目标 2026-07-28 + dual-era**：2.0 server 与存量 2025 legacy server 都能连。
3. **兼容性第一**：`mcpServers` 配置格式、`mcp__server__tool` 命名、`${VAR}` 展开与主流一致；自动读取标准配置文件（`.mcp.json` / `~/.agents/mcp.json` / `~/.config/mcp/mcp.json`）；支持导入 Claude Code 等主机配置。
4. 权限治理：MCP 工具纳入询问链 + 决策记忆 + 档位（safe/balanced 询问、permissive 直放）。
5. TS 注册面接通：pi 生态插件 `pi.registerMcpServer` 声明的 server 进入同一连接池（兼容既有生态）。

### 非目标（本期明确不做）
- 不做已弃用特性：Roots / Sampling / Logging / HTTP+SSE 传输 / RFC 7591。
- 不做反向 MCP server（把 BoenMind 暴露成 MCP server 供他客户端驱动）——远期。
- 不做 MCP Apps / Skills-over-MCP / Tasks 扩展跟随——远期。
- 不做 Cline 式 MCP Marketplace 商店——远期（注册表目录集成另排期）。

---

## 三、架构设计

```
                        ┌─────────────────────────────────────────────┐
                        │  bm-server 组装层                            │
 模型 ←→ ReactLoopAgent │                                             │
 (bm-loop)   ToolRegistry│  build_loop_agent 注册段:                   │
              ▲         │   内置工具 / subagent / todo / 插件工具      │
              │         │   +── mcp__ 工具（bm-mcp 提供）              │
              │         │                                             │
       QuickJsToolExecutor（统一分派口）                                │
              │         │                                             │
              ├─ mcp__ 前缀分支 ─► GatePort/询问链 ─► McpClientManager │
              │         │              (bm-mcp crate)                 │
              └─ 其余     │              │ stdio spawn / HTTP          │
                 (现状)   │              ▼                            │
                          │       ┌──────────────┐  ┌──────────────┐  │
                          │       │ MCP server A │  │ MCP server B │  │
                          │       │ (stdio)      │  │ (HTTP/2.0)   │  │
                          │       └──────────────┘  └──────────────┘  │
                          └─────────────────────────────────────────────┘
```

要点：
1. **bm-mcp crate（新）**：MCP client 核心——server 注册表、连接管理、stdio 子进程 spawn、Streamable HTTP client、协议版本协商（dual-era）、工具枚举（`tools/list` → `ToolDef`）、工具调用（`tools/call`）、重连与 `tools/list_changed` 重同步。
2. **插件形态**：以 Rust 插件（`Plugin` trait，仿 bm-compactor）+ 服务面注册（`McpPort`）接入内核；出厂清单 `BUILTIN_PLUGINS` 增加条目，前端插件页可见、manifest schema 驱动设置页（server 增删改查 / 连接状态 / 测试按钮，参照 web-search 设置页模式）。
3. **工具注入**：`build_loop_agent` 新增注册段，从 McpPort 取 `mcp__<server>__<tool>` 工具集；执行走 `QuickJsToolExecutor` 新增 `mcp__` 前缀分支 → 权限门 → `McpClientManager::call_tool`；tool_call/tool_result 事件自然落事件日志。
4. **权限**：MCP 工具按 server 维度首用询问（`PermissionStore` 按 `(mcp, server, tool)` 记忆），档位语义与现有 extension_policy 一致；stdio server 是任意可执行程序，spawn 本身也是一次权限事件（首连询问）。
5. **配置**：主配置在插件 settings.json（manifest 驱动 UI）；**自动发现**项目 `.mcp.json`、`~/.agents/mcp.json`、`~/.config/mcp/mcp.json`（格式均为标准 `mcpServers`）；支持 `${VAR}` / `${VAR:-default}` 展开；他生态配置（`~/.claude.json` 等）做**一次性导入**而非动态依赖。
6. **TS 注册面**：保留 `pi.registerMcpServer`，把 JS 声明的 server 同步进 McpClientManager（经既有快照 `__pi_snapshot_extensions` 消费或新增命令通道消息）；stub 不动（pi 生态插件若直接 `import` SDK 的空 client 本身就没法工作，属上游行为，不影响）。

---

## 四、兼容性策略（重点，吸收来源标注）

| 维度 | 做法 | 吸收自 |
|---|---|---|
| 协议版本 | dual-era：2.0 无状态 + legacy `initialize` 握手双路径；`-32022` 支持版本列表重试 | MCP 官方规范（TS/Python SDK v2 默认行为） |
| 工具命名 | `mcp__<server>__<tool>`，64 字符契约 + 确定性哈希防撞 | dsh / Claude Code 系事实标准 |
| 配置格式 | 标准 `mcpServers` JSON（stdio: command/args/env；http: url/headers） | Claude Code `.mcp.json` 格式为基准 |
| 配置作用域 | 用户级 + 项目级（项目优先） | Cursor / Kilo / Roo / Gemini |
| 自动发现 | `.mcp.json`（项目）、`~/.agents/mcp.json`、`~/.config/mcp/mcp.json` | pi-mcp-adapter / Warp |
| 配置导入 | 一次性导入 Claude Code / Cursor / Codex 主机配置，转成自有配置后自主管理 | pi-mcp-adapter `/mcp setup` |
| env 展开 | `${VAR}` / `${VAR:-default}`（url/headers/command/args/env） | Claude Code |
| 传输 | stdio + Streamable HTTP；SSE 不做（已弃用，存量需求再评估） | 主流一致 |
| 懒连接 | 默认直注册（与主流一致）；大 server 集合场景提供代理工具模式（可配置开关） | dsh 直注册 + pi 代理工具双参照 |
| 降级 | 401/404/405/406/415 兼容降级与诊断信息 | pi-mcp-adapter |

---

## 五、分阶段实施

### 阶段 0：技术验证 spike（先行，输出依赖决策）
- 用 rmcp v3 实连：stdio 官方示例 server、Streamable HTTP、2026-07-28 协商、**legacy 2025 server 兼容性实测**（rmcp 对 dual-era 的支持程度是自研量的决定性变量）。
- 输出：**拍板点 1** 的实证依据（用 rmcp vs 自研 JSON-RPC 层）。

### 阶段 1：协议底座
- 按阶段 0 决策：`bm-mcp` crate——JSON-RPC 编解码、stdio spawn / streamable HTTP client、协议版本协商（dual-era）、`server/discover`、工具枚举与调用、结果类型处理（`structuredContent` 校验、超限截断）。

### 阶段 2：插件骨架（可演示端到端）
- Rust 插件注册（Plugin trait + `McpPort` 服务面）+ `BUILTIN_PLUGINS` 出厂条目 + 前端插件页/设置页。
- 工具注入 `build_loop_agent` + `QuickJsToolExecutor` mcp 分支 + 权限三件套挂接。
- 配置读取（settings.json 主配置）+ 首连询问 + 端到端验证（真实 filesystem / memory server）。

### 阶段 3：兼容性与生命周期（"兼容性"主体）
- 自动发现 `.mcp.json` / `~/.agents/mcp.json` / `~/.config/mcp/mcp.json`；`${VAR}` 展开；他生态配置导入工具。
- 生命周期：超时（默认 60s）、崩溃重启（指数退避 500ms→30s、10 次熔断）、`tools/list_changed` 重同步（失败保留上一代）、env 凭据清洗、SSRF 防护（复用 bm-core url 校验）。
- TS 注册面接通（`pi.registerMcpServer` 声明 → 连接池）。
- 集成测试套件（mock MCP server fixture，仿 event_log 模式）+ 兼容性矩阵测试。

### 阶段 4：2.0 强化
- `server/discover` 预探测、MRTR `input_required` → 询问链联动、响应缓存（ttlMs）、`subscriptions/listen`。
- 远程受保护 HTTP server 的 OAuth 2.1 client（PKCE、RFC 8707、AS 发现、`iss` 校验）——本地场景后置。
- 代理工具模式（pi 式懒连接）可配置项。

### 阶段 5：远期（另行排期，不进本期）
- SSE legacy 传输（按存量需求）、prompts → 斜杠命令（`/mcp__server__prompt`）、反向 MCP server（`mcp serve` 模式）、MCP Marketplace 目录集成（Glama / mcp.so）、MCP Apps / Skills-over-MCP 扩展跟随。

---

## 六、拍板点（7 项，2026-08-16 全部拍定）

1. **协议底座**：**拍定 = 官方 rmcp v3.1.2，裁剪 features**（`default-features = false`，只开 `client` + `reqwest`(rustls) + `transport-child-process` + `transport-streamable-http-client-reqwest`）。release 产物只含用到的符号（lto=thin 下精确到函数）。
2. **插件形态**：**拍定 = Rust 插件**（`McpClientManager` + `McpService` 服务面经 kernel port "mcp" 注册，bm-compactor 模式）。
3. **工具注册**：**拍定 = 直注册** `mcp__<server>__<tool>`（dsh/Claude Code 同款命名 + 64 字符契约 + 12 位哈希防撞）。
4. **配置兼容范围**：**拍定 = config.toml `mcp` 数组（JSON 形态）为主**；自动发现 `.mcp.json` / `~/.agents/mcp.json` 等留阶段 3。
5. **协议版本**：**拍定 = dual-era 一步到位**（`ClientLifecycleMode::Auto`：首选 2026-07-28，legacy 回退 2025-11-25）。
6. **TS stub 处理**：**拍定 = 保留注册面**，TS 声明（`pi.registerMcpServer`）接通真连接留阶段 3。
7. **OAuth 2.1 远程**：**拍定 = 后置**（本地 stdio/localhost 零 token，远程受保护 server 留阶段 4）。

### 实施记录（2026-08-16）

- **阶段 0 spike**：rmcp 3.1.2 裁剪 features 编译通过；双场景实测——legacy（官方 filesystem server，TS SDK 1.29）协商 2025-11-25 回退成功；modern（TS SDK v2 自写 server）协商 2026-07-28 首选成功；stdio spawn / tools/list / tools/call / structuredContent 提取 / 断开全通。
- **阶段 1**：`bm-mcp` crate（backend/crates/bm-mcp）：`McpServerConfig`（配置形状对齐 Claude Code mcpServers）、`McpClientManager`（连接/枚举/调用/断开 + 工具快照缓存）、`McpService` 服务面 trait（tools/servers/call_tool，std 锁同步读）、工具名规范化 + 哈希防撞。6 单测全绿。
- **阶段 2**：组装层接线（bm-core config.rs `mcp` 字段 → bm-server lib.rs 启动连接 → kernel port "mcp" 注册 → build_loop_agent 工具注册段 → QuickJsToolExecutor `mcp__` 分支 → McpGate 权限门（决策记忆 + 询问链，permissive 直放，与 BuiltinGate 同源））。**端到端验收通过**（隔离 BOENMIND_HOME + 双 server）：modern 协商 2026-07-28、legacy 协商 2025-11-25 并存；真实会话模型调 `mcp__modern__hello` 成功返回 "hello BoenMind"。全量测试 219 全绿 + clippy 新增代码零警告。
- **阶段 3（eaf0227 + a26c910）**：兼容性与生命周期——
  - `discover.rs`：项目 `.mcp.json`（Claude Code 格式）+ `~/.agents/mcp.json` + `~/.config/mcp/mcp.json` 自动发现（pi-mcp-adapter/Warp 同款）；标准 mcpServers 条目解析（sse/ws 弃用跳过）；`${VAR}`/`${VAR:-default}` 展开；HTTP url SSRF 校验（语义对齐提供商端点）。
  - `client.rs`：崩溃重连 supervisor——`is_transport_closed` 检测（**is_closed 仅显式关闭才置位，被动断开必须用传输状态**——踩坑）、指数退避 500ms→30s、连续 10 次熔断、重连后工具快照刷新。
  - TS 注册面接通：`pi.registerMcpServer` 声明进连接池（第三来源：config.toml > 自动发现 > TS 声明）。
  - 集成测试：`tests/fixtures/echo_server.mjs` 纯 Node JSON-RPC fixture（零 npm 依赖，含 crash 工具）；legacy 协商/枚举/调用/崩溃重连全链路；node 缺失自动跳过（CI 兜底）。
  - 验收：三来源五 server 并存（2.0/legacy 混合）；TS 声明 server 真实会话调用成功。
  - 注：bm-compat 4 个 doom/ext_conformance 测试失败为既有环境缺失（artifacts 未下载），与改动无关。
- **前端设置页（81d549e）**：`routes/mcp.rs` 管理 API（GET servers 状态 + POST connect/disconnect，连接持久化 config.toml `mcp` 数组重启生效）；`McpSettings.tsx` 设置页（5s 轮询、协议版本/工具数展示、断开、添加表单 stdio/http）；SETTINGS 注册表 + i18n 四语言；vite 代理 BM_API_TARGET 可覆盖。**浏览器实测通过**：列表渲染 6 server 状态正确，CUA 填表添加 ui-test → 出现（2026-07-28/2 工具）→ config.toml 持久化。坑：McpServerConfig Option 字段须 skip_serializing_if（TOML 不支持 null）；运行中进程锁 exe 导致 build 静默失败（反复跑旧二进制）——build 前先杀服务。
- **阶段 4b（反向 MCP server，`bm-server --mcp-serve`）**：把内置工具面（read/write/edit/grep/find/ls/bash——主引擎同款 BuiltinTools）暴露成 stdio MCP server，供 Claude Code / Claude Desktop / Cursor 等外部 client 经 mcpServers 配置接入。实现：`bm-mcp/src/serve.rs`（McpServeTool + Router + ToolRoute::new_dyn 注册，结果转 CallToolResult + structured_content）；`bm-server/src/mcp_serve.rs`（BuiltinTools 定义映射，executor 闭包内按 cwd 重建实例）；main.rs `--mcp-serve` 分支（tracing 不初始化，stdout 是协议通道）。**手动全链路验证**：discover（2026-07-28 直通、supportedVersions 全列表）→ tools/list（7 工具）→ tools/call read 返回真实文件内容。坑：
  - **RunningService 必须持有**——DropGuard drop 时取消服务任务；`serve_server().await` 返回值直接丢弃 → discover 响应后任务即取消，后续请求无人应答（症状：connect 成功但 tools 列表空、进程秒退）。修复 = `running.waiting().await`。
  - **wire 格式**：2.0 无状态请求的 `_meta` 在 `params` 内（`Request` 的 serde 实现从 `params._meta` 提取），key 是 `io.modelcontextprotocol/protocolVersion` 长格式；裸 key/顶层 `_meta` 会被 server 拒为 "expect initialized request"。
  - **结果形状**：宿主工具 text_output 包装 `{content:[{type:text,text}],details:null}` 若整个塞进 structured_content，client 端（structured 优先）返回包装对象而非文本——serve.rs 解包为纯文本，结构化结果（bash 的 {stdout,stderr,code,killed}）保留 structured_content。
  - 集成测试 `bm-server/tests/mcp_serve.rs`：spawn exe → bm-mcp client 接入 → 断言协议 2026-07-28、7 工具齐全、ls/read 真实调用通过。

---

## 七、验收标准

- **兼容性矩阵全绿**：stdio × streamable HTTP ×（2.0 server + legacy 2025 server）四组合实连通过。
- **端到端**：真实会话中模型调用 MCP 工具成功，工具卡片事件、事件日志、压缩插件行为与本地工具一致（长程测试同口径）。
- **权限**：mcp 工具走询问链 + 决策记忆 + permissive 直放；拒绝理由对模型可见。
- **配置兼容**：把 Claude Code 的 `.mcp.json` 原样放进项目即可用；`${VAR}` 展开生效；导入工具可用。
- **测试**：单测 + 集成套件（mock server fixture）+ 浏览器实测设置页；`cargo test` 全绿、clippy 清零（既有质量门）。
- **性能**：工具列表注入不拖慢会话启动；懒连接延迟到首用。

## 八、风险

- **rmcp v3 对 legacy server 兼容不足** → 阶段 0 spike 提前暴露，据此修正自研量（最坏情况：legacy 路径自研、2.0 路径用 rmcp）。
- **工具名/前缀冲突** → dsh 哈希防撞方案照搬（确定性、稳定、顺序无关）。
- **token 膨胀**：大 server 集合下 tools 数组膨胀 → 拍板点 3 的代理工具模式兜底。
- **安全**：stdio server 是任意可执行程序 → 首连询问 + env 凭据清洗 + SSRF 防护必须齐套。
- **pi 生态插件直接 import SDK stub** 的极端场景 → 属上游行为，文档声明即可。

---

## 九、待完成任务（剩余事项清单）

已完成：阶段 0-3（客户端底座/配置/兼容性/TS 注册面）+ 前端设置页 + 阶段 4a（MRTR 内建确认、SSE legacy 实测拒绝）+ 阶段 4b（反向 MCP server）。

- [ ] **阶段 4c：OAuth 2.1 远程认证**（拍定后置）——本地 stdio/localhost 零 token 场景用不到；当出现受保护的远程 HTTP server 需求时启动：rmcp `auth` feature（PKCE + RFC 8707 resource 指示符 + AS 发现 + `iss` 校验）+ token 存储 + 授权流程。**启动判据：首个需要认证的远程 server 接入需求**。
- [ ] 阶段 5 远期（另行排期）：SSE legacy 传输（按存量需求）、prompts → 斜杠命令（`/mcp__server__prompt`）、MCP Marketplace 目录集成（Glama / mcp.so）、MCP Apps / Skills-over-MCP 扩展跟随。

---

## 参考来源（要点）

- MCP 规范 2026-07-28 changelog / versioning / authorization：modelcontextprotocol.io/specification/2026-07-28/
- rmcp（官方 rust-sdk）：github.com/modelcontextprotocol/rust-sdk（v3.1.2）
- dsh mcp-client：github.com/deepseek-ai/deepseek-harness（packages/mcp/mcp-client/）
- pi-mcp-adapter：github.com/nicobailon/pi-mcp-adapter
- Claude Code MCP：code.claude.com/docs/en/mcp；"Bringing MCP 2026-07-28 to Claude"
- Codex CLI MCP 2.0 opt-in：github.com/openai/codex（mcp_2026_07_28）
- Cline / Zed / OpenCode / Gemini CLI / Warp MCP 文档（详见各节）
