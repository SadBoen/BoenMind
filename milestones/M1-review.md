# M1 里程碑回看记录(基线 §19 门)

## Evaluation Record

```text
milestone_id:         M1(最小 Runtime 与单 Agent 闭环)
build_or_commit_id:   2972957(实现主提交)+ 本次回看提交(性能回填/本记录)
test_run_id:          cargo test --workspace(2026-08-29 夜间,本机)
                      = 50 passed / 0 failed(含 GT-01 两场景、fuzz 500 用例)
log_range:            每测试装配独立事件流,event_seq 恒为 1..N 连续(INV-3 断言);
                      Execution Log log_seq 1..M 连续(t17 断言)
deterministic_checks: bm-contract 同步测试(R2/R3/R4/R6 镜像)全绿;
                      全部事件过 envelope schema;日志条目过 exec-log schema;
                      validate.py 全绿
failure_tests:        t08(降级链超时→failed 非 outcome_unknown)、
                      t09(显式取消→cancelled)、t10(终态取消被拒)、
                      t05(排空停机无 cancelled)、t04(close-in-flight 不损 Session)、
                      t13(回合数上限)、t16(加密文件库往返)、泄漏扫描 0 命中(t14)
replay_result:        GT-01 场景 A:11 条事件逐条形态+payload 值一致 ✓
                      GT-01 场景 B(简版补全):12 条事件序列一致 ✓,错误信封
                      timeout/retryable=false 与轨迹一致 ✓
llm_evaluation:       不适用(M1 无工具调用与领域任务;M8.7 独立 Judge 起)
known_failures:       见下方「已知边界与遗留」
architecture_changes: 无合同变更;规格 §8 六条解读条款待复核(见 C-1..C-6)
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-29
```

## §5 逐门记录

- **A 功能测试**:M1.1–M1.8 全部落地并有对应测试——启停(t02/t03)、
  Session 生命周期(t03)、单回合+模型调用(t07)、错误/取消/超时(t08–t10)、
  Execution Log(t17/t14)、Secret Store(t14/t16)、连接器合同+降级链(t08)、
  预算记账(t12/t13)。
- **B 回归测试**:M0 无代码,合同库 validate.py 全绿 + bm-contract 同步测试
  即回归面;全绿。
- **C 故障测试**:断网/Provider 崩溃归 M7(外部进程不存在);进程终止崩溃恢复
  归 M2(无持久层,S4 依 §8.5 明确延期);超时/重复投递/断开已测(t08、
  bus 投影测试、t04)。**M2 回看须补:杀进程后 resume、损坏库、同 seq 前缀
  重建确定性、旧 epoch 拒绝(ADR-0004 增补四项)。**
- **D 日志回放**:事件流与 Execution Log 可完整解释一次回合的全过程
  (t17 三类条目 + GT 回放即回放验证);M2 落持久层后补跨进程回放。
- **E 确定性评估**:Schema/状态机/Operation 收据全部机器校验;M1 无权限面
  (M4)。
- **F LLM 评估**:不适用(M1 无领域任务)。
- **G 架构复盘**:未出现新事实源(事件日志为唯一时序源,内存投影可重建);
  无 Broker 可绕(M4 前无跨域能力);bm-contract/bm-core/bm-providers 分层
  与 L1/L2/Provider 对齐,连接器与 Secret Store 均为端口注入,无内核特权。
- **H 验收裁决**:passed_with_conditions。
- **I 性能冒烟**:P-01/03/08 首次回填(见 perf-baseline §1.1),无历史基线,
  自本记录起进入回归监控。

## §6 条件与遗留(进 M2/M3 议程)

1. **S4/P-02/04/05/07 延期**(规格 §8.5):依赖持久层,M2 开工即补,
   M2 回看按 ADR-0004 四项混沌测试验收。
2. **P-06 RSS**:M3 守护进程形态起独立采样。
3. **GT-01 示例 secret_ref `secret:model/zhipu` 含 `/`**,不在 connector 合同
   字符集内(规格 §8.3):建议 Patch 级修订 GT 示例为 `secret:model.zhipu`,
   下次合同库触碰时一并处理。
4. **`agent.started`/`session.resumed` 等注册表事件在 M1 的发射面**(规格 §8.6):
   session.resumed 已发射;agent.started 仍无触发流程,维持「封闭允许集」读法。
5. **GLM 适配器(D1)仅编译验证**,未联调真实端点:留待 M2 开工前人工联调
   (feature 门控不影响验收)。
6. **action_summary 无内容模板**(规格 §8.4)与 PI A4 一致,复核通过。
7. **Windows CI**:【已关闭 2026-08-29】ci.yml 已推送(80cc449+ae0aa09 修正
   working-directory),三平台矩阵全绿(run 33232349993:contracts-validate ✓
   / ubuntu ✓ / windows ✓ / macos ✓)。

## §7 回看七问(基线)

1. 新增能力解决目标问题?是——单 Agent 闭环端到端可运行,验收面全部机器化。
2. 旧能力可用?是——合同库与全部 M0 工件未动,validate.py 全绿。
3. 崩溃/断线/重复执行?断线不损 Session(INV-6 实证);重复投递不改投影;
   崩溃恢复待 M2(已知边界,非回归)。
4. 日志能否解释每一步?能——事件流 + Execution Log 双轨覆盖一次回合全过程。
5. 结果被实际核验?M1 层面收据/状态机/泄漏扫描均为机器核验;领域结果核验
   属 M5 Observation。
6. 合同与状态模型稳定?是——实现期未改任何冻结合同;六条解读条款留档。
7. 推进还是退回?推进——进入 M2(开工前结算 ADR-0003/0004 验收条件)。
