# ADR-0016: Skill v0.2 脚本执行架构与 Broker 管线覆盖

- 状态: Accepted (用户 2026-09-02 裁决, 待实施前确认)
- 日期: 2026-09-03
- 关联: ADR-0001(三权分立与 Broker 管线), ADR-0005(万物皆插件与安全不变量), ADR-0006(权限以合同显式化), 基线 §4/§5/§7

## 背景

Skill 在 BoenMind 原定义中为挂载在角色/会话上的知识包（提示模板与指令），本身属于静态数据。用户于 2026-09-02 裁决启动 **Skill v0.2** 升级：
1. **第一步（合同面）**：在 `capability/skill.v0_1` 中增发 `version`（版本号）与 `references`（按需加载引用分支文件清单），实现入口指令常驻注入、详细分支文档回合内按需调阅；
2. **第二步（执行面）**：支持技能携带轻量脚本（scripts）执行能力。

为恪守 BoenMind 的核心架构纪律（硬纪律 4：权限以合同显式化；基线 §7：任何跨域与动作调用必须统一经过 Broker 裁决，严禁绕过运行时直接执行代码），必须在写实现代码前明确 **脚本执行引擎选型** 以及 **Broker 七步管线如何全量覆盖脚本执行**。

## 决策

### 1. 运行时选型：wasmtime (WASM 为主，Shell 为辅，零 Python 依赖)
- **主选型 (WASM)**：采用 `wasmtime` 作为安全隔离执行容器。编译为 WebAssembly 字节码的技能脚本具备跨平台单二进制确定性、极低启动开销（亚毫秒级冷启动）、精确的内存/燃料（fuel/gas）限制与纯 Capability-based 的 WASI 资源沙箱（未显式挂载的目录与网络一律不可达）。
- **辅助面 (Shell)**：保留轻量受限的宿主 Shell 脚本执行通道，但必须受操作系统白名单与严格超时看门狗保护。
- **环境解耦**：彻底摆脱对宿主 Python/Node 等外部重型运行时的依赖，保持个人 AI OS 单 exe 原生交付能力。

### 2. Broker 七步管线对技能脚本的全量覆盖

技能脚本作为可执行单元，统一被编译/包装为 `skill.<skill_id>.<script_name>` 形式的标准 Capability，严丝合缝嵌入 Broker 的 7 步调用管线：

```text
       ┌───────────────┐
       │ Agent / Model │
       └───────┬───────┘
               │ 调用请求 (capability = skill.<id>.<name>, args)
               ▼
┌─────────────────────────────────────────────────────────────┐
│ Capability Broker (七步管线)                                 │
│                                                             │
│  Step 1: 身份识别 (Identity)                                │
│          - 校验调用者 principal (如 agent:assistant)        │
│          - 携带内容链信任标记 DataTrust (Untrusted/Trusted) │
│                                                             │
│  Step 2: 权限查表 (Grant & Policy)                          │
│          - 查询 GrantLedger 是否持该 skill 脚本权限         │
│          - 未授权直接 Denied (NoGrant), 绝无默示执行       │
│                                                             │
│  Step 3: 资源与沙箱边界 (Scope & Sandbox Matching)          │
│          - 检查入参资源路径是否落在技能沙箱范围内           │
│                                                             │
│  Step 4: 入参契约校验 (Input Schema Validation)             │
│          - 入参严格按 Manifest 的 input_schema 进行校验     │
│                                                             │
│  Step 5: 审批门禁 (Approval Gate)                           │
│          - 根据脚本 RiskClass (如 external/reversible) 裁决 │
│          - untrusted 来源强制提级审批; 触发 W4b 前端审批卡片 │
│                                                             │
│  Step 6: 凭证与 Binding 锚定 (Call Credential)              │
│          - 签发包含 binding_epoch 的调用凭据                │
│                                                             │
│  Step 7: 隔离执行与出参校验 (Execution & Output Schema)     │
│          - wasmtime 沙箱实例化并注入限定 WASI 上下文        │
│          - 设置 execution deadline 与 fuel 上限             │
│          - 捕获异常/panic，返回值过 output_schema 校验      │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
                   ┌───────────────────────┐
                   │ Event / Execution Log │ (落盘审计与收据)
                   └───────────────────────┘
```

#### 详细管线映射：
1. **身份识别 (Identity)**：调用主体标记为 `agent:<agent_id>` 或 `skill:<skill_id>`；来自用户输入的标记为 `DataTrust::Trusted`，来自模型自发生成的标记为 `DataTrust::Untrusted`。
2. **权限查表 (Policy / Grant)**：脚本必须先在 Registry 中注册有 CapabilityManifest。Broker 在 `GrantLedger` 中以 $O(1)$ 查表核验授权。
3. **资源边界 (Scope)**：wasmtime 容器仅在启动实例化时挂载技能自身的 `data_dir` 与当前工作区的目标目录，禁止越界读取宿主文件系统（防路径遍历与逃逸）。
4. **入参校验 (Input Validation)**：入参 JSON 必须过脚本 Manifest 声明的 `input_schema`。
5. **审批门禁 (Approval Gate)**：带有副作用的脚本（如写入文件、外部调用）若无持久 Grant，则停在 `waiting_approval` 态，触发 W4b 审批流，由用户在前端点击裁决后方可执行。
6. **绑定固化 (Binding & Credential)**：生成绑定当前 Provider 实例与 Epoch 的 `CallCredential`，防止执行期竞态与悬挂。
7. **沙箱执行 (Execution)**：通过 `wasmtime` 执行 wasm 字节码；设置燃料（fuel）和执行超时看门狗（如默认 10s）；执行结果必须过 `output_schema` 校验，并作为事实输出生成收据，由核心单写者写入 Event Log。

## 后果与约束

- **零特权降级**：技能脚本执行在架构上与内置能力、MCP 能力享受完全平等的安全待遇，无特殊直通通道。
- **阶段推进**：
  - 本轮先落地合同 Minor 字段变更（`version` 与 `references`）与架构 ADR 设计；
  - 待用户过目确认本 ADR 设计后，下一轮接入 `wasmtime` 依赖并编写 `bm-providers` WASM 脚本 Provider 实现。
