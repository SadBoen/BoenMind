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
| ~~F-06 C4 模型回写(约 12+7 项模型-实现漂移;F-08 /admin 面一并)~~ **已回写(2026-09-02)**:boenmind.c4 回写完成——超前容器标注阶段归属(阶段一现役/阶段二规划/阶段一并入内核)、App=MCP 真实形态(Market/Wiki 标注 stdio MCP)、Web Surface 标注现役 assistant-ui 壳+管理面、补齐阶段一内核组件(watchdog/contextLog/bm-judge 核验/内建模型连接器);硬纪律 2 闭合 | FULL-REVIEW §2.2 + AUDIT F-06/F-08 | DONE(2026-09-02) |
| F-09 deepwiki S1/S2/S6/S7/S10 逐条裁决;S5 口径偏宽复核(t119b≠quarantined 分表) | AUDIT F-09 + FULL-REVIEW §2.4 | OPEN(随下一里程碑回看) |
| W4b:异步能力结果回流超时(60s 轮询未取到终态)——根因已修(turn.rs GetOpResult 轮询 `tx.send` 漏 `.await`,2026-09-01 代码审计轮);**已闭合**:2026-09-02 上下文透视面板真模型实测工具轮两步流水全通(MCP 工具调用→结果回喂→模型重调,快照与趋势可见) | W4 规格 §3 + 代码审计轮 | DONE(2026-09-02) |
| ~~W4b:多角色与会话级角色选择;Skill 挂载(合同 Skill 实体未建)~~ **全部交付(2026-09-02)**:①多角色 CRUD+会话级切换(详见上批);②Skill 挂载=合同 capability/skill.v0_1 新增(Minor)+ config/skills.json + /admin/skills CRUD + 角色编辑挂载勾选 + bm-core::roles::compose_role_prompt 统一组装(角色 prompt=基底+挂载技能指令,会话创建烤入/回合热读双口径一致);真模型实测「押韵诗人」技能挂载后回复严格遵循打油诗格式,透视面板核验技能指令已注入 system prompt(截图 shots-w5-context/08) | W4 规格 §3 | DONE(2026-09-02) |
| ~~W4b:非直通工具的对话内审批联动~~ **已交付(2026-09-02)**:registry.chat_tools() 按 Broker 同口径(effect 可审批类或声明 required)暴露全部对话能力;turn.rs 审批等待流(CapabilityCall 返回 ApprovalRequired → 反查审批单 → BM_APPROVAL 标记随 SSE 上屏 → 轮询裁决+执行 300s);/admin/approvals/{id}/respond 免鉴权裁决端点;前端审批卡片(工具名+真实参数+批准/拒绝);顺带修三个真 bug:①幂等键纯 tc.id 跨回合碰撞致模型收到旧收据反复重调(改含 op_id);②内置工具 input_schema 空 schema 模型无法传参(补 properties);③工具结果回喂无引导致模型重复调用(加明确指令);真模型实测笔记写入审批全闭环(截图 shots-w5-context/06、07) | W4 规格 §3 | DONE(2026-09-02) |
| W4 验收记录回填(按规格 §4 验收门补实测证据与截图) | W4-implementation-spec | OPEN |
| ~~会话历史未回喂模型(多轮无记忆,P1 疑似)~~ **已确认并修复**:turn.rs 每回合 messages 从零组装属实;W5 修复=World.session_chats 台账(20 轮/24K 字符上限)+ spawn 时历史回喂 + 成功落定 Cmd::RememberTurn 回写;真模型实测两轮暗号往返+跨页面重载均答中(HISTORY W5 行) | W1 §4 vs turn.rs 代码对照 | DONE(2026-09-02) |

## 4. 审计台账 F 系(明细与处置见 milestones/AUDIT-2026-08-30.md,均登记待批)

| 条目 | 一句话 | 状态 |
|---|---|---|
| ~~F-01 (P1)~~ | ~~exec_log I/O expect 可 panic~~ **已修(2026-09-02)**:打开/追加/flush/重写四处全部降级为 stderr 告警+内存镜像兜底,外部条件不再 panic | DONE(2026-09-02) |
| ~~F-02~~ | ~~投影写 `let _=` 静默失败~~ **已修(2026-09-02)**:save_idem_receipt 两处落表失败改 stderr 告警(重启后幂等抑制失效可被察觉) | DONE(2026-09-02) |
| ~~F-03~~ | ~~glm_http SECRET_BRIDGE 死代码~~ **已修(2026-09-02)**:删除静态桥,凭据改构造注入(与 OpenAiConnector 同口径),GLM 连接器从"运行必败"变可用 | DONE(2026-09-02) |
| ~~F-04~~ | ~~位点 meta 损坏静默按 0 处理~~ **已修(2026-09-02)**:统一 parse_meta_seq 解析助手,损坏值 stderr 告警后按缺失兜底(语义不变,损坏事实可见) | DONE(2026-09-02) |
| F-05 | 200+ 行函数重构债(与 L-01/R-08 同批) | OPEN(缓办) |
| F-07 | bm-surface-http → bm-persist 直依赖待裁决(收口或留档) | OPEN |
| ~~F-10~~ | ~~autorun send_failed 等测试缺口~~ **已补(2026-09-02)**:t155 自主环 send_failed 收口验收(会话关闭→INV-6 回合不取消→续推失败→finished/send_failed)+ exec_log 复扫降格分支验收(凭据=脱敏标记自替换构造) | DONE(2026-09-02) |
| F-11 | memory_drawer_verdict 硬编码权限规则与 ADR-0006 张力(2026-09-02 已在 broker.rs 源码补注;合同化重构待排期) | OPEN |
| P3 大文件拆分 | broker.rs(1657 行)/turn.rs(1694)/task_ops.rs(1710)/sqlite_state.rs(1205);bm-core 过重,与 F-05/L-01/R-08 同批缓办(2026-09-01 代码审计轮);broker 建议拆法(2026-09-02 审计第二轮)=mod+policy(GrantLedger)/credential/executor/audit | OPEN(缓办) |
| P4 非测试 unwrap 甄别清理 | 全仓约 400 处 unwrap 需区分测试/非测试逐步替换;非测试 panic 10 处均系不变量断言,评估=维持现状(同审计轮) | OPEN(缓办) |
| P5 Capability 抽象演进 | ①同步 `CapabilityProvider::invoke` 无超时护栏(长阻塞会占住单写者;当前内置能力均快路径未爆,已在 trait 注释写明选型约束)②invoke 错误 `String`→结构化枚举(与 wire ErrorCode 信封对齐)③与 AsyncCapabilityExecutor 分层已注释(2026-09-02 审计第二轮);统一单 async trait 的评估留 M 系列回看 | OPEN(缓办) |

