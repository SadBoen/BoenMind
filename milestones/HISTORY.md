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
| W4b 对话内审批联动 | 2026-09-02 | (见 git) | 289 | **写操作工具审批闭环交付**:chat_tools 按 Broker 同口径暴露审批类能力;turn.rs 审批等待流(BM_APPROVAL 标记随 SSE 上屏+轮询裁决 300s);/admin/approvals 免鉴权裁决端点;前端审批卡片(工具名+真实参数+批准/拒绝);顺带修三真 bug(幂等键跨回合碰撞/工具空 schema/结果回喂无引导);真模型实测笔记写入审批全闭环(卡片→批准→执行→结果回喂→收尾,截图 06、07) |
| 健壮性:坏 MCP 条目容错 | 2026-09-02 | (见 git) | 289 | load_mcp_setups 条目级容错 + 服务器启动循环隔离——单条 MCP 配错不再拒绝整个服务启动;测试语义更新(坏条目跳过/混合装载) |
| 前端自动化冒烟套件 | 2026-09-02 | (见 git) | 289+6 | Playwright 6 用例(对话闭环/新建清空/上下文页/角色页/插件页去重/日志页签);网络层 mock 确定性零 token;`npm run test:smoke` 全绿 4.4s;CI 接入留候选 |
| 工程债批处理(F-01~F-04/F-10) | 2026-09-02 | (见 git) | 291 | F-01(P1)exec_log 四处 I/O expect 改告警+内存兜底(外部条件不再 panic);F-02 幂等收据落表失败留痕;F-03 glm_http 静态桥删除改构造注入(运行必败→可用);F-04 位点 meta 损坏统一告警;F-10 补 t155(send_failed 收口)与 exec_log 降格两测试 |
| W4b Skill 挂载 | 2026-09-02 | (见 git) | 291 | **合同 Minor:capability/skill.v0_1 新增**(Skill=知识包,只是数据不改变权限);config/skills.json + /admin/skills CRUD + 角色编辑挂载勾选;bm-core::roles::compose_role_prompt 统一组装(基底 prompt+挂载技能指令;会话烤入/回合热读双口径一致);真模型实测「押韵诗人」挂载后回复严格遵循打油诗格式且透视面板核验指令已注入(截图 08);W4b 全部闭合 |
| W4b 多角色管理与会话切换 | 2026-09-02 | (见 git) | 289 | **角色库 CRUD 与会话级选择交付**:config/roles.json 扩展多角色模型 + /admin/roles 增删改查及设为全局默认;Agent 结构与 turn.rs 组装支持会话级 system_prompt 覆盖;Composer 工具栏集成角色切换下拉,发请求传 X-Bm-Role;真模型实测「代码架构师」角色答复口吻精确匹配设定,上下文透视面板中 System Prompt 验证生效,截图 shots-w5-context/04 |
| v0.0.1 首发:官方插件移入+Linux 发布线 | 2026-09-02 | `v0.0.1` | 插件 27+1 | **web-multisearch 自独立仓移入 `plugins/mcp/web-multisearch/`**(官方自带插件;外仓历史归档分支 archive/boenmind-mcp-servers,外仓目录已清;--self-describe 紧凑单行修复随移入库);release.yml 重写为 v* tag 触发的 Linux x86_64 发布线(boenmind-server+插件+预构建 dist 同包,自动建 GitHub Release,含 INSTALL-linux.md);用户裁决只要 Linux(VPS 安装场景) |
| W6 对话级模型选择 | 2026-09-02 | (见 git) | 全量绿+validate 绿 | **设置「模型提供商」改名「模型」+常用清单+输入框选模型三合一**:①providers.json 增 modelsCommon(⊂models,设置页多选勾选,卡片显「清单共 N 个 · 常用设置为:…」);②合同 **Minor**:agent.send_input 增可选 model_override(turn 降级链整体替换);③bm-providers 新增 RoutingConnector(按 model_id 分发,未命中回落默认,invoke_stream 透传保流式),World/回合循环零改动;④providers 写后免重启重建路由+密钥播种(secret:model.<id>,INV-5 不破);⑤输入框模型下拉(localStorage 持久化,刷新/新对话不变,**同会话中途切换下一条即生效**);⑥未知模型 400 带可用清单;真模型实测同会话 glm-5.3-flash→mimo-v2.5 热切换,快照证两 turn model_id 各归其位(VPS mock 根因=未配 provider,W6 后设置页配好免重启即真模型) |
| W7 关于页+在线升级 | 2026-09-02 | (见 git) | 全量绿 | **设置左侧新增「关于」**:版本/平台/数据目录/更新源 + 检查更新(GitHub latest release 三段版本比较,按平台选资产)+一键升级(下载→sha256 校验→tar 解包→换装二进制/dist/plugins→BOEN_UPGRADE_CHILD 拉新进程+旧进程排空,子进程绑定重试 60s);apply 仅回环地址;workspace 版本对齐 release 线(0.1.0-m1→0.0.2);release.yml 补 windows 双平台包;**铁规矩立档:未经用户明示严禁发版(打 tag/发 release)**,在线升级只消费不发布;apply 全链待下次授权发版实战 |
| 界面反馈:输入框下拉主题化 | 2026-09-02 | (见 git) | — | 用户反馈两则:①「角色:/模型:」文字标签删除只留下拉;②原生 select 弹出层直角不合皮肤——两下拉换 Radix 主题化组件(圆角+popover 配色+分组+勾标,跟主题走);升级按钮按检出结果条件显示(已是最新不显示,防呆设计,用户问询已答) |
| 界面反馈:页签紧凑化+左右抽屉 | 2026-09-02 | (见 git) | tsc/构建绿+浏览器实测 | ①WORKSPACE 文件/产物/待办页签太厚重——降高降字号,active 去底色块只留 2px 细下划线;②SESSION/WORKSPACE 无收放——chat-head 两端加 PanelLeft/PanelRight 门型开关(lucide 同族),收起=列宽归 0+面板 padding 清零,状态入 bm_layout 刷新保持;⚠ 列宽过渡动画移除(无头渲染环境冻结布局,即时收放更稳) |
| 修抽屉收放布局散架 | 2026-09-02 | (见 git) | 浏览器落位断言三态全过 | **用户截图报障**:收 SESSION 后聊天区整片空白、WORKSPACE 挤成细条——根因=收起时条件卸载分隔条,网格子元素数(5)与模板列数(6)不匹配,自动错位一格(聊天进 0px 列、WORKSPACE 落 5px 列);修=分隔条始终渲染,收起时仅 visibility:hidden+禁指针;教训=网格布局验收必须断言各面板落位(x/w),只量轨道宽/单面板宽会漏错位(本轮初验即栽在这) |
| 界面反馈:上拉菜单+目录树右键 | 2026-09-02 | (见 git) | curl+浏览器实测 | ①输入框两下拉改上弹(side=top),去「默认」字样,Provider 名 12.5px/600 vs 模型名 12px 缩进,在用项打钩移到名字前;②目录树右键菜单=重命名(对话框)/复制绝对/相对路径/下载/文件夹打包下载 zip——后端新增 /admin/fs/rename + /admin/fs/download(safe_resolve 防逃逸,256MB/5000 条目守门,Content-Disposition 含 RFC5987 中文名);③rail 图标 fg-3→fg-2 加深;多选打包留候选(需先做树多选 UX) |
| 前端美学设计系统全面整构与统一 | 2026-09-02 | (见 git) | build绿+contracts绿+291测试绿 | ①全域消除硬编码色彩补丁(建立 --state-success/warn/error 语义令牌及 .notice-* 全局类,替换 8 页面写死 Tailwind 色);②规范 Z-Index 标阶(--z-base/splitter/drawer/settings/dropdown/modal/popover/toast,修复 Dialog 与 SettingsPage 混乱);③玻璃主题弹层全面毛玻璃化(右键菜单/Dialog/审批卡片/文件预览加 backdrop-filter 与半透白底,消除实体色块遮挡);④特质化下潜(古典温润纸墨色/卡通底部色影/玻璃弹窗遮罩模糊);⑤字阶标尺收敛(消除任意小数点像素,统一 5 级阶梯)+ Button 微交互触感(active:scale-[0.97]) |
| 前端工程架构与规范深度治理(评审闭环) | 2026-09-02 | (见 git) | build绿+contracts绿+291测试绿 | 采纳外部架构评审有效建议并闭环:①统一 CSS 变量源(消除 styles.css 中 --radius 与 :root 双重声明,让 shadcn/Tailwind 变量作为唯一基线);②移除 thread.tsx 中重复定义的 local Button,回归统一 @/components/ui/button;③加固 themes.ts 背景图 URL 白名单清洗(防范 CSS url() 逃逸与 XSS 注入);④抽象统一 storage.ts 常量定义与安全存取器,根治全站 7 处 localStorage 魔法字符串;⑤Composer 异步请求归入 api.ts;⑥清理 surfaces.tsx 死代码;远期项(McpPage 拆分/ESLint CI)登记 BACKLOG §7 |
| 外部评审合并版复核(后端二修+架构候选入册) | 2026-09-02 | (见 git) | 294 测试绿+contracts绿+clippy 改动面零警告 | 报告前端部分基于旧快照(其 P0/P1 已随 2c53a26/db27dff 闭合,逐项对照表存档 REVIEW-2026-09-02-external);本轮修二:①roles.rs 的 skills.json 读取提出技能循环体(读盘/解析次数不再随挂载数放大);②openai_compat 进程级静态 OnceLock 会话表归入 AppState.v1_sessions(随路由生灭、测试隔离,语义不变);顺带 cargo fmt 收纳上轮 fs_rename/fs_download 未格式化的 26 行;架构级建议全部登记 BACKLOG §8(Skill v0.2 与 App 产品面=候选待裁决,MCP 完整热插拔=补登记真实欠账,内置能力全量 MCP 化=复核意见倾向不采纳) |
| 五项新工与缺陷全面交付(流式+热插拔+Skill v0.2+音乐App+远程MCP) | 2026-09-03 | (见 git) | 294+6 冒烟全绿+contracts 绿+clippy 零警告+构建绿 | **用户裁决批次全量交付**:①**启动流式修复**:boenmind-server 组装对齐 config/model.json stream 优先于 env 兜底;②**MCP 完整热插拔**:mcp_reload 全流程支持修改/删除/新增免重启(hub.disconnect_server 发送 shutdown/exit+CapabilitiesUnregister 摘除注册表与持久库 binding);③**Skill v0.2 第一步+ADR-0016**:合同 Minor 增发 version+references(按需加载),发布 ADR-0016 确立 wasmtime WASM 沙箱+Broker 七步管线覆盖设计;④**音乐播放器 App**:实现规格入册 MUSIC-APP+apps/music_server.py stdio MCP+前端工作区新增「音乐」页签与内置 HTML5 播放器(扫描/搜索/列表/播放);⑤**远程 MCP 传输层**:mcp-server 合同扩展 sse/http/streamable-http 传输及 bearer_token,bm-providers 新增 HttpMcpTransport+initialize 协商握手 |
| v0.0.3 发版:Linux 服务器单平台 | 2026-09-03 | `v0.0.3` | 插件 27+1 全绿 | 用户明示发版;release.yml 收敛为单 Linux x86_64 作业(去掉 Windows 矩阵,PLAYBOOK 口径同步),包内增补 apps/ 真实 App(wiki/market/music);workspace 版本 0.0.2→0.0.3(顺带修 getrandom 0→0.4 fill 缺失);发布前修复插件集成测试环境敏感断言(config_hot_reload 热重载证据改 mock B 命中标志——ddgs 外网源恢复可用后与 mock 在 RRF 同分,首位断言属硬币正反面);Release 资产:boenmind-0.0.3-linux-x86_64.tar.gz+SHA256SUMS |

## W 序列验收惯例

W 序列不另立 review 文件:验收门与实测证据并入各规格(W2 §7、W3 §6;W4 待回填,见 BACKLOG)。截图目录:shots-w2/、shots-w3/、shots-w5-context/。
