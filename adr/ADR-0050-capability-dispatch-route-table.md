---
status: accepted
date: 2026-09-12
summary: 异步能力分派由硬编码 if-else 改为声明式路由表——新 provider 族 = 追加路由,不改内核分派
supersedes: []
superseded_by: []
---

# ADR-0050: 能力执行面收口(声明式路由表取代内核 if-else)

- 关联: ADR-0041(去特化/按归属分道)、ADR-0042(按声明能力集分道而非前缀)、ADR-0036(execution_mode 为分道真源)、ADR-0005/ADR-0023(万物皆插件)、ADR-0020/0021(内置能力冻结清单)
- 背景(2026-09-12 架构评估): 「万物皆插件」在**裁决与注册面**已成立——`Broker::decide` 零 provider 种类分支、`CapabilityRegistry::register` 统一带冻结合同门。但**执行面**是硬编码分派:`SplitExecutor::call` 用一条 if-else 链按 `system.exec` 精确常量 → `FsExecutor::handles` → wasm 宿主编译表 → 回落 MCP 依次判定;`dispatch_capability` 另有 `task.share.` 前缀内联拦截。后果是**新增一个 provider 族必须修改内核分派代码**,而不是以插件身份接入,与基线 §2.3「新增功能优先做成插件,不改内核」及 §7 统一管线存在张力(见 GitHub issue #67)。

## 决策

**异步执行分派改为装配期声明的有序路由表,内核分派不再认识任何 provider 家族。**

1. 引入 `AsyncRoute { predicate, executor }`(`bm-providers::system_exec`):`predicate` 是**归属谓词**(按执行器声明的能力集/宿主编译表判断,不用名字前缀猜——沿用 ADR-0042/0041 口径),`executor` 是 `Arc<dyn AsyncCapabilityExecutor>`。
2. `SplitExecutor` 从「四字段 + if-else」改为「有序 `routes: Vec<AsyncRoute>` + `fallback`」;`call` 只做「首个谓词命中即分派,否则回落」。
3. 各族的归属关系由装配点 `SplitExecutor::new(exec, fs, skills, fallback)` 一次性声明:顺序即优先级(先具体后通用),新增族 = 在新装配处追加一条路由,`SplitExecutor::call` 本体不变。
4. **历史行为精确保持**:路由顺序与旧 if-else 逐一对应(exec/job_output → fs → wasm → fallback);`skills=None` 时该路由不注册,行为等价于旧的「wasm 执行面未启用」分支消失后落入 fallback(生产装配 `skills` 恒为 Some;无法判定的能力本就应交给 fallback)。

## 后果

- **扩展点归位**:新增 provider 族不再触碰内核分派逻辑;「注册即路由」的接缝从 if-else 变成路由表条目。
- **零行为变更**:全量测试套件通过(`skill_wasm_reload` 端到端、`SplitExecutor` 单元、workspace 全绿);`is_async` 分道与 `execution_mode` 合同真源不变。
- **未做(如实标注)**:①`dispatch_capability` 的 `task.share.` 前缀内联拦截保留——公告栏是**内核内联事件投影**(无外部副作用,不触 Provider 通道),其归属无法用「执行器谓词」表达,属刻意设计而非补丁;②`registry.provider_is_async` 的 `mcp./.async/skill.` 前缀回退保留为旧 manifest 兼容路径(ADR-0036 已校订:前缀回退保留而非删除);③`CapabilityProvider::invoke` 占位符形态保留——执行体在异步执行器、占位符只负责拒绝同步直调与声明身份,这是「异步族注册即占位」的一致口径,不是未完成的抽象。
