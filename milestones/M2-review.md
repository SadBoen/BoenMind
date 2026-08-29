# M2 里程碑回看记录(基线 §19 门)

## Evaluation Record

```text
milestone_id:         M2(持久化、事件日志与崩溃恢复)
build_or_commit_id:   8ffa7bd(ADR 结算)→ b204007(T5-T6)→ c47c836(T8)
                      → 本次回看提交(性能回填/本记录)
test_run_id:          cargo test --workspace(2026-08-29,本机)
                      = 68 passed / 0 failed(M1 的 50 项全部保留为回归)
log_range:            每测试装配独立;跨重启 seq 连续(t20)、混沌④审计事件
                      经 INV-3 断言;log_seq 连续(t17)
deterministic_checks: bm-contract 同步测试全绿(22 事件: M1 20 + M2 增发 2);
                      全部事件过 envelope schema;日志条目过 exec-log schema;
                      validate.py 全绿;schema v1→v2 expand 迁移测试(t27)
failure_tests:        S4 真实硬杀进程(t22,混沌①)、claim 幂等续跑(t25)、
                      outcome_unknown 裁定矩阵(t26,INV-10/11)、
                      状态库损坏自重建(t29,混沌②)、过期 CAS 拒绝留痕
                      (t28,混沌④)、同前缀重建确定性(t24,混沌③)
replay_result:        重放与写穿共用同一 materialize reducer;混沌③确定性成立;
                      状态库损坏时自日志重建,行内容与损坏前一致(t29)
llm_evaluation:       不适用(M8.7 起)
known_failures:       见 §6 条件与遗留
architecture_changes: 无合同破坏性变更(2 个事件增发 + agent.created 载荷
                      增 budget 字段,均 Minor 只增);schema v1→v2 expand
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-29
```

## §5 逐门记录

- **A 功能测试**:M2.1 SQLite 规范状态(v2,expand-contract)、M2.2 Append-only
  Event Log(JSONL,fsync)、M2.3 快照与压实(自动策略,CAS 单调)、M2.4
  Event Replay(rebuild_projection)、M2.5 Operation 状态机(M1 已有,持久化
  后回归)、M2.6 outcome_unknown 处置(裁定入口 + claim 续跑)、M2.7 全局
  event_seq 与 resume cursor(跨重启连续 + 跨进程 resume)——全部有测试。
- **B 回归测试**:M1 的 50 项测试全部保留且全绿(GT-01 两场景、INV 全量、
  PI 子集);GT 场景在持久化路径下形态不变。
- **C 故障测试**:S4 真实硬杀(t22)、断线不损会话(M1 INV-6 回归)、
  重复投递不改投影(bus 测试)、损坏状态库自重建(t29);超时/取消(M1 回归)。
- **D 日志回放**:事件日志为唯一重建依据;重建确定性(混沌③)成立;
  Execution Log 与事件流双轨可完整解释一次回合。
- **E 确定性评估**:Schema/状态机/收据机器校验;CAS 门禁底座(混沌④)。
- **F LLM 评估**:不适用。
- **G 架构复盘**:未出现新事实源——事件日志=事实史,SQLite=快路径规范状态,
  互为校验(位点 ≤ max(日志末尾, 快照位点));claim 依赖的输入原文存于
  受保护状态库(A4 边界的解读,见 §8.1),不构成第二条事实源(事件流不可
  重建它,但它的丢失只降级为"需裁定",不产生错误状态)。无 Broker 可绕。
- **H 验收裁决**:passed_with_conditions。
- **I 性能冒烟**:P-02/04/05/07 首次回填(perf-baseline §1.2),进入回归监控。
  P-04=106 条/s 的瓶颈是每条 fsync(Windows),M3 守护形态可改批量提交,
  回看时重新定标口径。

## §6 条件与遗留

1. **CI 三平台确认**:【已关闭 2026-08-29】同 M1-review §6.7——三平台矩阵
   全绿(run 33232349993)。
2. **P-06 RSS**:M3 守护进程形态起独立采样。
3. **WAL checkpoint 策略**:P-07 口径含未检查点 WAL;策略随 M3 守护形态定标。
4. **输入原文存规范状态库**(规格 §8/本次解读):A4 只约束事件与普通日志;
   content 丢失时 claim 降级为"需裁定",无错误状态。M4 威胁模型复核时
   评估是否加密存储。
5. **压实后 resume 补发语义**:自快照位点起的日志后缀即全部可补发内容
   (快照+后缀重建,ADR-0004 条件 2 的设计语义),在 P-02 口径中已验证
   完整日志场景;压实场景的补发截断是设计行为,已文档化。
6. **两个实现期真 bug 由新测试逼出并修复**(快照 CAS 非单调、互为校验
   未感知压实)——测试先行的直接证据,留档。
7. **ADR-0003/0004 条件账本**:已闭合与顺延项见 `M2-adr-settlement.md`;
   其中 0004 条件 3(task_epoch 完整门禁)与条件 4(幂等键全面启用)
   随 M4/M5 落地,0004 条件 6(编排重启触发者/窗口)随 M5。

## §7 回看七问(基线)

1. 解决目标问题?是——强制终止后 Session/Operation 可恢复(混沌①实证),
   幂等续跑成立(claim)。
2. 旧能力可用?是——M1 全量回归绿,GT 两场景形态不变。
3. 崩溃/断线/重复执行?硬杀恢复实证;重复投递不改投影;压实与位点自洽。
4. 日志能解释每一步?能——事件流+Execution Log+可重放(混沌③确定性)。
5. 结果被实际核验?收据/状态机/重建确定性均为机器核验。
6. 合同与状态模型稳定?是——仅 Minor 增发;schema expand-contract 演练成立。
7. 推进或退回?推进——进入 M3(统一 Wire API、CLI 与跨平台启动)。
