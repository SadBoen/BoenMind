# M1 实现规格 v1.0（冻结）

> 第 2 层工件:M1 的技术栈、仓库结构、CI 与任务分解。地位在基线(第 0 层)与
> 合同库(第 1 层)之下;与两者冲突时,以上两层为准。
> 状态:**v1.0(2026-08-29 冻结)**。开放裁决点 D1–D3 已由用户裁决(2026-08-29,
> 全部按本文件建议方案),见 §7;合同解读条款见 §8。此后修改本文件 = 里程碑内
> 修订,须在提交说明中写明动机;语义回看归 §19 门。

## 1. 范围

实现基线 §18 M1.1–M1.8 与合同库 M1 范围的全部对象;验收面 =
黄金轨迹 `M1-GT-01` 两场景可回放 + 12 条不变量(INV-1..INV-12)每条至少一个
同名测试 + `m0/perf-baseline.v0_1.md` 的 P-01..08 数值回填。

非目标(合同库已锁):Capability/Approval(M4)、Task/Team(M5/M6)、
CLI/Surface(M3)、外部 Provider 进程(M7)、跨进程持久化(M2)。
**M1 测试形态 = 进程内直调**,无网络服务。

## 2. 技术栈

| 项 | 选择 | 理由 |
|---|---|---|
| 语言 | Rust stable(edition 2024),`rust-toolchain.toml` 钉版本 | 基线 §17 裁决 Rust Runtime Core;本机 1.98 |
| 异步运行时 | tokio(`rt-multi-thread`, `time`, `sync`, `macros`) | 事实标准;`tokio::sync` 足够支撑单写者 bus |
| 序列化 | serde + serde_json | Wire API 即 JSON(信封 schema draft-07) |
| Schema 校验 | `jsonschema` crate(draft-07) | **[裁决]** 直接消费 `boenmind-contracts/` 的冻结 schema 文件,合同即单一真源,详见 §4.1 |
| ID | `ulid` | 合同约定 `<prefix>_<ULID26>` |
| 时间 | chrono(`serde` feature) | 合同约定 ISO-8601 UTC |
| 错误 | thiserror(crate 边界用具体错误类型,不跨边界用 anyhow) | 错误必须能映射到合同错误码(§2 registry) |
| Secret | `keyring` crate(OS keychain;Windows=Credential Manager)+ 加密文件兜底 | M1.6;keyring 三平台覆盖,正是 M0.3 测试矩阵要验证的适配点 |
| HTTP(可选) | reqwest(rustls),feature-gated | 见 §4.3 [开放 D1] |
| 诊断日志 | tracing + tracing-subscriber | 与 Execution Log(合同对象)严格分离,见 §4.4 |
| 属性测试 | proptest | INV-1/INV-4 的检查方式即属性测试 |
| 测试跑器 | cargo-nextest(本地与 CI) | INV-* 命名用例的分组与重跑体验;`cargo test` 兼容兜底 |

## 3. 仓库结构(Rust workspace)

```text
runtime/
  Cargo.toml                 # workspace,tokio 单一版本来源
  rust-toolchain.toml
  crates/
    bm-contract/             # L1 合同的 Rust 投影
      src/wire/              #   envelope / session / agent 类型(serde)
      src/model/             #   connector 合同类型 + 调用账本
      src/registries.rs      #   事件/错误码注册表的编译期内嵌(include_str!)
      src/schemas.rs         #   冻结 schema 内嵌 + jsonschema 编译缓存
    bm-core/                 # L2 Runtime Core(M1 子集)
      src/runtime.rs         #   启停(M1.1)
      src/session.rs         #   Session 状态机(M1.2)
      src/agent.rs           #   Agent 状态机 + 回合驱动(M1.3)
      src/operation.rs       #   Operation 状态机 + 执行收据(M1.4)
      src/bus.rs             #   Event Bus 内存版,event_seq 单写者(M1.5)
      src/exec_log.rs        #   Execution Log(JSONL 追加写)
      src/budget.rs          #   预算记账与三强制点(M1.8)
      src/ports.rs           #   ModelConnector / SecretStore trait(可替换点)
    bm-providers/            # ports 的 M1 实现
      src/mock_model.rs      #   确定性 mock 连接器(脚本化响应,默认)
      src/glm_http.rs        #   真实 GLM 适配器(feature = "glm",见 D1)
      src/keyring_secret.rs  #   OS keychain 实现
      src/file_secret.rs     #   加密文件兜底
    bm-testkit/              # 测试支撑(不进生产二进制)
      src/replay.rs          #   黄金轨迹回放器:驱动 runtime 逐条比对
      src/invariants.rs      #   INV 断言 helper(泄漏扫描、迁移表校验)
      src/chaos.rs           #   断连/取消/超时注入
    bm-runtime/              # bin:进程内组装入口(M3 前无 CLI、无网络监听)
```

