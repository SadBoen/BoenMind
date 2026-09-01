# 未结事项总台账(BACKLOG)

> 定位:全仓「还欠什么」的**唯一入口**(ADR-0015 确立)。各里程碑回看产生的遗留条件,在回看完成时登记到本表;历史明细仍在各回看/审计文件里可溯,本表只做汇总与状态。
> 状态口径:OPEN=未动工 / DEFERRED=用户拍板后置 / INPROGRESS / DONE(留出处)。
> 维护:每轮回看收官时增改;完成一条就把状态改 DONE 并写出处,不删行。

## 1. 掉链项(前序回看写明「随某里程碑补」,但该里程碑未接住;来源 FULL-REVIEW §4)

| 条目 | 来源 | 状态 |
|---|---|---|
| WAL checkpoint 策略定标 | M2-review §6-3 → M3-review §6-5(称随 M8,M8 未接) | OPEN |
| bus.resumed 发射点 | M4-review §6-2 | OPEN |
| CLI capability list 命令 | M4-review §6-2 | OPEN |
| Orchestrator 单进程等价映射复测(独立进程形态) | M5-review §6-1 | OPEN |
| Task 级停滞窗口/并发上限配置字段 | M6-review §6-1(M5 部分闭合) | OPEN |
| 成员级 max_concurrent_tools 定标 | M6-review §6-1 | OPEN |
| 多代 stdio 进度聚合 | M7-review §6-5 | OPEN |
| capability 操作不落 operations 表复核 | M4-review §6-4 | OPEN |
| P-06 RSS 独立采样接入 perf 套件 | M1 §6-2 → M2 → M3 §6-4 后无接续 | OPEN |

## 2. 用户拍板后置(DEFERRED,裁决记录见 milestones/PENDING.md)

| 条目 | 来源 | 状态 |
|---|---|---|
| S4 draining 两步摘除实测 | M7 §6-1 → M8 §6-4 → M9 §6 | DEFERRED(留档后置) |
| lease 通道真实吞吐实测 | M4 §6-1 → M8 §6-4 | DEFERRED(留档后置) |
| 远程 MCP(HTTP/SSE 接入) | M7 §6-3 → M9 §6-3 | DEFERRED(先真实使用一周再开工,方向不变) |
| 桌面安装包(`cargo tauri build` 出 .exe) | M3 §6-1 → M8 §6-1(D-M8-3) | DEFERRED(搁置不排期,骨架在 shell/tauri) |
| memory 条目级删除所有权 | M9-review §6-2 | DEFERRED(演进项) |

## 3. 下一批开工前置与 W4b

| 条目 | 来源 | 状态 |
|---|---|---|
| F-06 C4 模型回写(约 12+7 项模型-实现漂移;F-08 /admin 面一并) | FULL-REVIEW §2.2 + AUDIT F-06/F-08 | OPEN,**下一批开工前置** |
| F-09 deepwiki S1/S2/S6/S7/S10 逐条裁决;S5 口径偏宽复核(t119b≠quarantined 分表) | AUDIT F-09 + FULL-REVIEW §2.4 | OPEN(随下一里程碑回看) |
| W4b:异步能力结果回流超时(60s 轮询未取到终态)——**根因已修**:turn.rs GetOpResult 轮询 `tx.send` 漏 `.await` 致命令从未发出(2026-09-01 代码审计轮),待真浏览器实测验证后闭合 | W4 规格 §3 + 代码审计轮 | 已修待验证 |
| W4b:多角色与会话级角色选择;Skill 挂载(合同 Skill 实体未建) | W4 规格 §3 | OPEN |
| W4b:非直通工具的对话内审批联动 | W4 规格 §3 | OPEN |
| W4 验收记录回填(按规格 §4 验收门补实测证据与截图) | W4-implementation-spec | OPEN |

## 4. 审计台账 F 系(明细与处置见 milestones/AUDIT-2026-08-30.md,均登记待批)

