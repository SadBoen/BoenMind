# M7 里程碑回看记录(基线 §19 门)

## Evaluation Record

```text
milestone_id:         M7(Provider、MCP 和 App 隔离)
build_or_commit_id:   833ff23(T0 合同/规格/ADR-0010)→ 8c17201(T1 真实连接器)
                      → fa0c759(T2 模型调用过 Broker)→ 4eba662(T3 MCP 接入)
                      → 本提交(T4 健康面 + 回看)
test_run_id:          cargo test --workspace(2026-08-30,本机)
                      = 213 passed / 0 failed(M6 196 → M7 213;增量:
                      t100-t104 MCP 接入、t105-t109 健康信任隔离、
                      m7_provider_tests 离线 6、broker::m7_tests 1);
                      实网验证 1 次(#[ignore] 门控,见下)
log_range:            事件 41 → 43(capability.progress / provider.health.changed)、
                      capability.invoked 覆盖模型调用(授权点/审计点分离,
                      预生成 call_id 缝合)、grant.created 新增 agent 创建
                      即授 model.invoke(创建即授,可撤销,持久不丢)
deterministic_checks: validate.py 全绿;事件 43 断言与镜像同步;
                      mcp-server.v0_1 新合同(Minor,纯追加)+ GT-05
failure_tests:        t103 deadline 超时不无限等待、t105 熔断开闸/冷却快速
                      失败/半开恢复、t106 MCP 崩溃→unavailable→重连恢复→
                      超限封禁、t104 stdio 子进程真死亡重生(#[ignore])、
                      t107 配置不合规/env 明文拒绝
replay_result:        GT-05 场景 A(接入→首调审批→进度→成功)与场景 B
                      (崩溃→快速失败→重连)由 e2e 承载;GT-01(更新回合流
                      12 条)/02/03/04 回归绿
llm_evaluation:       不适用(M8.7 起);但本里程碑起 LLM 通道为真实网关:
                      t116 实网验证通过(gpt-5.6-luna,4691in/7out/2.13s)
known_failures:       见 §6 条件与遗留
architecture_changes: 合同 Minor:事件 +2、mcp/mcp-server.v0_1、GT-05;
                      ADR-0010(第三方网关信任边界);M4 §5.8 模型调用
                      豁免撤销(模型是能力不是旁路);内核新增
                      AsyncCapabilityExecutor 端口与 provider 健康面;
                      C4 模型零改动(实现既有拓扑)
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-30
```

## §5 逐门记录

- **A 功能测试**:M7.1 内置 Capability Provider(model.invoke 注册、
  Broker 决策、Grant 授权、审计三路覆盖)、M7.2 MCP Server 接入
  (InProc + stdio 双传输、握手/发现/动态注册/异步调用)、M7.3 handshake
  与能力发现(tools/list → manifest 生成,annotations → effect/approval
  映射,不合规工具名拒注册)、M7.4 崩溃/重启/unavailable(熔断 + 重生 +
  探针上限)、M7.5 进度/超时/取消(progress 回注、deadline 折算超时)、
  M7.6 数据域隔离、M7.7 安装与首调信任——全部有测试实证。
- **B 回归测试**:M1–M6 存量全绿;GT-01 期望序列按新事件流更新
  (回合流 11→12 条,含 capability.invoked);GT-02/03/04 回放绿。
- **C 故障测试**:Provider panic 收容(M4 既有 catch_unwind,异步执行器
  隔离于 dispatch spawn)、MCP 子进程死亡不拖垮核心回路(t106)、
  熔断期零触达(call_count 不变)、stdio 双 pending 表 bug 与
  Windows 管道缓冲陷阱在测试先行下暴露并修复。
- **D 日志回放**:GT-05 双场景逐事件可回放;模型调用审计经
  预生成 call_id 与授权点缝合;provider.health.changed 只在迁移沿发射。
- **E 确定性评估**:错误映射三分(5xx/429/传输=可重试 unavailable、
  4xx=不可重试、解码失败=internal)单测;工具名规范化(连字符归一、
  非法段拒)单测;scope choices 消费语义(once 耗尽回到审批)t108。
- **F LLM 评估**:不适用(M8.7 起)。
- **G 架构复盘**:内核对 MCP 零感知——只依赖 AsyncCapabilityExecutor
  通用端口,`manifest.provider` 前缀是唯一路由标记(可替换性守恒);
  健康面是进程内软状态不入 core-transitions(合同不破);密钥三层实现
  (Mem/Keyring/AES-GCM File)复用,M7 仅增装配路径,明文零入仓
  (INV-5,.secrets/ gitignored + example 模板)。测试钓出并修复两处
  真 bug:审批重放不识 DispatchedAsync(失败收口 → 完成回流撞表外
  迁移杀死核心回路)、Wire 直调从不挂 idempotency_key(M4 潜伏缺口)。
- **H 验收裁决**:passed_with_conditions。
- **I 性能冒烟**:记录⑤——P-03(turn 过 Broker)p50 +0.8%/p99 +1.6%
  门内;P-01 p95 首遍 0.281 触门解释(双复跑 0.249/0.194 为噪声,
  p50 三值稳定门内);P-08 门内。