划分原则:合同类型独立成 crate(L1/L2 边界的编译期体现);可替换 Provider
只依赖 `bm-core::ports` 的 trait,不依赖实现(万物皆插件的预告);M7 外置进程时
从 `bm-providers` 拆出,调用方无感(基线 5.4)。

## 4. 关键设计决策

### 4.1 合同的消费方式 **[裁决]**

- 冻结 schema 用 `include_str!` 编译期内嵌进 `bm-contract`,运行时零文件依赖;
  CI 里有一步校验"内嵌副本与 `boenmind-contracts/` 逐字节一致",防止漂移。
- 生产路径:serde 类型为快路径;`strict-schema` feature 打开后每个出入参过
  jsonschema(慢,仅诊断用)。
- 测试路径:黄金轨迹回放、样例 payload 一律过 schema——合同库 CI 规则 R2 在
  实现侧的镜像。

### 4.2 event_seq 与单写者 **[裁决]**

M1 的 Event Bus 是进程内内存版:单一 `mpsc` 通道 + 单写者任务分配
`event_seq`(严格递增、无空洞,INV-3 只约束单次运行)。订阅方按 seq 排序投影,
乱序投递不改变投影——这是 M2 持久化前的同构演练,接口按"可换持久后端"设计。

### 4.3 模型连接器:mock 为主,真实为辅 **[已裁决 D1]**

- 默认交付:`MockConnector`,脚本化响应/超时/失败序列——GT-01 场景 A/B、
  降级链、INV-4 全部可确定性回放;P-01..08 性能数值按 perf-baseline 约定以
  mock 回填。
- **D1 裁决(2026-08-29)**:真实 GLM HTTP 适配器列入 M1 交付,但
  `feature = "glm"` 门控、默认关闭、不参与 P-01..08 定标;M1 验收不依赖外网。

### 4.4 Secret 边界(M1.6)与 INV-5

凭据只存在于 `SecretStore`;上下文/事件/日志/错误信封只允许
`secret:<namespace>/<name>` 引用格式。`bm-testkit` 提供泄漏扫描:对全量事件 +
Execution Log + 收据执行 secret 明文 grep,命中数必须为 0(INV-5 测试形态)。

### 4.5 取消与终态语义(M1.4)

迁移严格走 `core-transitions`:只有 `agent.cancel` 或用户裁定可产生
`cancelled`(INV-12);`session.close`/`runtime.stop` 走 detach/排空,进行中
Operation 状态不变(INV-6)。模型调用无外部副作用,失败必须落 `failed`
(GT-01 场景 B 的关键对照),`outcome_unknown` 在 M1 只允许由 guard 表达式
触发(INV-10/11 的 fuzz 用例覆盖)。

## 5. CI(GitHub Actions)

仓库:`SadBoen/BoenMind`(公开)。触发:push main / PR / tag。

```text
job contracts-validate   ubuntu   python boenmind-contracts/scripts/validate.py(全绿门)
                                  + bm-contract 内嵌副本与合同库一致性比对
job test                 matrix: ubuntu-latest / windows-latest / macos-latest
                                  rustfmt --check
                                  cargo clippy --workspace --all-targets -- -D warnings
                                  cargo nextest run(P0 套件 = 全部 INV-* + GT-01 回放)
job perf-mock            ubuntu   P-01..08 mock 定标跑,结果作为工件存档(M1 回看用)
```

三平台矩阵即 M0.3 测试矩阵的执行载体;Windows 是第一优先平台(本机)。

