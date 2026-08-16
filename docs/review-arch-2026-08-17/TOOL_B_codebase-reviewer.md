# 工具 B · codebase-reviewer 独立报告（架构文件六维）

> 2026-08-17。硬排除 WIKI。未读 A/C。对象 = 五份活架构文档 + 抽样对照代码。不写 QUESTIONS.md、不改库。

**Verdict：** 活文档停在 2026-08-16 皮肤轮口头当前态，代码已走到 14 协议面 + 运行期 mcp + 配置单锁 + 设置五阶段 + 聊天排队。主伤是状态行 / HANDOFF / 登记表不同步，不是缺一篇新架构论文。

| 维 | 分 |
|---|---|
| ARCH | 4.5 |
| SEC | 5.0 |
| PERF | 6.0 |
| QUAL | 3.5 |
| BUG | 4.0 |
| IMP | 4.0 |
| **均分** | **4.5** |

---

## Architecture

- **ARCH-001 High** 主文档状态行/§7.2「13 面」vs 代码 14 协议面 + 运行期 mcp。改与登记表同一口径。
- **ARCH-002 High** HANDOFF 第 10 行写第 14 面，第 12/99 行又写 13 面且漏 provider。
- **ARCH-003 High** `register_port("mcp")` 存在；登记表与 `bm-protocol` 不认。补行，契约在 bm-mcp。
- **ARCH-004 High** §15.1「内核未接线」与同文 §7.2/代码相反。标当时态，勿删历史。
- **ARCH-005 High** 桌面壳：主文档双 DE / 设置文已删 / HANDOFF 仍完成态。三份改「代码已退役，开关占位」。
- **ARCH-006 High** 配置单锁只写在 08-17 交叉审查。§5.5 + HANDOFF 补当前态。
- **ARCH-007 Medium** 章节十一→十三→十五→十二附录；3.4 错位。附录改名或加「无独立 §十二正文」。
- **ARCH-008 Medium** 「唯一事实源」与 sidecar 标注并存。术语表同步 §5.1。
- **ARCH-009 Medium** §5.3 五个扩展点 / HANDOFF 10 / 代码与登记表 12。
- **ARCH-010 Medium** §14.1 称 Steward 走 `enqueue_turn`；生产是 `dispatch_steward_round`。
- **ARCH-011 Medium** 分层图把 Chat/Coding 画成应用插件；§6.8 已承认宿主组件。图下加当前态一行。
- **ARCH-012 Low** README/HANDOFF 钉 v0.24，主文档 v0.25。

## Security

- **SEC-001 High** §5.4 现在时写 GateChain/哈希链/taint/配额；代码只有询问链+门闩。每条加已落地/未做。
- **SEC-002 High** 08-17 CSRF 头 / 工作区白名单 / GET 掩码 / CredentialsPort 取 key 未回写活文档。
- **SEC-003 Medium** 设置文 KEY 落盘未写 0o600 / GET 掩码 / 单锁。
- **SEC-004 Medium** credentials 登记待接线，引擎已 lookup。

## Performance

- **PERF-001 Medium** 「6060 行」过期（抽样约 6.6k）。改约数或只留预算。
- **PERF-002 Medium** EventQuery 类型过滤已落地，§11.3 N+1/O(n²) 仍像待修。
- **PERF-003 Low** TokenRing 写死 128K 参考窗，文档未记载。
- **PERF-004 Low** checkpoint「每请求 fsync」现在时，应标预留。

## Quality

- **QUAL-001 Medium** 状态行承担整部版本史。只留版本+3 条当前态。
- **QUAL-002 High** HANDOFF 第 16 行「M2 下一件=分支图」后文又写已完成。
- **QUAL-003 High** 设置文阶段 1–5 ✅ 后再复制 2–6 ⬜。删损坏行。
- **QUAL-004 Medium** §6.9「现状检查」写提示词硬编码、记忆零实现，与上行表格打架。
- **QUAL-005 Low** 借鉴清单 3.4 错位。
- **QUAL-006 Medium** README 缺设置文 / MCP 计划 / 08-17 交叉审查。
- **QUAL-007 Low** 登记表无「最后核对」日期。

## Bugs

- **BUG-001/002 High** M2 状态：主文档 ⏳ + skill 混进原验收句；HANDOFF 自相矛盾。
- **BUG-003 High** 活链接指向已归档路径（ACCEPTANCE_M1、HANDOFF_DESKTOP_SHELL、REVIEW_ARCHITECTURE、kernel-implementation-plan）。
- **BUG-004 Medium** §11.3 阶段 0 行号清单读起来像现在还炸。
- **BUG-005 Medium** scheduler 已消费（set_wake）。
- **BUG-006 Medium** 「接线完毕」未定义：注册 ≠ 消费中。
- **BUG-007 Medium** 引擎按 app 过滤已做；skill 系统提示按场景注入未做。须拆开。
- **BUG-008 Low** §7.1「热升级/桌面壳/验签」捆在一起，桌面壳已假。

## Improvements

- **IMP-001 High** 缺设置中心 / bm-mcp / 配置锁 / 聊天排队专节（链出去，勿再展开 20 页）。
- **IMP-002 Medium** 登记表补皮肤、settingsSchema、skill settings.json、mcp 发现链。
- **IMP-003 Medium** 设置文与主文档互链。
- **IMP-004 Medium** 设置轮 7 条已拍未进 HANDOFF 待拍板。
- **IMP-005 Medium** MCP 计划 4c OAuth 后置会丢。
- **IMP-006 Low** §十「内核未接线」项缩句+改 archive 链。
- **IMP-007 High** README 文首加三份活文档对照代码日期。
- **IMP-008 Low** 引用 SERVICE_FACES 须声明「止于 13 面」。
