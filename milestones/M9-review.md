# M9 回看——阶段二第一批(2026-08-30)

- 里程碑:M9(阶段二第一批:记忆抽屉授权 × 模型真流式 × worker 自主环 v0)
- 规格:`M9-implementation-spec.md`(2026-08-30 冻结)
- tag:`m9-stage2-batch1`
- 结论:**通过(passed_with_conditions)**;条件与遗留见 §6。

## §1 验收记录

```text
date:             2026-08-30
tag:              m9-stage2-batch1
commits:          规格 7b895c9 / S1 fb73c8c / S2 10115fe / S3 8555363
validate:         全绿(R2-R4;45 事件 × 方法枚举 × task schema 同步)
tests:            254 passed / 0 failed(235 存量 + 19 新增)
clippy:           --all-targets 零警告
fmt:              --check 通过
live:             t144 实网流式 1 次调用通过(全批实网调用 = 2:
                  1 次 curl 线格式取证 + 1 次 t144;均 max_tokens 受限)
```

## §2 三轨落地实录

| 轨 | 交付 | 验收测试 |
|---|---|---|
| S1 记忆抽屉授权 | Broker 步 4.5 主体边界(agent/task 自抽屉常量放行、search 对 user 抽屉放宽、越界一律升级审批)+ 审批签发捕获 scope 谓词(批准只覆盖被批准抽屉)+ grant.created 载荷补 resource(合同双侧同步) | broker 单测 ×7 + t130/t131+132/t133/t135 |
| S2 模型真流式 | 合同 `model.content.delta`(Minor)+ `invoke_stream` 默认退化实现(旧连接器零改动)+ OpenAI SSE 覆写(字节缓冲整行解码防劈字、损坏行容错、中断聚合如实标记 stream_interrupted)+ RuntimeConfig.model_streaming(默认关零回归)+ BOEN_MODEL_STREAM 开关 + web UI 增量渲染(textContent 防 XSS) | t140/t141/t142/t143 + t144 实网 |
| S3 worker 自主环 v0 | `task.autorun`(方法枚举/参数结果/事件三处合同同步)+ 事件驱动状态机:受理 → 专属会话回合 → TurnEvent 回账 → pump 裁决(继续/哨兵完成/停滞/超限/外部暂停) | t150/t152/t153/t154 |

## §3 前置结算

- **D-M5-2(memory:user 显式授权执行面)**:闭合——agent 写 user 抽屉
  升级审批(t131),批准签发带 scope 谓词的 Grant(t132),PENDING 条目随
  本回看正式关闭。
- **M8-review §6-4 六项遗留**:本批了结两项(worker 自主环 v0、模型流式);
  其余四项(S4 draining、lease 吞吐、memory 按主体授权的**条目级所有权**、
  远程 MCP HTTP/SSE)留 §6(用户拍板:远程 MCP 下一批;桌面壳先不搞,
  web 版调整优先)。

## §4 规格偏差(如实留档)

1. **自抽屉授权改为常量规则直通**(规格原文:agent 创建物化默认抽屉
   Grant)——避免每 agent 三枚 Grant 事件破坏「默认配置 golden trace 零
   变化」承诺;越界持久授权仍走 Grant(审批签发带谓词)。语义等价、事件
   面更省。
2. **t151(worker 工具预算)改口**:max_tool_calls 硬限作用于
   worker_capability_call 路径,自主环 v0 的模型回合由 max_turns(默认 6)
   与 agent token 预算约束;工具预算并入自主环随真工具闭环(tools 合同)
   演进。
3. **完成哨兵 `[[AUTORUN_DONE]]`**:模型面无工具调用(M7 起合同即二值
   收敛),v0 以哨兵作完成声明;提交报告走 M5 证据门——**未经核验的完成
   声明转 blocked(outcome_unknown)等用户验收**(t150 实证,架构纪律优先)。

## §5 回看七问(基线)

1. **计划与实际的偏差?** 三轨全落地;偏差即 §4 三条,均为实现期收敛且
   有测试实证。
2. **哪些是临时绕路?** 无。哨兵完成是 v0 明确边界(真工具闭环需模型
   tools 合同,属后续里程碑)。
3. **合同是否被破坏?** 否。三处全部 Minor 纯追加(2 事件 × 1 方法 ×
   grant.created 载荷键 resource),validate.py 全绿,sync 对账通过;
   默认配置下存量 golden trace 零变化(235 存量测试全绿为证)。
4. **性能是否触门?** 复跑结果见 perf 记录⑦(S2/S3 面默认关或旁路,
   热路径预期零改动)。
5. **安全边界是否松动?** 否。抽屉越界一律升级审批(可听声);流式增量
   经单写者落事件(无旁路);SSE 错误分支零响应体零凭明(INV-5 延续);
   web 渲染 textContent(XSS 纪律)。
6. **下一个里程碑最需要什么?** 按既定策略:真实使用一周攒手感;候选
   队列 = 远程 MCP(用户已拍板下一批)、自主环真工具闭环、web 版调整
   (用户指定优先于桌面壳)。
7. **如果重做会怎样?** 会把 S3 的完成哨兵直接设计成「报告+验收」两段
   (而非先当 completed 预期再修正);其余不变。

## §6 条件与遗留

1. **模型 tools 合同**:自主环接真工具调用需连接器 tools 参数与合同
   增量(当前二值收敛)——列阶段二后续。
2. **memory 条目级所有权**:memory.delete 按 entry_id 定位、args 不含
   scope,主体维度执行面未覆盖删除——列演进(条目所有者列)。
3. **远程 MCP(HTTP/SSE 传输)**:用户拍板下一批。
4. **桌面壳**:用户裁决先不搞;web 版调整优先(D-M3-1/D-M8-3)。
5. **autorun 的 worker 工具预算**(§4-2):随 tools 合同一并定标。
