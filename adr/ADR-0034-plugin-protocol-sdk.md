---
status: accepted
date: 2026-09-11
summary: Rust(plugins/mcp/sdk)+Python(apps/mcp_sdk.py)最小 SDK 单源化协议循环+未知工具统一 -32602/--self-describe 独立合同化(2026-09-11)
supersedes: []
superseded_by: []
---

# ADR-0034: 插件协议最小 SDK——双语言收口与未知工具口径统一 
- 状态: Accepted（） - 日期: - 关联: ADR-0005（万物皆插件）、ADR-0011（首批 App 以 MCP server 接入）、ADR-0017（官方 Rust 插件）、ADR-0022（工具协议与描述治理）、ADR-0023（随包插件发现与生命周期）、ADR-0033（#56 的 OPEN 登记处） 
## 背景 
MCP/JSON-RPC 协议循环在仓内被手写 N 遍，且无共享类型： 
- **Python**：`apps/wiki_server.py`、`market_server.py`、`music_server.py` 各写一份 readline + `json.loads` + 方法分发循环（约 48/52/82 行），`initialize` 结果形状、`-32700`/`-32601` 分支逐份重复。 - **Rust**：`plugins/mcp/context-inspector`、`web-multisearch` 两插件各有独立 Cargo 项目（不入 runtime workspace），各自手写 stdio 循环与 `--self-describe` 输出，循环骨架近乎逐行相同。 - **无语义类型**：全仓不存在 `JsonRpcRequest/Response/Error` 之类共享结构；`jsonrpc:"2.0"` 信封、`-32700/-32601/-32602` 魔法数字、`protocolVersion:" 
协议演进（MCP 版本升级、错误码增补）需 N 处同步改，漂移风险线性放大。此外坐实一处跨实现分歧：`tools/call` 未知工具，Python apps 回应用层 `result.isError=true`，Rust 插件回协议错误 `-32602`；两者。 
已由 commit `005a9ad` 落地的非决策子集（`plugins/smoke_test.py` 跨实现冒烟 + 两插件补 `-32700`）不在本 ADR 范围。 
## 决策 
1. **提供双语言最小插件 SDK，协议循环单源化。**  - Rust：`plugins/mcp/sdk`（包名 `boenmind-plugin-sdk`），集中 readline 循环、通知跳过、坏 JSON `-32700`、未知方法 `-32601`、未知工具 `-32602`，以及 `initialize`/`ping`/`tools/list`/`tools/call` 分发；两插件改以 path 依赖引入，只保留业务逻辑（工具实现、配置、扩展方法注册）。  - Python：`apps/mcp_sdk.py`，职责同构（`run_stdio` + `emit_self_describe`）；三 app 只保留 `TOOLS` 与工具实现。  - **SDK 不进 runtime workspace**：插件各自独立 Cargo 项目与 CI 矩阵的现状不变，SDK 以 path 依赖被编译，`target/release` 布局与发布打包路径零改动。 
2. **未知工具口径统一为协议错误 `-32602`（Invalid params）。** 依据 MCP ）。三个 Python app 的未知工具分支与 `apps/smoke_test.py` 的断言同步改为 `-32602`。 
3. **`--self-describe` 合同化，独立 schema。** 新增 `boenmind-contracts/mcp/mcp-self-describe.v0_1.schema.json`（Minor，只增），冻结发现声明的形状（`name/title/description/config_schema/suggested_entry`），并镜像进 `bm-contract`。与 `mcp-server.v0_1`（安装配置合同）分离：后者描述「用户如何安装插件」，本 schema 描述「插件自报是什么」。 
4. **SDK 边界：只收协议，不收业务与宿主。** SDK 不承载任何工具语义；宿主客户端（`bm-providers` 的 `McpTransport`/`McpHub`，含 HTTP/SSE 传输）本批不动，其采用同一 codec 另立后续 issue。测试夹具 `bm-testkit/fixtures/mini_mcp.py` 维持零依赖原始形态（其价值正在于不经 SDK 的独立实现）。 
## 后果 
- 新增插件只需实现工具表 + 调用处理，协议面由 SDK 统一；协议不变量（握手版本、错误码、通知语义、坏 JSON 应答）有单处真源与单测。 - `plugins/smoke_test.py` 与 `apps/smoke_test.py` 的未知工具断言归一为 `-32602`，跨实现一致性由两份冒烟 + SDK 单测共同守门。 - `--self-describe` 从约定俗成升为合同字段：管理面扫描消费的形状（`webadmin/mcp.rs`）有了机器可校验定义。 - 插件独立 `Cargo.lock` 随 path 依赖更新并提交；CI 新增 `plugin-sdk` job（fmt/clippy/test），两插件 job 经 path dep 覆盖 SDK 编译。 - 遗留：宿主客户端 `bm-providers/mcp.rs` 采用同一 codec（follow-up issue）；MCP 子进程无 OS 级沙箱（#55）维持 OPEN。 