| 条目 | 一句话 | 状态 |
|---|---|---|
| F-01 (P1) | exec_log I/O expect 可 panic | OPEN |
| F-02 | 投影写 `let _=` 静默失败,待加失败日志钩子 | OPEN |
| F-03 | glm_http SECRET_BRIDGE 死代码(删或接线) | OPEN |
| F-04 | 位点 meta 损坏静默按 0 处理 | OPEN |
| F-05 | 200+ 行函数重构债(与 L-01/R-08 同批) | OPEN(缓办) |
| F-07 | bm-surface-http → bm-persist 直依赖待裁决(收口或留档) | OPEN |
| F-10 | autorun send_failed 等测试缺口 | OPEN |
| F-11 | memory_drawer_verdict 硬编码权限规则与 ADR-0006 张力(2026-09-02 已在 broker.rs 源码补注;合同化重构待排期) | OPEN |
| P3 大文件拆分 | broker.rs(1657 行)/turn.rs(1694)/task_ops.rs(1710)/sqlite_state.rs(1205);bm-core 过重,与 F-05/L-01/R-08 同批缓办(2026-09-01 代码审计轮);broker 建议拆法(2026-09-02 审计第二轮)=mod+policy(GrantLedger)/credential/executor/audit | OPEN(缓办) |
| P4 非测试 unwrap 甄别清理 | 全仓约 400 处 unwrap 需区分测试/非测试逐步替换;非测试 panic 10 处均系不变量断言,评估=维持现状(同审计轮) | OPEN(缓办) |
| P5 Capability 抽象演进 | ①同步 `CapabilityProvider::invoke` 无超时护栏(长阻塞会占住单写者;当前内置能力均快路径未爆,已在 trait 注释写明选型约束)②invoke 错误 `String`→结构化枚举(与 wire ErrorCode 信封对齐)③与 AsyncCapabilityExecutor 分层已注释(2026-09-02 审计第二轮);统一单 async trait 的评估留 M 系列回看 | OPEN(缓办) |

## 5. 文档与工程欠账(2026-09-01 文档对齐轮登记,ADR-0015)

| 条目 | 说明 | 状态 |
|---|---|---|
| api_dsh.rs 删除待追认 | 工作区已删 `bm-surface-http/src/api_dsh.rs`(dsh 宿主协议端点 /api/*),与 ADR-0013「后端保留 api_dsh 适配器」口径相抵;提交该删除时需用户追认并补记(下一条 ADR 或随回看) | OPEN |
| MCP 示例配置启动命令不实 | `apps/mcp-config.example.json` 的 web_multisearch 原写 `python -m boenmind_mcp_servers.web_multisearch`(模块任何环境不可 import);2026-09-02 已改为 Rust 版 exe + `--config` 真实形态 | DONE(2026-09-02) |
| release.yml 承诺未兑现 | `.github/workflows/release.yml` 注释称 Tauri Windows 安装包随 T6/M8 加入工作流,实际从未加入(或兑现或改注释) | OPEN |
| webapp 无自动化测试 | 前端现状=真实浏览器手测+截图留档(纪律见 PLAYBOOK);候选:playwright 冒烟套件 | OPEN(候选) |
| 坏 MCP 条目导致启动拒绝 | 一条损坏的 MCP 配置会拒绝整个 server 启动(用户反馈轮发现);应降级为跳过+告警 | OPEN |
| fmt 门禁缺口:lib.rs | W 系列提交均未跑 `cargo fmt --check`(CI fmt 门禁红);2026-09-01 已全仓规范化,唯 lib.rs(HEAD 版本即不干净)留待 api_dsh 追认提交时一并入库闭合 | OPEN(随追认闭合) |
| web_multisearch Rust 版切换决策 | **已切换现役(2026-09-02)**:exe 安装于 `<数据目录>\mcp\`,经两段式(扫描→批准→重载)上线,探活绿、真模型对话实测工具闭环全通(注入/调用/结果回流/错误降级);剩 ddgs 指纹挑战与 Marginalia 公共 key 限流为外部状态(用户裁定当前不修),想恢复全源覆盖 = 设置页 MCP 配置表单填一把免费 Jina Key(热生效)或自建 searxng 填地址 | DONE(外部源覆盖随 Key) |
| PENPOT-quickstart 过时 | 指向已删除的 runtime/web/tokens.css;已加过时标注(2026-09-01),待归档或更新 | OPEN(低优) |

## 6. 候选队列(用户提过、未排期)

- 使用反馈轮(按「先真实使用一周」节奏收集);
- 远程 MCP(见 §2,使用期后开工)。

## 7. 已闭合备忘(留出处,勿重开)

- 合同 Minor 三笔:tools maxItems 0→16 / finish_reason 加 tool_calls / invoke_response 加 tool_calls(commit 40988a7,W4)。
- PENDING.md 的 11 条 D-M 系列 user 裁决全部闭合(见 milestones/PENDING.md 表)。
- S5(S4 部分实现)、S9(verification 三分法,M5/M7)裁决见 FULL-REVIEW §2.4。
