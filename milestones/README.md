# milestones/ — 里程碑规格、回看与台账

> 第 2 层:开工时写实现规格,收官时写回看;W 序列(ADR-0014)验收入规格不另立 review(ADR-0015)。
> 交付时间线看 `HISTORY.md`;未结事项看 `BACKLOG.md`;待用户裁决看 `PENDING.md`。

## M 序列(阶段一 M0-M8 + 阶段二 M9,spec+review 成对)

| 里程碑 | 规格 | 回看 |
|---|---|---|
| M0 范围/合同/测试基线 | (工件在 boenmind-contracts/m0/) | — |
| M1 最小 Runtime 与单 Agent 闭环 | M1-implementation-spec.md | M1-review.md |
| M2 持久化/事件日志/崩溃恢复 | M2-implementation-spec.md | M2-review.md |
| M3 统一 Wire API、CLI、跨平台启动 | M3-implementation-spec.md | M3-review.md |
| M4 Capability/Broker/权限审批 | M4-implementation-spec.md | M4-review.md |
| M5 Butler、Task 和长期监护 | M5-implementation-spec.md | M5-review.md |
| M6 Team、Delegate 和多 Agent 协作 | M6-implementation-spec.md | M6-review.md |
| M7 Provider、MCP 和 App 隔离 | M7-implementation-spec.md | M7-review.md |
| M8 首批真实 App 与发行质量 | M8-implementation-spec.md | M8-review.md |
| M9 阶段二第一批(抽屉授权/真流式/自主环) | M9-implementation-spec.md | M9-review.md |

## W 序列(WebUI,ADR-0014;验收入规格)

| 批次 | 规格 | 验收记录 |
|---|---|---|
| W1 壳+OpenAI 插座流式 | W1-implementation-spec.md | 规格 §(真浏览器实测) |
| W2 设置中心/工作区/可拖布局 | W2-implementation-spec.md | 规格 §7 + shots-w2/ |
| W3 两级主题系统 | W3-implementation-spec.md | 规格 §6 + shots-w3/ |
| W4 对话工具闭环+角色 | W4-implementation-spec.md | 待回填(见 BACKLOG §3) |

## 台账与横切文件

- `HISTORY.md` — 交付时间线(append-only,唯一进度真源);
- `BACKLOG.md` — 未结事项总台账(掉链项/后置项/前置项/审计 F 系/文档欠账);
- `PENDING.md` — 待用户裁决队列(2026-09-01 起清零,11 条历史裁决见表);
- `AUDIT-2026-08-30.md` — 审计台账(只记录不修改;A/R/F 系条目);
- `FULL-REVIEW-2026-08-30.md` — M1-M9 全面回看(ADR 兑现度、S1-S10 总表 §2.4、遗留 72 条总账 §4);
- `M2-adr-settlement.md`、`M4-adr-settlement.md` — 开工前 ADR 条件结算表(已闭合);
- `W-ui-inventory.md` — assistant-ui 官方资产盘点(W 序列选装参考);
- `PENPOT-quickstart.md` — Penpot 上手(⚠ 2026-09-01 标注:令牌对接路径已随 dsh 前端失效);
- `shots-w2/`、`shots-w3/` — 验收截图存档。

## 惯例

1. 里程碑范围定义与通过条件的规范文本在基线 §18;本目录规格是落地细化。
2. 收官动作:全量测试+validate.py 全绿 → 回看门(基线 §19)→ HISTORY 加行 → BACKLOG 登记遗留 → git tag。
3. 规格与回看是历史记录,不回头改写(状态行除外);新事实进台账或 ADR。
