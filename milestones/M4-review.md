# M4 里程碑回看记录(基线 §19 门)

## Evaluation Record

```text
milestone_id:         M4(Capability、Broker、权限和审批)
build_or_commit_id:   ff90b60(T4)→ 1f4f4ea(T3d)→ 5df81ff(T5)→ 68bdff1(T6a)
                      → 3808c78(T6b)→ 0943d06(T7)→ 493e8a1(T8)→ 66e8b51(T9)
test_run_id:          cargo test --workspace(2026-08-29,本机)
                      = 134 passed / 0 failed(增量:M1 50 起,e2e t34–t46、
                      守护 G2/G3、证伪 P-09/10/半径、决策矩阵 30 格、PI×5)
log_range:            审计事件族首度入流:capability.invoked(intent/ok/error/
                      suppressed)/capability.denied/approval.requested/
                      resolved/expired/grant.created/provider.binding.changed/
                      bus.degraded;GT-02 双场景回放由 e2e t40–t43/t34 承载
deterministic_checks: validate.py 全绿(合同库 21 文件,含 capability 4 份新
                      合同 + GT-02;R2–R4 泛化为轨迹遍历);信封/事件/收据
                      schema 全校验;迁移表边校验(waiting_approval 三边)
failure_tests:        t41(高危恒审批+deny 闭环)、t44(幂等抑制:Provider 恰
                      执行一次,审计可证)、t45(outbox pending→outcome_
                      unknown 恢复)、t46(持久故障降级:副作用拒绝+查询照常)、
                      注入测试(伪造类型行→Corrupt 拒开)、panic 收容、
                      P-09/P-10/P-半径三证伪
replay_result:        GT-01 两场景回归绿;GT-02 场景 A(直通+高危拒)/B(untrusted
                      升级→批准→Grant→执行)由 e2e 全链路承载
llm_evaluation:       不适用(M8.7 起)
known_failures:       见 §6 条件与遗留
architecture_changes: 合同 Minor 增发:capability 4 合同 + wire/capability +
                      envelope 枚举(method+3/error_code+4)+ 事件注册表 +10 +
                      状态机 waiting_approval 三边 + transport rpc_path +3 +
                      GT-02 + validate.py 轨迹遍历泛化 + perf-baseline P-09/10;
                      实现侧:模型调用豁免 Broker 显式留档(规格 §5.8,M7 复议)
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-29
```

## §5 逐门记录

- **A 功能测试**:M4.1 Registry(两层+epoch 单调+发现面)、M4.2 Broker(O(1)
  查表+七步逻辑拆分+凭证)、M4.3 出入参 schema 校验、M4.4 权限交集(Grant
  台账+谓词)、M4.5 Approval 持久+恢复、M4.6 审计归因链、M4.7 审批状态机
  (once/count/ttl/forever)+waiting_approval、M4.8 input_trust 门控(构造面
  收权+effective 上提)——全部有测试或制品实证。
- **B 回归测试**:M1–M3 全部存量测试保留且全绿;GT-01 回放、信封逐字节、
  混沌四项、SSE 断线重连均在场;性能 P-01..P-08 复跑门内(见 §6.6)。
- **C 故障测试**:Provider panic 收容、持久写故障降级(t46)、outbox 崩溃
  窗口恢复(t45)、伪造/乱序日志注入检出(bm-persist)、Grant 撤销/过期/
  耗尽、binding 切换凭证拒绝。
- **D 日志回放**:GT-02 双场景逐事件形态可回放;capability.invoked 的
  intent→ok/suppressed 序即副作用证据链;validate.py R2–R4 遍历双轨迹。
- **E 确定性评估**:30 格决策矩阵(5 风险×3 trust×有/无 Grant)硬断言;
  untrusted→reversible+ 升级率 100%、越权拒绝率 100% 为 CI 门槛(非报告
  指标);PI-02/03/06/07/12 决策层同名测试。
- **F LLM 评估**:不适用(M8.7 起)。
- **G 架构复盘**:三权分立首次真实落地——Registry 只答「谁提供什么」,
  Broker 独占「能不能调」,Bus 只述事实;无特权通道(构造面收权 + 决策
  矩阵 + G3 反断言);capability 操作不落 operations 表(系统容器内存态,
  规范状态由 approvals/grants 承载)——留档 §6.5 回看复核;模型调用豁免
  Broker 显式登记(规格 §5.8,M7 复议)。
- **H 验收裁决**:passed_with_conditions。
- **I 性能冒烟**:P-01..P-08 复跑门内(P-05 首跑触门经核查判噪声,双值
  留档);P-09 首填 release p99≈0.2µs(门 10µs,余量 50×);P-10 零劣化
  (perf-baseline 记录③)。

## 11 条硬约束逐条结算(settlement → 实证)

