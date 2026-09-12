---
status: accepted
date: 2026-09-11
summary: manifest.execution_mode 落声明(缺省回退旧约定)+删前缀猜法+同步无 deadline 为显式非目标+CallContext 入端口记为方向不实施(2026-09-11)
supersedes: []
superseded_by: []
---

# ADR-0036: 能力执行分道以合同声明为唯一真源

- 状态: Accepted（2026-09-11，issue #39 与 issue #59 残余-1 的裁决与分期）
- 日期: 2026-09-11
- 关联: ADR-0016（Skill wasm 沙箱）、ADR-0020（内置能力冻结）、ADR-0031（Agent v0.2 通信面）、ADR-0032（注册期冻结校验）、ADR-0033（skill.* 异步分道归位）、ADR-0034（插件协议 SDK）、issue #39、issue #59

## 背景

「同步」与「异步」执行分道长期由**命名约定**判定，散在三处各自为政（ADR-0033 已把启动/热注册两处收口到 `provider_is_async`，但约定本身仍是字符串前缀）：

- `registry.rs::provider_is_async`：`mcp.*` / `skill.*` / `*.async`（按 provider 名）；
- `system_exec.rs::SplitExecutor`：按 capability 名 `system.exec`/`fs.`/`skill.` 前缀选子执行器；
- `skill_wasm.rs`：`capability.starts_with("skill.")` 守卫。

后果（有实据，非理论）：ADR-0033 坐实热装载分支曾漏判 `.async` 后缀，能力落同步占位 provider 报错；分道判据与 provider 命名强耦合，新增执行族必须同时记得改多处前缀表。同时 #59 残余-1 记录：两个执行端口均不携带 `CallContext`，带身份的能力只能内核内联（`task.share.*` 需 principal 派生 task_id，故 ADR-0031 内联；`memory.*` 只能把 scope 当形态校验）。

## 决策

1. **manifest 增可选字段 `execution_mode`（`sync` | `async`，Minor，只增不破）**。执行分道的唯一真源 = 合同声明，不再由名字猜。
2. **注册期读声明，缺省回退旧约定**。`CapabilityRegistry::mark_async_for`（及注册门禁）优先读 `manifest.execution_mode`；未声明条目回退既有 provider 命名约定（`mcp.*`/`skill.*`/`*.async`），保证旧 manifest 与测试夹具零改动。
3. **生产 manifest 一律显式声明 `execution_mode`**。异步族：MCP 工具、skill wasm、`system.exec`、`fs.*`；同步族：`context.compress`、内置演示能力。声明缺失只在不影响行为时容忍（测试/fixture）。
4. **同步路径无 deadline 为显式非目标（ratify，不再作为缺陷跟踪）**。同步端口按构造只承载进程内快能力（现有同步 manifest `timeout_ms` 实测 1–5s），`manifest.timeout_ms` 仅约束异步端口（超时/取消由异步执行器钳制）；不以线程化超时改造 Broker 同步步，避免在单写者回路上引入等待。
5. **`CallContext` 入执行端口 = 记录方向，本期不实施**。当前无生产能力需要端口内身份：`memory.*` 已按 #59-4 迁出内核（生产组合根零调用），`task.share.*` 需 `&mut World` 故仍内核内联（ADR-0031）。方向保留，待出现**真实需要身份的执行族**时另立 ADR 实施（届时两端口 trait + 全 providers 适配，属 Major）。

## 后果

- 分道判定单源：`execution_mode` → `registry.is_async()`；新增执行族只需在 manifest 声明，不必登记前缀表。`SplitExecutor` 仍在异步族内做子路由（它只被异步能力触达），但 async-vs-sync 分类不再依赖字符串。
- 合同 `manifest.v0_1` 增可选枚举字段；消费方必须忽略不认识字段（既有 `additionalProperties: true` 语义），无破坏。
- 守护测试：声明 `execution_mode: async` 的能力必进异步分道、声明 `sync` 必不进（含热注册分支）；生产 manifest 全部带声明（防回退到约定）。
- #39/#59 残余-1 的 CallContext 与同步 deadline 两项以本条为裁决依据结项；实现项为 `execution_mode` 分道单源化。
- `decisions.md` 评估增/替条：执行分道以合同声明为准（与既有第 11 条「执行分道须配单源判定」同源，合并表述）。
