# W9 上下文轨迹(会话记录)实现规格

来源:2026-09-03 用户裁决「加强日志工具,去看 dsh 的聊天记录」;调研=deepseek-harness
会话子系统(session.md/persistence.md/ui-trajectory,报告已归档对话)。
对标:DSH 事件溯源会话日志 + ui-trajectory 轨迹视图。

## 一期(本批)

1. **context-log 升级为逐轮事件流**(bm-core/src/context_log.rs):每回合记录
   `request`(system prompt+工具清单+消息数,既有)、新增:
   - `tool_call` {capability, args}(工具轮每次调用);
   - `tool_result` {capability, result 原文/错误, elapsed_ms}(回喂模型的原文);
   - `assistant_final` {content, usage_in, usage_out}(终稿与用量);
   - `turn_end` {outcome: succeeded|failed|cancelled, error_code, latency_ms}。
   兼容:既有行不改(append-only),新事件走同一 jsonl 同一读取面。
2. **「上下文」页签升级为轨迹视图**(webapp w2 ContextPanel):按 turn 分组
   时间线;每条记录点开巡检(工具回喂原文、用量、耗时、失败码);失败回合
   红标。接口:/admin/context 增返回新事件类型(不破坏既有字段)。
3. 验收:真模型对话含工具轮,轨迹页可回放「调用→原文→终稿」全链;回归全绿。

## 二期(候补,未排期)

跨会话全文检索(SQLite FTS5,复用 M5 经验);三期末:会话分叉/崩溃恢复补账
(合成 turn_end{interrupted})。

## 不做

不改合同(事件流=壳子私用管理面,沿 W2 口径不入冻结库);不搬 DSH 的
Zstd/checksum(个人单机,明文 JSONL 即可);projection/cache 体系(量级不需要)。