| # | 硬约束 | 裁决 | 实证 |
|---|---|---|---|
| 1 | Broker O(1) 查表 + 三证伪 | ✅ 落地 | P-09 p99≈0.2µs(门 10µs);P-10 零劣化;panic 收容测试 |
| 2 | binding_epoch 三方一致 | ✅ 落地 | 凭证签发+执行点重验;切换测试(epoch 1→2 拒旧凭证/旧 lease);epoch 持久恢复不回退(t43 前置) |
| 3 | 事件校验加固+伪造/乱序注入 | ✅ 落地 | 持久前形状拒绝+store.write.rejected 告警;伪造类型行→Corrupt 拒开 |
| 4 | lease 准入四测试+吞吐解耦 | ✅ M4 范围落地 | epoch/policy_version/deadline/byte_budget 四门测试;真实吞吐 M7 复测(条件) |
| 5 | 事务性 outbox+指标上限 | ✅ 落地 | intent 前门禁+pending→published 行;t44 duplicate=0;t45 崩溃窗口→outcome_unknown 闭环;audit_gap=0 由前门禁+恢复承载 |
| 6 | Bus 暂停降级+物理隔离 | ✅ 落地(A/B 两态) | A:订阅者离开不阻塞核心;B:FailingStore→拒写降级+bus.degraded;bus.resumed 发射点随 M8(条件);lease 通道不落盘=隔离结构 |
| 7 | 守护三件套进 CI | ✅ 落地 | G1(命令语义拒绝)/G2(审批命令不入流)/G3(缓存清空行为一致)常驻 m4_guard_tests |
| 8 | manifest 分级+Grant 下限集 | ✅ 落地 | mutation_class 字段;Grant 全字段(delegation_depth=0/撤回版本/父哈希);Approval 六态 |
| 9 | input_trust 100%/100% CI 门槛 | ✅ 落地 | 构造面收权(surface/content_chain/TrustViolation)+30 格矩阵+PI×5 同名测试 |
| 10 | 双路径统一合同(定义不启用) | ✅ 定义面落地 | Grant/幂等/收据合同唯一;Domain Agent 随 M5 启用,双开禁止由结构保证 |
| 11 | 幂等抑制审计证明 | ✅ 落地 | t44:suppressed 事件 + key_hash 与 ok 一致 + Provider 恰执行一次 |

**裁决:11/11 落地(其中 4/6 附 M7/M8 复测条件)——ADR-0001/0002 对外口径
自本回看待升级为「成立」;ADR-0001 条件 1-7 中 M4 范围项全部实证,ADR-0002
条件 1/3/4/6 实证、条件 2 部分(子树过滤随 M5)、条件 5 部分(memory.* 随 M5)。**

## S1–S10 相关项裁决(deepwiki-validation 修订建议)

- **S5(注册期前置校验)**:部分采纳——M4 register() 注册期即拒绝非法
  manifest(Err,非 handshake 期);quarantined 分表随 M7 插件安装。
- **S4(draining 两步)**:部分相关——M4 已有 binding 状态位与原子切换;
  「摘除→排空→终止」完整两步随 M7 真实进程。
- **S8(Wire 合同分方向)**:方向已实践(wire/capability 独立文件);完整
  重组属合同 Major,留 M8 发行前评估。
- **S9(verification 三分法)**:manifest 字段已预留;Liveness/Readiness/
  Startup 动作映射随 M7 Provider 生命周期。
- S1/S2/S3/S6/S7/S10:属 M7(Provider 进程)/M8(升级与发行)范围,本回看
  不裁决。
- 其余 S 项维持 proposed,随对应里程碑回看逐条处理。

## §6 条件与遗留

1. **M7 复测项**(settlement 0001-4/5):lease 通道真实吞吐、真实副作用
   Provider 的收据/幂等查询/outbox 对账实证——mock 面已全部落地。
2. **M8 项**:bus.resumed 事件发射点(恢复路径=重启,部署形态定标);
   approval.list 全量查询 UI;CLI capability list(发现面 Wire 暴露)。
3. **T6c 收紧项**(留档):count 类 Grant 消费余量不持久化(重启回满);
   幂等收据仓为内存态——两者随 outbox 事务性写序统一收紧。
4. **回看复核项**(规格 §8/提交留档):capability 操作不落 operations 表
   (系统容器内存态);模型调用豁免 Broker(M7 外置时撤销);forever
   scope 的 M5 收紧策略;P-05 噪声双值留档。
5. **CI 三平台确认**:推送后矩阵全绿(134 项;R5/R6 镜像同步断言在列)。

## §7 回看七问(基线)

1. 解决目标问题?是——所有跨域调用经 Broker 统一裁决;高危操作强制过
   用户;审批跨重启可恢复;副作用防重防漏有机制有测试。
2. 旧能力可用?是——M1–M3 存量 129 项全绿;信封/事件/状态机零破坏
   (合同全 Minor 增发)。
3. 崩溃/断线/重复执行?t45 崩溃窗口收敛为 outcome_unknown 可裁定;
   t44 幂等抑制 Provider 恰执行一次;resume cursor 补发照常。
4. 日志能解释每一步?能——capability.invoked(intent/ok/error/suppressed)
   + denied + approval 三事件 + grant 二事件,归因链含 principal/epoch/
   instance/key_hash。
5. 结果被实际核验?决策矩阵 100%×100% 量化门槛硬断言进 CI;双零指标
   (audit_gap/duplicate)注入测试承载。
6. 合同与状态模型稳定?是——全 Minor 增发;字段只增不破;镜像断言
   (sync.rs 18 项)守门。
7. 推进还是退回?推进——M4 收官,进入 M5(Butler/Task/长期监护)。