## 5. 文档与工程欠账(2026-09-01 文档对齐轮登记,ADR-0015)

| 条目 | 说明 | 状态 |
|---|---|---|
| ~~api_dsh.rs 删除待追认~~ **已追认删除(2026-09-02)**:用户同意按计划推进;dsh 协议端点 `/api/*` 正式移除(前端已弃用 ADR-0013,归档分支保留历史),bm-surface-http/src/lib.rs 同步清理 | DONE(2026-09-02) |
| MCP 示例配置启动命令不实 | `apps/mcp-config.example.json` 的 web_multisearch 原写 `python -m boenmind_mcp_servers.web_multisearch`(模块任何环境不可 import);2026-09-02 已改为 Rust 版 exe + `--config` 真实形态 | DONE(2026-09-02) |
| ~~release.yml 承诺未兑现~~ **已兑现(改线)**:2026-09-02 用户裁决发布收敛为 Linux x86_64(VPS 安装场景,Tauri 桌面包承诺随之废止,桌面包本就 DEFERRED);release.yml 重写为 v* tag 触发的真实发布线=服务器+官方插件 web-multisearch 双制品+预构建前端 dist 同包+自动建 GitHub Release;v0.0.1 首发 | 文档对齐轮登记 + 2026-09-02 发布轮 | DONE(2026-09-02) |
| 启动流式开关漂移:模型流式只认 env `BOEN_MODEL_STREAM=1`,config/model.json 的 `"stream": true` 不生效(config_store 已解析该字段但 boenmind-server.rs RuntimeConfig 组装处只读 env;2026-09-02 重启服务实测发现) | 2026-09-02 重启轮 | OPEN |
| ~~webapp 无自动化测试~~ **已建冒烟套件(2026-09-02)**:runtime/webapp/e2e/smoke.spec.ts(6 用例:对话闭环/新建清空/上下文页渲染与逐项浏览器/角色列表/插件页去重断言/日志页签);网络层 mock(/v1 SSE + /admin)确定性零 token;`npm run test:smoke` 全绿 4.4s;CI 接入与真实模型路径覆盖留候选 | DONE(2026-09-02,CI 接入留候选) |
| ~~坏 MCP 条目导致启动拒绝~~ **已修复(2026-09-02)**:load_mcp_setups 条目级容错(单条合同校验/env 解析失败跳过并告警,不再整体失败);boenmind-server 启动循环 spawn/握手失败隔离跳过;测试 t107 更新为「坏条目跳过+坏好混合只留好条目」新语义 | DONE(2026-09-02) |
| ~~fmt 门禁缺口:lib.rs~~ **已闭合(2026-09-02)**:随 api_dsh 追认提交一并 cargo fmt 全量规范化 | DONE(2026-09-02) |
| web_multisearch Rust 版切换决策 | **已切换现役(2026-09-02)**:exe 安装于 `<数据目录>\mcp\`,经两段式(扫描→批准→重载)上线,探活绿、真模型对话实测工具闭环全通(注入/调用/结果回流/错误降级);剩 ddgs 指纹挑战与 Marginalia 公共 key 限流为外部状态(用户裁定当前不修),想恢复全源覆盖 = 设置页 MCP 配置表单填一把免费 Jina Key(热生效)或自建 searxng 填地址 | DONE(外部源覆盖随 Key) |
| PENPOT-quickstart 过时 | 指向已删除的 runtime/web/tokens.css;已加过时标注(2026-09-01),待归档或更新 | OPEN(低优) |

## 6. 候选队列(用户提过、未排期)

- ~~上下文透视面板~~ **已交付(2026-09-02)**:dsh-context 系 DSH 插件不可装(DSH 前端线已弃 ADR-0013),照其界面理念重做——context-log.jsonl 快照 + `/admin/context` + 对话区「上下文」页签(组成条/趋势/逐项浏览器)+ 日志页快照页签;实测证据 shots-w5-context/;后续增强(压缩/剪枝事件、缓存命中、成本估算)有需要再提名。
- 使用反馈轮(按「先真实使用一周」节奏收集);
- 远程 MCP(见 §2,使用期后开工)。

## 7. 已闭合备忘(留出处,勿重开)

- 合同 Minor 三笔:tools maxItems 0→16 / finish_reason 加 tool_calls / invoke_response 加 tool_calls(commit 40988a7,W4)。
- PENDING.md 的 11 条 D-M 系列 user 裁决全部闭合(见 milestones/PENDING.md 表)。
- S5(S4 部分实现)、S9(verification 三分法,M5/M7)裁决见 FULL-REVIEW §2.4。
