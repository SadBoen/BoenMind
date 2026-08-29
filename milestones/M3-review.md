# M3 里程碑回看记录(基线 §19 门)

## Evaluation Record

```text
milestone_id:         M3(统一 Wire API、CLI 与跨平台启动)
build_or_commit_id:   61693c0(CLI)→ ea29cd8(T8 适配)→ 5cf9482(T6a Web)
                      → 8d4633d(T6b Tauri 壳)
test_run_id:          cargo test --workspace(2026-08-29,本机)
                      = 74 passed / 0 failed(HTTP e2e 3 项 + 跨进程 e2e 1 项新增)
log_range:            SSE 流端到端(t31);跨进程事件补发(t32);
                      INV-3 跨重启连续已在 M2 断言,M3 复用
deterministic_checks: validate.py 全绿(合同库 20 个文件,含 surface 2 份新合同);
                      信封逐字节(request_id 回显断言);三平台 CI 矩阵全绿
failure_tests:        t32(硬杀 server 重启:resume active + op 保持 succeeded +
                      ID 防撞 + 无 cancelled);401/404/400 传输矩阵(t30);
                      /shutdown 鉴权 + Notify(t33)
replay_result:        SSE 流含完整事件序列(t31 断言 agent.completed 在场)
llm_evaluation:       不适用(M8.7 起)
known_failures:       见 §6 条件与遗留
architecture_changes: 合同 Minor 增发(surface 传输/鉴权 2 份新合同)+
                      agent.completed/agent.created 载荷增列;无破坏性变更
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-29
```

## §5 逐门记录

- **A 功能测试**:M3.1 Surface Protocol(HTTP 绑定 + SSE)、M3.2 CLI 命令组
  (session/agent/operations/events 全量;task/approval 占位)、M3.3 watch +
  resume cursor(SSE id=seq,断线重连)、M3.4 Tauri 最小界面(窗口直载
  Web Surface)、M3.5 三平台制品(release 工作流)、M3.6 跨平台适配
  (SIGTERM//shutdown/路径/编码核对)——全部有测试或制品实证。
- **B 回归测试**:M1/M2 的全部测试保留且全绿;GT 两场景在 HTTP 形态下不变。
- **C 故障测试**:t32 跨进程硬杀重启(HTTP 形态的 S4);断线重连(SSE
  since 语义);401/404/400 传输错误矩阵;M2 的混沌四项回归全绿。
- **D 日志回放**:事件流可经 SSE 全量重放;Execution Log 与事件流双轨不变。
- **E 确定性评估**:鉴权矩阵、信封回显、ID 防撞均为机器断言。
- **F LLM 评估**:不适用。
- **G 架构复盘**:CLI/Tauri/Web 均为纯客户端,同一 Runtime API——「Surface
  与核心解耦」(基线 §14)首次真实落地;server 零订阅状态(watch=resume
  cursor);无新增事实源。Web 壳不含业务逻辑,不构成第二事实源。
- **H 验收裁决**:passed_with_conditions。
- **I 性能冒烟**:P-01/02/03 复跑全部在劣化门内(P-01/P-03 反而改善;
  P-02 p50 +7.1% 远低于 25% 门)。

## §6 条件与遗留

1. **Tauri msi 安装包未产出**:bundle 配置就位,打包验证随 M8 发行质量
   (当前制品 = 三平台 CLI/server 二进制 + 桌面壳源码构建)。
2. **Web 界面观感简陋**(大白话:能用、不好看):最小三视图仅为验收载体,
   视觉/交互演进随 M8;已记 PENDING。
3. **桌面壳为窗口容器**:未深度集成(如自动拉起 server);server 进程管理
   随 M3.6 后续/M8 打包形态定标。
4. **P-06 RSS**:守护形态已具备,独立采样仍待 M8 前补测(方法论需定标)。
5. **WAL checkpoint 策略**:延续 M2 遗留,随 M8 定标。
6. **回答内容入事件流**(agent.completed.content,Minor):隐私视角为
   本地单用户合理;M8 数据保留期审查时复核。

## §7 回看七问(基线)

1. 解决目标问题?是——Runtime 首次成为可连接的服务,CLI/桌面/Web 三形态
   同源可用;HTTP+鉴权合同落地(ADR-0009 增项兑现)。
2. 旧能力可用?是——M1/M2 全量回归绿;信封/事件逐字节不变。
3. 崩溃/断线/重复执行?硬杀 server 重启实证(t32);SSE 断线重连=
   resume cursor;CLI 退出不取消(结构保证 + 断言)。
4. 日志能解释每一步?能——事件流经 SSE 可全量重放,Execution Log 双轨。
5. 结果被实际核验?回答内容(agent.completed.content)首次可观测,
   收据/状态机/泄漏扫描机器核验。
6. 合同与状态模型稳定?是——仅 Minor 增发(2 份新合同 + 3 处载荷增列)。
7. 推进或退回?推进——进入 M4(Capability、Broker、权限和审批),
   开工前结算 ADR-0001/0002 验收条件。

## 性能复跑(T9,记录②)

```text
P-01 冷启动:  p50=0.090 / p95=0.114 ms(基线 0.100/0.124,改善)
P-03 回合延迟: p50=218.9 / p99=233.2 ms(基线 223.1/236.9,改善)
P-02 resume:  p50=25.47 / p95=25.73 ms(基线 23.78/25.87,p50 +7.1%,门内)
全部在 25% 劣化门内,进入回归监控。
```
