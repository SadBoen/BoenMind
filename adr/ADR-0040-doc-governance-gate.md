---
status: accepted
date: 2026-09-12
summary: 整并取代 0015/0026/0027;ADR 状态机与取代双向一致入 CI;可提交面 .md 收归白名单;治理门自带自检防静默摘除(2026-09-12)
supersedes: [ADR-0015, ADR-0026, ADR-0027]
superseded_by: []
---

# ADR-0040: 文档治理以机器门固化——ADR 状态机 + 追踪白名单 
- 关联: 整并取代 ADR-0015(文档体系整理)、ADR-0026(规范与叙事分离)、ADR-0027(文档极简纪律);承接三者仍有效的原则,将其执行从"纸面规则"升级为"CI 硬门" - 背景: ),每套都删掉上一套钦定的文件;而 `.github/workflows/ci.yml` 六个 job 无一条检查文档——唯一被机器检查的文档内容仅 AGENTS.md 的一行版本号(A-26 诊断见 `.ai/context.md`)。根因:治理规则全靠自觉,无机器门,故必然被下一套推翻。 
## 决策 
1. **四类信息、四种介质、各自唯一权威**(同一事实只在一处维护): 
 | 信息类 | 回答 | 权威介质 | 生命周期 |  |---|---|---|---|  | 规范 | 现在什么是真的 | 基线/合同/ADR/各 README | 就地修订,零编年史 |  | 决策 | 为什么这么做、哪条算数 | `adr/` + **强制状态机** | proposed→accepted→superseded→归档 |  | 进行中 | 还有什么没做 | GitHub Issues(一条一件事) | open→closed,结论熔回上层 |  | 历史/证据 | 发生过什么 | git(tag + 提交说明) | 只增;不另立时间线文件 | 
2. **ADR 状态机(机器强制)**:每个 `adr/ADR-*.md` 头部带 front-matter `status`/`date`/`supersedes`/`superseded_by`;`status` 限 proposed/accepted/superseded/withdrawn;取代关系必须双向一致;终态必须移入 `adr/archive/`。**只有 accepted 且未被取代的 ADR 视为现行。** 
3. **追踪白名单(机器强制)**:`docs/doc-whitelist.txt` 是仓库文档的准入清单;任何未列其中、且可提交的 `.md` 一律 CI 红灯。新增文档须先登记——**文档增长成为一次显式决策,而非默认行为**。 
4. **治理冻结**:本 ADR 是文档治理的唯一权威。改动治理本身须发新 ADR;`scripts/doc_gate.py` 内置自检——`ci.yml` 必须仍含 `doc-gate` job、`ADR-0040` 必须存在且 accepted,**故治理门无法被静默摘除。** 
5. **派生文档生成(单一来源)**:凡可由机器从单一来源推导的内容一律生成,不手写。首个落地项 = `adr/README.md` 的索引表,由 `scripts/gen_adr_index.py` 从各 ADR 的 front-matter(status/date/supersedes/superseded_by/summary)、标题行与「条件与验收」段推导;CI 校验新鲜度,手改即红灯。新增 ADR 只需带 front-matter 与 `summary:`,索引自动更新。 
6. **承接仍有效的原则**(自 0026/0027):规范文档零编年史;过程文档(实现规格/回看/评审报告/转录)交付即删,原文溯 git;交付全史=git;去重单源。 
## 后果 
- **可证伪验收(30 天后自查)**:① `doc-gate` job 仍在跑且为绿;② 白名单外零新增 `.md`;③ 无两条互相矛盾的 ADR 并列为现行。 - **反向判据**:若该工作流被静默禁用、或白名单被绕过——即判本次治理与前四套同源失败,停用本 ADR。 - **净文档量约束**:本次改造受追踪 `.md` 数量须低于改造前的 75。 - 本 ADR 属流程纪律,无基线正文熔入项;`docs/architecture/decisions.md` 仅收"架构铁律"条目,文档治理纪律以本 ADR 为准。 