## 基线 M7 通过条件结算(五句逐条)

1. **调用方只依赖 Capability** ✓——模型调用收编为 model.invoke 能力
   (M4 §5.8 豁免撤销,ADR-0010);agent 创建即授永续 Grant(ADR-0006
   权力显式化),turn 前置 Broker 查表,Wire 直调执行体即拒;调用方
   (turn 循环/Wire)只见能力名。
2. **MCP Provider 可以发现、调用和报告进度** ✓——t100(发现/注册)、
   t101(调用收据/幂等/outbox published)、t102(progress 事件)、
   t104(stdio 真子进程)。
3. **Provider 崩溃不会拖垮 Runtime** ✓——MCP 故障隔离在子进程
   (t106:核心回路照常,后续调用快速失败/重连);同步 Provider panic
   被 execute 收容(M4 既有);异步执行器隔离于 dispatch spawn。
4. **失败调用不会无限等待** ✓——t103(deadline 折算 HTTP 超时预算,
   到点 Failed{timeout})、t105(熔断冷却期快速失败)、t106(超限封禁
   同步拒绝)。
5. **App 不能通过内部数据库绕过 Broker** ✓——结构面:存储只在核心
   循环内可达,Surface 16 方法无存储暴露(sync.rs 注册表对账);策略面:
   App 主体不享内建直通,跨 provider 访问默认拒绝,显式 Grant 后放行
   (t109 + broker::m7_tests)。

## 前置结算与承接项闭合

- **M4 §5.8「模型调用豁免 Broker」:撤销**(T2 落地,见通过条件 1)。
- **M4-review「真实副作用 Provider 的收据/幂等/outbox 对账实证」:闭合**
  ——t101 以 MCP external-side-effect 工具全链路实证(intent → published
  对账行 → 幂等抑制返回原收据)。
- **M4-review「lease 通道真实吞吐」:移交 M8**(第三方网关无 lease 型
  Provider;真实 App 落地时首测)。
- **deepwiki S4 draining / S5 quarantined 分表:移交 M8**(真实 App 与
  压测时裁决);S9 verification 已随 M5 completion gate 闭合。
- **D-M5-2(memory:user 授权面):移交 M8**(随首个用户数据 App)。
- **M6-review 遗留(worker 自主 turn 环)**:管道已通(worker 可调真实
  能力);自主编排环属 M8 多 Surface 协作面。

## §6 条件与遗留

1. **t116 实网验证为一次性门控**(`#[ignore]` + BOEN_LIVE=1):通过
   (2026-08-30,gpt-5.6-luna「连接成功。」4691in/7out/2.13s)。条件:
   M8 长任务压测(M8.4)须在真实通道上复测稳定性;第三方网关的内容
   可见性风险由 ADR-0010 记录,官方直连通道留后续 ADR。
2. **模型流式(SSE)与多模型降级链未实现**(M7 规格 §三-1/3 明确不做):
   非流式 + 单模型 gpt-5.6-luna;M8 按需增发。
3. **MCP HTTP/SSE 传输、resources/prompts 桥接未实现**(规格 §三-2):
   stdio v1 满足通过条件;M8 按 App 需求裁决。
4. **用户显式取消在途能力调用**:deadline 驱动 + CancellationToken
   管道已备;用户面取消入口随 M8 多 Surface 协作(M8.3)。
5. **stdio 进度订阅为单代**:重生代进度接续已留位(订阅位空缺时续上),
   多代进度聚合随 M8 压测验证。

## §7 回看七问(基线)

1. **计划与实际的偏差?** 规格七子项全部落地(S1-S6 裁决兑现);
   偏差一:原计划 T1 含「server 装配 env」提前完成,T4 才补 --mcp-config
   (安装面随信任裁决走);偏差二:stdio 传输多修了两个测试暴露的传输
   bug(双 pending 表、Windows 管道缓冲)——测试-first 的收益面。
2. **哪些是临时绕路?** 无。健康面未做持久化(进程内软状态,重启即
   重建)为有意取舍,非遗留。
3. **合同是否被破坏?** 否。事件 +2 与 mcp-server.v0_1 均为 Minor
   纯追加;connector.v0_1 零改动(OpenAiConnector 走既有 InvokeRequest/
   InvokeResponse);validate.py 全绿。
4. **性能是否触门?** P-03/P-08 门内;P-01 p95 首遍触门已解释
   (噪声,记录⑤)。
5. **安全边界是否松动?** 否,收紧两处:模型调用过 Broker(豁免撤销)、
   App 主体默认拒绝;密钥零入仓(INV-5 扫描面覆盖三层 Secret Store);
   审计面扩至模型调用与 MCP 全链路。
6. **下一个里程碑最需要什么?** 真实 App(Wiki/确定性领域)吃这套路
   由——它们将首次同时使用:真实模型通道、MCP 工具面、Task 编排、
   审批 UI;M8 规格须把「长任务在真实通道上的稳定性复测」列为硬条件。
7. **如果重做会怎样?** 会把 provider 健康面(而非 MCP 客户端)排在
   MCP 之前——T4 的状态机是 T3 超时/失败语义的自然收口,顺序对调可
   少一次 dispatch 分支返工;其余不变。
