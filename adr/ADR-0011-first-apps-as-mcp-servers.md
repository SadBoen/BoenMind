# ADR-0011:首批真实 App 以 MCP Server 形态接入

- 状态:Accepted(2026-08-30)
- 关联:ADR-0009(部署与 Surface)、M7 规格(MCP 接入与信任隔离)、
  基线 §18-M8(通过条件:两个真实 App 同套机制)

## 背景

M8 须交付首批真实 App(Wiki、确定性领域)并证明「两个真实 App 使用同一套
Runtime、Broker、Task 和日志机制」。候选形态:(a) 内核新增「App 对象」
与专用加载器;(b) App 以进程外 MCP server 接入,复用 M7 信任与隔离;
(c) App 编译进 Runtime 进程(内置 Rust 能力)。

## 决策

1. **首批 App 以 MCP stdio server 形态接入**(形态 b):python 标准库实现,
   经 `--mcp-config` 显式安装(M7.7 = 用户批准),工具即能力,全部调用
   过 Broker(审批/Grant/审计/收据/outbox 对账)与 Task/日志机制。
2. 不新增内核形态:无「App 对象」合同;App 的安装事实 = 配置文件 +
   动态注册的能力 manifest(provider=mcp.<app>)。
3. 真实副作用的锚定:Wiki 的写 = 磁盘文件变更(真实世界),收据 =
   写后内容摘要与字节数,outbox 对账沿用 M4/M7 语义。
4. 内置 Rust 演示能力集(system.*)保留为测试/演示面,不作为「真实 App」
   计入 M8 通过条件。

## 后果

- 正面:M7 的隔离(子进程崩溃不拖垮 Runtime)、信任(安装批准、首调
  审批)、收据(副作用对账)直接成为 App 运行时语义,零内核改动;
  App 可独立演进/重装(能力名命名空间 mcp.<app> 天然隔离)。
- 代价:App 分发 = MCP server 进程管理(启动/重生已由 M7 传输层承担);
  stdio 传输限本机,远程 App 待后续(与 ADR-0009 的本机/单 VPS 口径一致)。
- 回退:若未来需要更深的 App 集成(共享状态域、专用 Surface),发新
  ADR 增发「App 合同」,不影响本决策的运行语义。

## 条件与验收

1. 两个首批 App 在同一 Runtime 上共存并通过 e2e(M8-T1,t110-t113)。
2. App 的每个外部副作用在事件流与 outbox 中有收据与对账行(M8-T1)。
3. Market App 的确定性:同输入重放逐字节同结果(纳入 Judge 检查项)。