## 6. 任务分解与顺序

```text
T0  workspace 骨架 + CI 三 job 空跑绿                       (基础设施)
T1  bm-contract:类型 + schema 内嵌 + 注册表同步测试
T2  M1.1 启停:runtime.started/stopping/stopped,bus 单写者   (INV-3)
T3  M1.2 Session:create/resume/close                         (INV-6/8)
T4  M1.3 单回合 + MockConnector + 账本                       (INV-1/2/4)
T5  M1.4 错误/取消/超时 + 降级链                              (INV-10/11/12,GT-01 场景 B)
T6  M1.5 Execution Log(JSONL)+ 泄漏扫描                     (INV-5)
T7  M1.6 SecretStore(keyring + 兜底)                        (INV-5)
T8  M1.8 预算三强制点                                         (INV-7)
T9  GT-01 回放器收口:两场景逐条比对                          (全 INV)
T10 P-01..08 mock 回填 + 三平台留痕 + §19 回看门              (M1 收尾)
```

依赖:T1 → T2 → T3 → T4 →(T5,T6,T7)→ T8 → T9 → T10。
每步合并后主干必须保持可校验(AGENTS.md 硬纪律 6)。

## 7. 裁决点(2026-08-29 用户裁决,全部定案)

| # | 问题 | 裁决 |
|---|---|---|
| D1 | 真实 GLM 适配器是否列入 M1 | 列入,feature 门控默认关,验收不依赖外网 |
| D2 | crate 前缀 `bm-` | 可用;对外发布名 M3 再定 |
| D3 | M1 的 resume 语义边界 | 仅同进程内 resume(session.resume 对 active 会话幂等重连、对 closed 会话报 validation_failed);跨进程恢复明确归 M2 |

## 8. 合同解读条款(实现期裁决,§19 回看时复核)

实现中发现合同文本存在少量需裁决的缝隙,按以下解读实现;均为对合同**的
读法**,不是对合同的修改,逐条留档供回看追认或修订:

1. **`not_started→running` 不发射 `operation.state.changed` 事件**。迁移表
   描述说"每次迁移必须产生一条 operation.state.changed",但黄金轨迹场景 A
   事件序号 1..11 连续(INV-3 无空洞)且不含该事件——收据本身同步承载了
   该迁移(A2 响应 state=running)。以 GT 为验收权威,仅对 dispatch 之后的
   迁移发射事件;INV-2(已发射事件的边合法性)不受影响。
2. **预算拒绝不创建 operation**。迁移表给 `not_started` 只有
   `→running`(dispatch_accepted)与 `→cancelled` 两条边,预算拒绝的回合
   若创建 operation 将永远无法到达终态,违背 INV-1 的终态条款。故
   强制点①在创建 operation 之前执行:send_input 直接返回
   `budget_exceeded` 错误信封,无收据。INV-1 的"恰好产生一个 operation_id"
   适用于通过前置检查的 send_input。
3. **secret_ref 采用 schema 合法格式**(`secret:model.zhipu`)。黄金轨迹
   示例 `secret:model/zhipu` 的 `/` 不符合 connector 合同的
   `^[A-Za-z0-9_.-]{1,64}$` 模式;以 schema 为准,回看时修订 GT 示例(Patch)。
4. **收据 action_summary 使用无内容模板**(如"回合 N 完成(X 入 / Y 出
   token)")。GT 示例回显了输入要义,但 PI 公共断言 A4 要求载荷原文不进
   普通日志;结构(shape)与 GT 一致,文本取安全侧。
5. **M1 义务边界**(供 §19 门,不是豁免):S4 崩溃恢复与 P-02/04/05/07
   依赖持久层,归 M2 回看补验;PI 用例集 M1 子集 = PI-01/05/10/11
   (断言 A1/A4 + 不崩溃 + 不改变会话状态),其余依赖工具/记忆/审批,自
   M4+ 全面生效(prompt-injection-cases 的 M1 适配条款)。
6. **`agent.started` 等 6 类事件在 M1 主流程不发射**。注册表是封闭允许集,
   非强制发射集;GT-A 不含 `agent.started`,故 created→starting→running
   迁移不产生事件。
