# 里程碑与大事时间线(HISTORY)

> 由 ADR-0015 确立:交付状态只记录在这里与 git,其他文档不再复制进度。**只追加**:新批次收官在表尾加行,不回头改旧行;细节权威=各规格/回看文件与 git tag。
> 「当前在哪」看根目录 AGENTS.md 顶部一行;「还欠什么」看 milestones/BACKLOG.md。

| 批次 | 交付日期 | tag / commit | 测试 | 结论与要点 |
|---|---|---|---|---|
| M0 范围/合同/测试基线 | 2026-08-28 | `m0.2-contracts-frozen` | — | 合同库冻结 v1.0;黄金轨迹可回放;非目标清单与扩展点六问生效 |
| ADR-0009 部署形态裁决 | 2026-08-29 | — | — | VPS 托管 + Web/TUI Surface + Windows Tauri 壳;受限解除「无远程访问」 |
| M1 最小 Runtime 与单 Agent 闭环 | 2026-08-29 | `m1-runtime-loop` | 50 | 规格/回看入库;GT-01 两场景可回放 |
| M2 持久化/事件日志/崩溃恢复 | 2026-08-29 | `m2-persist-recovery` | 68 | 四项混沌验收通过;ADR-0004 四项 M2 适配落地 |
| M4 Capability/Broker/权限审批 | 2026-08-29 | `m4-capability-broker` | 134 | 11 条硬约束全部落地;三 Surface 同源审批闭环 |
| M3 统一 Wire API、CLI 与跨平台启动 | 2026-08-29 | `m3-surface-cli` | 74 | CLI/桌面/Web 三形态同源可用 |
| M5 Butler、Task 和长期监护 | 2026-08-30 | `m5-butler-task` | 188 | 八项前置结算条件闭合;ADR-0002 口径升级「成立」 |
| M6 Team、Delegate 和多 Agent 协作 | 2026-08-30 | `m6-team-delegate` | 196 | 四门禁强制点化;ADR-0002 条件 5 余项闭合 |
| M7 Provider、MCP 和 App 隔离 | 2026-08-30 | `m7-provider-mcp` | 213 | 五句通过条件逐条结算;真实网关实网验证;ADR-0010 |
| M8 首批真实 App 与发行质量 | 2026-08-30 | `m8-apps-release` | 229 | 双真实 App+Judge+实网压测+备份迁移+三平台;ADR-0011;**阶段一收官** |
| M9 阶段二第一批 | 2026-08-30 | `m9-stage2-batch1` | 254 | 记忆抽屉授权×模型真流式×worker 自主环 v0;passed_with_conditions |
| 全面回看 M1-M9 | 2026-08-30 | — | 260 | `FULL-REVIEW-2026-08-30.md`;四道门禁全绿;F-01..F-11 入审计台账;C4 回写列下一批前置;五项用户拍板(先使用一周/桌面包搁置/看护 15min·3 次·24h 等定案) |
| Web 界面改版 v3(D-M3-1) | 2026-08-30 | — | 260 | dsh 双栏布局定稿、审计 A-06~A-08 销账(前端线后被 ADR-0013 整体弃用,后端管理面保留) |
| 弃用 dsh 复刻前端 | 2026-08-31 | ADR-0013 | 261 | runtime/web 全量删除(137 文件),归档分支 `archive/m10-dsh-frontend`;后端合同面不动 |
| W1 WEBUI 开篇(ADR-0014) | 2026-09-01 | — | 261 | assistant-ui 自建壳(runtime/webapp)+ 后端 OpenAI 兼容插座 /v1(SSE 流式);真浏览器验收:发消息→真实网关流式上屏 |
| W1.1 真键盘吞字修复 | 2026-09-01 | `ac12a4d` | — | assistant-ui 0.12.28→0.15.17(IME compositionend 卡死修复);用户实测确认 |
| W1.2 输入框贴底等小修 | 2026-09-01 | `0bfeecf` 等 | — | 空会话输入贴底;移除禁用搜索占位框 |
| MCP 搜索插件+热装载 | 2026-09-01 | `c18d248` | — | web_multisearch(12 源聚合,独立仓 boenmind-mcp-servers)接入;CapabilitiesRegister 运行期追加;kill_on_drop 误杀修复 |
| MCP 探活与设置紧凑化 | 2026-09-02 | `8decb82` | — | 联通绿点+测试按钮+30s 轮询;外观页主题下拉(后被 8cb2316 改小 LOGO 组) |
| W2 设置中心+工作区+可拖布局 | 2026-09-02 | `5748f9a` | 280 | provider 库 CRUD/探针/清单/设为当前(config_store 接回);webadmin /admin/* 管理面;目录树+文件预览;三栏拖宽持久;五道验收门实测(截图 milestones/shots-w2/) |
| W2/W3 用户反馈轮一 | 2026-09-01~02 | `c7acf23`/`03d0cba`/`9876117` | — | 设置=rail 开关+独占页;目录树去卡片;SESSION 单行;玻璃设置页统一毛玻璃 |
| W3 主题系统·两级换肤 | 2026-09-02 | `551bcca` | 280 | 四主题+每主题设置项 schema+实时预览;四道验收门 DOM 探针实测 |
| W4 对话工具闭环+角色 | 2026-09-02 | `b2fc6a8`+`40988a7`(合同 Minor) | 280+ | tools 合同启用(maxItems 16)+FinishReason::ToolCalls+Role::Tool;直通工具免审批注入;角色 system_prompt 热生效;遗留=W4b(见 BACKLOG) |
| W4 用户反馈四则 | 2026-09-02 | `8cb2316` | 280+ | 主题小 LOGO 按钮组;MCP 配置 Dialog 下拉选择器;RolesPage 重复渲染修复;临时补丁脚本清理 |
| W5 会话记忆修复+上下文透视 | 2026-09-02 | (见 git) | 289 | **修复多轮无记忆**:turn.rs 历史回喂(session_chats 台账,20 轮/24K 字符上限,Cmd::RememberTurn);上下文透视面板:context-log.jsonl 快照(messages+tools+usage,INV-5 脱敏+16K 截断)+`/admin/context`+对话区「上下文」页签(组成条/趋势/逐项浏览器)+日志页快照页签;真模型实测两轮暗号往返+跨页面重载均答中,截图 shots-w5-context/;动因=用户提名 dsh-context(评估:DSH 插件不可装,照理念重做,登记 BACKLOG §6) |
| W5 查漏补缺批 | 2026-09-02 | (见 git) | 289 | 用户要求核对逻辑与界面后修四处:①remember_turn 存活守卫(close 不取消在途回合,迟到落定不得复活已清退台账条目,INV-6 交互面);②快照增 attempt 字段(区分降级链重试的同序号步骤);③快照耗时改真实墙钟测量(补 openai_http「0 占位、由调用方测量」从未兑现的欠账,成败两路均记,实测 4.8s/4.2s);④界面耗时显示+attempt>1 标注+空态提示细化+无会话号禁用「仅当前会话」;工具轮两步流水(工具结果入组成)实测可见 |
| W5 用户反馈两则 | 2026-09-02 | (见 git) | — | ①插件页去重(**用户裁决改版**:插件页只列系统内置能力,MCP 服务器组不再重复展示、统一归「MCP 管理」页;旧 mcp: 前缀 PIN 点击兼容跳 MCP 管理);②聊天「新建对话」加号接线(此前无 onClick 纯占位):派发 bm-chat-new → 中止在途回合+丢弃会话号+清空视图,下一条消息自动开新会话;真浏览器复验两处 |
| F-06 架构模型回写 | 2026-09-02 | (见 git) | 289 | **硬纪律 2 闭合**:boenmind.c4 回写消除 19 处模型-实现漂移(超前容器标注阶段一/二归属、App=MCP 真实形态[Market/Wiki]、Web Surface 现役形态与 /admin 关系、阶段一内核组件补齐[watchdog/contextLog/bm-judge/内建连接器]);开工硬前置全部就绪 |

## W 序列验收惯例

W 序列不另立 review 文件:验收门与实测证据并入各规格(W2 §7、W3 §6;W4 待回填,见 BACKLOG)。截图目录:shots-w2/、shots-w3/、shots-w5-context/。
