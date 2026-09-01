# BoenMind 合同工件库（Layer 1）

> 本文不是架构文档。架构原则见《BoenMind 核心架构基线》；这里是基线 M0.2 所说的
> "可机器校验合同"的实际载体。版本 **v1.0（2026-08-28 冻结，M0.2 交付；自冻结起
> 字段只增不破，M4/M7/W4 等按 Minor 只增）**。
>
> **这不是项目源代码，也不只是校验器。**本目录是规格层：源代码在 `../runtime/`
> （Rust workspace），由 AI 实现者按本目录的 schema、迁移表、黄金轨迹和不变量
> 编写并接受验收。validate.py 只是本目录（40+ 文件，22 个冻结 JSON）中的 1 个——
> 它校验合同自身的一致性（R1-R4）；R5（不变量↔测试同名）与 R6（错误码枚举同步）
> 由 `bm-contract`/`bm-testkit` 的 Rust 同步测试在 `cargo test` 中代偿执行。
> 当前合同域：surface/wire/registry/state-machines/capability/task/mcp/memory/
> model/evaluation/budget/logs + golden-traces + invariants。

## 在规格分层中的位置

```text
第 0 层  架构基线（BoenMind-CORE-ARCHITECTURE.md）  原则/边界/不变量/决策
第 1 层  本目录                                     机器可读合同 ← 你在这里
第 2 层  里程碑实现规格                              开工时写，不预写
第 3 层  代码                                       由 AI 按本目录约束生成
```

## AI 实现者的使用方式

```text
1. 实现前：读本目录。schema 定义"长什么样"，迁移表定义"合法路径"，
   轨迹定义"端到端长什么样"，不变量定义"什么是错的"。
2. 实现中：所有出入参必须通过对应 JSON Schema 校验；
   所有事件类型必须在 runtime-events 注册表内；
   所有错误码必须在 error-codes 注册表内；
   状态迁移必须沿 core-transitions 的边走，违例即 bug。
3. 实现后：黄金轨迹必须可回放；每条不变量至少对应一个测试。
```

## CI 校验规则（本库自身的纪律）

```text
R1  所有 .json 文件必须是合法 JSON，schema 必须符合 draft-07
R2  golden-traces 中每个 payload 必须通过其标注的 schema
R3  轨迹中的事件类型 ⊆ runtime-events 注册表；错误码 ⊆ error-codes 注册表
R4  轨迹中的每次状态迁移必须是 core-transitions 中的一条边
R5  invariants/ 中每条不变量必须在测试套件中有对应实现（id 同名）
R6  envelope 中的错误码枚举与 error-codes 注册表保持同步（CI 比对）
```

## 版本规则

```text
v0.x  草稿，可在里程碑回看时修订
冻结（M0.2 交付）：字段只增不破；删除或改名任何字段 = Major 升级，
走基线 13.5 的升级级别流程
扩展错误码/事件类型：新增命名空间条目，不改既有条目
```

> **冻结记录（2026-08-28）**：M1 范围内 9 个 JSON 合同自本日起冻结为 v1.0——
> 每个 JSON 顶部带 `"x-frozen": "2026-08-28"` 机器可读注解。文件名与 `$id` 中的
> `v0.1` 后缀**保持不变**（作为谱系标识，避免破坏跨文件 `$ref` 与黄金轨迹引用），
> 内容版本以本记录为准。自冻结之日起：删字段/改字段名/改字段语义 = Major，
> 新增可选字段 = Minor，修错别字与描述 = Patch（基线 §13.5 分级同样适用于本库）。

## 当前范围（M1）

M1 = 最小 Runtime 与单 Agent 闭环：无 Capability（M4）、无 Approval（M4）、
无 Task/Team（M5/M6）、无 CLI Surface（M3）、无外部 Provider 进程（M7）。
因此本库刻意不定义这些对象——这正是分层规格的意义：
**M4 只需增发 capability.* 工件，不改 M1 已冻结的任何文件。**

```text
wire/envelope.v0_1.schema.json            请求/响应/错误信封
wire/session.v0_1.schema.json             session create/resume/close/poll
wire/agent.v0_1.schema.json               send_input/cancel/get_operation + 执行收据
model/connector.v0_1.schema.json          模型连接器合同 + 调用账本 + 降级链
budget.v0_1.schema.json                   预算对象（开放键值）与记账记录
logs/execution-log-entry.v0_1.schema.json Execution Log 条目
registry/error-codes.v0_1.json            错误码注册表
registry/runtime-events.v0_1.json         运行时事件注册表
state-machines/core-transitions.v0_1.json Operation/Session/Agent 迁移表
golden-traces/M1-GT-01-single-agent-turn.md  黄金轨迹（含成功与超时两条场景）
invariants/M1-invariants.md               不变量断言清单

m0/test-matrix.v0_1.md                    三平台测试矩阵（M0.3）
m0/prompt-injection-cases.v0_1.md         提示注入用例集（M0.4）
m0/threat-model.v0_1.md                   威胁模型与数据信任分级（M0.5）
m0/perf-baseline.v0_1.md                  性能与资源基线定标骨架（M0.6，数值由 M1 回填）
```

## 约定

```text
传输无关：本库只定义信封与负载，不绑定 IPC 形态。
M1 测试进程内直调即可；传输决策属 M3（Surface Protocol）。
ID 格式：  <prefix>_<ULID26>，如 req_01J9Z8G3K2X7M4Q6B8WD5RNYVT
时间：     一律 ISO-8601 UTC（基线 8.3）。
排序：     一律以 event_seq / log_seq 为准，时间戳仅供参考（基线 8.3）。
```
