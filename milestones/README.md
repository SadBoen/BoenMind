# milestones/ — 批次工件

> 第 2 层。文档极简纪律(ADR-0027,2026-09-08):本目录只存导航与验收截图;实现规格=开工时写的临时工件,收官回看结论入台账后即删;交付全史=git(tag+提交说明),本目录不存时间线。
> 未结事项看 GitHub Issues (`gh issue list`);架构铁律查重看 `docs/architecture/decisions.md`。

## 文件清单

- `W-ui-inventory.md` — assistant-ui 官方资产盘点(W 序列选装参考,唯一保留的参考件);
- `shots-*/` — 各批验收截图存档(证据,非文档)。

## 惯例

1. 里程碑范围定义与通过条件的规范文本在基线 §18;开工写《批次实现规格》(技术栈/拆分/验收门),属临时工件;
2. 收官动作:全量测试+validate.py 全绿 → 回看门(基线 §19)→ 结论入 decisions.md(裁决)/GitHub Issues(遗留)/ADR(架构决策)→ 删规格与回看文件 → git tag+提交说明记交付(ADR-0027);
3. 规格与回看文件不返修、不归档、不留版本:历史真相在 git 提交史;
4. 外部评审/审计/复盘类一次性报告不入库(2026-09-07 用户令):复核结论写 decisions.md/GitHub Issues,原文溯 git 史。
