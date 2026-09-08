# 已裁决与驳回总清单(SETTLED)

> 定位:全仓「已有结论、勿再翻案」的**唯一查重清单**(ADR-0026 确立,2026-09-08)。
> 评审/回头看/审计出新意见前,先在此查重:**已有条目不得重提**;翻案须本清单无此条**且**带新证据。
> 主代理每驳回一条,当场补一行;结论被后续决策推翻时加「已废止」标注并写新行,不删旧行。
> 行格式:一行一条 = 结论(含一句依据)|出处|日期。来源=各批复核在 HISTORY/BACKLOG 行内注记与历次评审台账(原文溯 git 史)。
> 出处文件已按 ADR-0027(2026-09-08)移出仓者(规格/回看/结算/HISTORY/PENDING/debates/验证报告),均溯 git 史,结论本身仍然有效。

## 1. 产品与流程裁决(用户拍板,勿重提)

1. 发版纪律:未经用户明示严禁打 tag/发 GitHub Release;push main 不受限 | PLAYBOOK §1/HISTORY v0.0.7 行 | 2026-09-02
2. 发布线=v0.0.3 起单 Linux x86_64(.tar.gz+.sha256);Windows 端在线升级提示缺资产=设计非故障 | HISTORY v0.0.3 行 | 2026-09-03
3. apps/ Python 演示 App 只入仓不入发布包 | HISTORY v0.0.13 后复盘复核行(文档校准) | 2026-09-03 裁决/09-07 校准
4. 内置能力全量 MCP 化=不搞,勿再提案;同步 trait 快路径系刻意选型(零进程开销),全量子进程化对单用户场景属倒退 | BACKLOG §6 | 2026-09-02
5. 一次性评审/审计/复盘报告不入库:复核结论写 HISTORY 行、误报驳回写 BACKLOG/SETTLED 行,原文溯 git 史 | milestones/README 惯例 4 | 2026-09-07
6. 涉网页 UI 的「仿真/模拟/实测」=一律真实浏览器可视化点击+截图为证;脚本模拟须用户明示放行 | PLAYBOOK §3 | 2026-09-07
7. 里程碑/合同/历史记录文件不回头改写(状态行除外);新事实进 HISTORY/BACKLOG/新 ADR | PLAYBOOK §5/milestones README 惯例 3 | 2026-08 起惯例
8. 桌面安装包(cargo tauri build)搁置不排期,骨架保留 shell/tauri | PENDING D-M8-3 | 2026-08-30
9. W 序列不另立 review 文件,验收入规格;截图留档 shots-*/ | milestones/README | 2026-09-01
10. 前端路线=assistant-ui 自建壳,不抄成品(dsh 线 2026-08-31 弃用归档,勿提议复活) | PENDING D-M10-1/ADR-0013/0014 | 2026-09-01
11. 评审/回头看对话严禁绑定「演示项目」工作区(=真仓),只准绑默认工作区沙箱(2026-09-07 R2 实战:评审 AI 在自己克隆改了 20 文件,真仓幸免) | 本机评审循环台账收拢(.tools 不入仓) | 2026-09-07
12. 文档清理历次裁决勿翻案:一次性报告 13 份已删(2026-09-07)/.zcode/plans 12 份已删/归档与合并建议驳回 24 条 | HISTORY 文档清理审计核实批行 | 2026-09-07

## 2. 架构与实现刻意设计(评审高频误报区,勿当缺陷报)

1. 单写者纪律为核心状态机根基;管理面(/admin)不入冻结合同系基线既有决策 | BACKLOG §3.2(webadmin 审计行) | 2026-09-07
2. 直通工具 inline_sync 同步收据特判=2026-09-03 真实 P1 修复(直通工具 60s 超时根因),chat_direct_tool 专项测试在册;调用本体仍走 Broker 审计 | HISTORY VPS 实测 P1×2 修复行+第三方评审复核行 | 2026-09-03/09-07
3. 审批抽屉只授抽屉谓词(子集匹配)=审批主路径刻意设计,精确匹配被 t131_132 审批流测试否决;Grant 精确模式已登记 BACKLOG 待裁决 | HISTORY 架构回头看复核行 | 2026-09-07
4. context-log.jsonl=对话正文唯一落盘(A4 决策);events.jsonl 只留摘要(INV-3);重启续聊=context-log 逆向重建(rebuild_session_chats,W5/重启续聊批已验收),「诊断日志数据流倒挂」定性已驳 | HISTORY W5 行+重启续聊行+第三方评审复核行 | 2026-09-02/09-06/09-07
5. 工具循环防护现状=总轮数安全网 limits.tool_rounds_max(默认 64 可关)+同命令同参死循环熔断(连续 5 次);「固定 5/30 轮上限」与「无熔断」两种说法均过时 | HISTORY 任务呈现重构行+架构回头看复核行 | 2026-09-06/09-07
6. 工具 wire 短名(SHORT_WIRE_NAMES:rgrep/powershell 等)=2026-09-06 刻意交付(rgrep 命名裁决);wire_name manifest 自描述已登记 BACKLOG 待裁决 | HISTORY rgrep 命名裁决行+第三方评审复核行 | 2026-09-06/09-07
7. 审批/异步工具 400ms 轮询单写者通道=刻意语义(改推送已登记 BACKLOG 低优) | BACKLOG §3.2 | 2026-09-07
8. MockConnector=无模型环境首启兜底,刻意设计非残留 | HISTORY v0.0.13 后复盘复核行 | 2026-09-07
9. failed→running 自愈=合同 Minor 增发(resend_after_failure,failed 退出终态集),刻意设计非「状态机漏洞」 | HISTORY 外部评审三轮复核行+v0.0.13 后复盘行 | 2026-09-03/09-07
10. 意图门控软防线(回合挂工具时注入「工具纪律」System 段)=用户提报已落;硬门控=BACKLOG 待设计,非缺陷 | HISTORY 过夜二轮行+BACKLOG §1 | 2026-09-06
11. 会话 Cookie 内存态 30 天/重启重登=既有语义;logout+Secure 已登记 BACKLOG 待用户裁决 | HISTORY v0.0.5 行+BACKLOG §4 | 2026-09-03
12. SSE 流式链路=900s 硬顶(limits 热可配)+keepalive+任意字节 poke 三层俱在;「SSE 无超时」不实 | HISTORY 外部架构审查复核行+架构回头看行 | 2026-09-07
13. exec 限制现状=默认 120s/前台硬顶 600s(limits.json 热可调)+超限自动转后台(ADR-0024/0025);「exec 60s 铁顶」说法过时(旧版实例升级即解) | HISTORY v0.0.12 行 | 2026-09-07
14. 生产能力装配面=model.invoke+exec/job_output+fs.*+skill scripts;memory_capabilities() 零装配=BACKLOG §1 已登记待设计,非遗忘 | BACKLOG §1 | 2026-09-07
15. 事件信封 JSON 字段名=`type`(serde rename),不是 event_type | PLAYBOOK §2 | 2026-08-30
16. libsqlite3-sys bundled 默认启用 SQLITE_ENABLE_FTS5,M5 memory 检索 FTS5 实际生效,LIKE 仅兜底(权威源 PLAYBOOK §4;历史行写「AGENTS.md 明示」者同指此条) | PLAYBOOK §4 | 2026-09-02
17. 门户墙:未配置密码=墙不启用零影响(本地/测试零变化);公网收紧已落(v0.0.5);/admin Bearer 鉴权欠账已登记 BACKLOG 专项 | HISTORY v0.0.5 行+BACKLOG §3.2 | 2026-09-03/09-07
18. 事件日志已内存化(JsonlEventLog 镜像,文件仍权威);「每次 append 全量扫盘」说法过时 | HISTORY 外部评审三轮复核行 | 2026-09-03
19. 配置写盘已原子化(atomic_write=tmp+fsync+rename 全接线);skills/roles 损坏 JSON=拒绝覆写(Result 化)非静默清盘 | HISTORY 三轮复核行+v0.0.13 后复盘行 | 2026-09-03/09-07
20. 前端门禁=tsc --noEmit 固化进 build+ESLint CI 门禁(react-hooks error 级,ef40c34);「前端无静态检查」过时 | HISTORY 会话删除行+ESLint CI 行 | 2026-09-06/09-07
21. 【现状变更】对话链路限制默认全零=不限制(用户 2026-09-08 裁决,ADR-0028;0 语义消费点全适配):tool_rounds_max/模型单呼超时/降级链重试/流式硬顶/非流式等待/工具与审批等待/历史回喂双上限/autorun 轮数;唯一保留硬停止=同命令同参连续 10 次熔断(5→10)——§2-5「64 轮安全网」与 §2-12「900s 硬顶」的默认值口径自此过时(机制仍在,可设置页调回) | ADR-0028 | 2026-09-08
22. 【现状变更】回喂忠实性(用户 2026-09-08 裁决,ADR-0029):内核回喂只承载事实——失败/超时附真实错误与未执行原因、幂等抑制如实告知、熔断/触顶调用不再蒸发、内核代写 assistant 终稿废除、硬编码话术全清(含 §2-10 工具纪律段的硬编码形态废止,可经插件通道回归);detail_ref 恒 None 系对 INV-5 过度解读,脱敏后 detail 字段为合法承载 | ADR-0029 | 2026-09-08
23. workspace_id 会话绑定已跨重启持久(schema v10);「不持久」说法过时(进程内作用域部分=刻意) | HISTORY 会话基础欠账清零批行 | 2026-09-06
24. webadmin.rs 上帝文件已拆 11 子模块(2026-09-07);引用「webadmin.rs 2985 行」=过时快照 | HISTORY v0.0.13 行 | 2026-09-07
25. MCP 官方插件启动播种+墓碑防复活+批准即上线=ADR-0023 既定;播种重名 panic 已修(seen 去重) | HISTORY 插件管控批行 | 2026-09-06
26. 内置冻结清单=system.exec(过渡态例外)+fs.* 四件+model.invoke 内核私有(ADR-0020/0021);「内置该迁移 MCP」类提案勿再提(见 §1-4) | ADR-0020/0021 | 2026-09-04
27. OpenCode Go 网关 2026-09-07 起强制 x-opencode-session 头,openai_http 已默认带(BOEN_OPENCODE_SESSION_ID 可固定);「缺头 400」=网关新要求已适配 | HISTORY v0.0.13 行 | 2026-09-07

## 3. 已驳回评审意见(逐案带证据,勿重查)

1. core_loop panic 无挂死:命令方即刻收错误,rx drop 即时报错(2026-09-05 复核证实);升级 exit(70)=BACKLOG 待裁决非缺陷 | BACKLOG §3.2 | 2026-09-05
2. 恢复边界:启动无条件 recover,open(校验)/recover(修复)职责分离——评审 P0-5 驳回 | HISTORY 架构回头看复核行 | 2026-09-07
3. 取消竞态不存在:单终态+迁移表拒绝表外迁移——评审 P1-18 驳回 | HISTORY 架构回头看复核行 | 2026-09-07
4. keepalive+任意字节 poke 防护已在——评审 P1-27 驳回(同族「SSE 无超时」见 §2-12) | HISTORY 架构回头看复核行 | 2026-09-07
5. 运行时配置键序:BTreeMap 即规范序,「键序敏感」不实——评审 P1-25 驳回 | HISTORY 架构回头看复核行 | 2026-09-07
6. 投影自愈已在/阶段一不存在孤儿 Agent/workspace_id 已登记——2026-09-03 外部评审三误报 | HISTORY 外部评审复核+P1 修复行 | 2026-09-03
7. 「工具循环无步数熔断」不实(总轮数+同参熔断双防线,见 §2-5) | HISTORY 外部评审三轮复核行 | 2026-09-03
8. InProcMcpServer 仅集成测试消费,非生产路径残留 | HISTORY v0.0.13 后复盘复核行 | 2026-09-07
9. GLM feature 已定义(默认关=刻意)/ProvidersPage=活代码——2026-09-07 v0.0.12 外部审查两驳回 | HISTORY 外部架构审查复核行 | 2026-09-07
10. apps/smoke_test.py=CI 协议冒烟门禁(ci.yml 硬编码路径),移动即破坏 CI;apps 打包不动=用户裁决(§1-3) | HISTORY 第三方评审复核行 | 2026-09-07
11. 「工具纪律注入系模型幻觉/自行发明」不实=用户提报的软防线(§2-10) | HISTORY v0.0.13 后复盘复核行 | 2026-09-07
12. capability 操作不落 operations 表=正当(纯内存系统容器态,approvals/grants/outbox 完整承载规范状态) | BACKLOG §6/M4-review §6-4 | 2026-09-04
13. load_world_rows expect×14「恢复失败=拒开」=handle.rs 明示设计决策(fail-fast),维持;可选小改仅错误信息可读化 | BACKLOG §3.2(P4 行) | 2026-09-07
14. rebuild_session_chats/工具纪律/记忆 LIKE 兜底三项「报告指瑕」均系刻意设计或已在册(分见 §2-4/§2-10/§2-14) | HISTORY 第三方评审复核行 | 2026-09-07
15. 「后台作业孤儿进程泄漏」不实:服务退出=进程级退出→tokio runtime drop→kill_on_drop 兜底杀作业子进程(单宿主形态);ADR-0025 已裁决台账/日志边界——审计 P1 驳回 | bm-providers/jobs.rs+ADR-0025 | 2026-09-08
16. 「tsconfig 缺 exclude 致 e2e/scripts 混入生产类型树」后果误报:tsconfig include=["src"] 已收拢,build(eslint+tsc)均不扫测试目录 | runtime/webapp/tsconfig.json+package.json scripts | 2026-09-08
17. 「前后端命名风格撕裂」非同层混用:camelCase 忠实映射 providers/probe 的 json! 字面量家族,snake_case 忠实映射 bm_contract/context-log wire 家族,前端未自创第三套 | bm-surface-http/webadmin/providers.rs+webapp api.ts | 2026-09-08
18. /admin/mcp/search-test/{name}=管理面对插件扩展方法(web_search_test)的测试通道,前端 ServerConfigDialog「测试搜索」在用,刻意交付非 OCP 破坏 | bm-surface-http/webadmin/mcp.rs+webapp ServerConfigDialog.tsx | 2026-09-08
19. 「withdraw 魔法字符串」系审批裁决 wire 字面量(admin respond 端点解析),与事件信封 `type` 字段同口径(§2-15),非坏味道 | bm-core/handlers.rs+approval.rs | 2026-09-08
20. 「MCP manifest 解析 unwrap_or_default 静默接受畸形 JSON」定性不实:876 行系对**已成功** tools/list 响应的 tools 数组提取降级(缺数组=插件贡献零工具);无名工具已被 tool_manifest 里 normalize_tool_name 的 `?` 丢弃,无「错误工具被注册」面 | bm-providers/mcp.rs:872-895+tool_manifest:89 | 2026-09-09
21. 「LimitKey.value/default 前端应声明 number|null」=不可达:KEY_META 键与 Limits 结构体字段对齐,serde 序列化必产全字段,`unwrap_or(Value::Null)` 仅守卫性兜底;若真漂移页面立现 null 即可见缺陷 | bm-surface-http/webadmin/limits.rs:19-31 | 2026-09-09
22. 「App.tsx handleNewChat 删 setTimeout 清理=回归」不实:该函数系 CustomEvent 监听回调,返回值从不被消费,原 `return () => clearTimeout(timer)` 本为死代码,删除系正确清理 | runtime/webapp/src/App.tsx:301-305 | 2026-09-09
23. 「FileBadge 删 case "rs"=行为变化」不实:`.rs` 已被特殊文件名段 `fileName.endsWith(".rs")` 拦截且配色逐字相同,switch 内 case 系不可达重复,删除系正确去重 | runtime/webapp/src/w1/components/FileBadge.tsx:30-31 | 2026-09-09
24. 「webapp build 未接 lint=质量关卡缺项」已覆盖:ci.yml webapp-lint job 显式先 `npm run lint` 再 build,本地 build 不接 lint 系取舍非缺口 | .github/workflows/ci.yml | 2026-09-09

## 4. 引文纠错(常见编造/误引,引用前先核)

1. 「基线 §78 第一公民长期记忆」=编造引文;基线仅 §1-§24 | HISTORY v0.0.13 后复盘复核行 | 2026-09-07
2. 「ADR-0009-transport-convergence」文件不存在,实为 ADR-0009-deployment-and-surface;纯 SSE 面=ADR-0014 既成决策 | HISTORY 第三方评审复核行 | 2026-09-07
3. 合同版本=v1.0 冻结(字段只增不破);「v0.1」系 skill.v0_1 等文件名谱系后缀误读 | HISTORY 第三方评审复核行 | 2026-09-07
4. Git 历史已重写(2026-09-07 filter-repo 清 target-inspector 121MB+target-wms 335MB):main 与 v0.0.9/11/12 tag 哈希全变,既有 clone 须重拉;GitHub Release 资产不受影响 | HISTORY webadmin 拆分+历史瘦身行 | 2026-09-07
5. 磁盘 target-inspector/target-wms 残留=被忽略的本地构建产物,git 全历史已清(b13f531/38b0cb5),「仓库里有 456MB 构建产物」=引旧 clone | HISTORY 第三方评审复核行 | 2026-09-07
6. 【已废止 2026-09-08:用户重裁升级为删除,见下行 13】辩论转录(debates/ 六份)=基线 §17.1/§24 证据底稿,不归档 | HISTORY 文档清理审计核实批行(溯 git 史) | 2026-09-07
7. 【已废止 2026-09-08:用户重裁升级为删除,见下行 13】M-reviews/M2/M4-settlement=ADR-0015 决策 6 明列不动之物+多处活锚点,不归档 | HISTORY 文档清理审计核实批行(溯 git 史) | 2026-09-07
8. .tools/=本机工作台不入 git,其内 STATE/LEDGER/LOCK 非丢失文档 | PLAYBOOK §7+HISTORY 文档清理行 | 2026-09-07
9. W-ui-inventory 导航已在 milestones/README,删除反致 W2 规格 §5.1 活引用悬空 | HISTORY 文档清理审计核实批行 | 2026-09-07
10. INSTALL「重启生效」经 providers.rs 代码核实为真(设为当前=改默认须重启;增删改 provider 本身免重启热重建) | HISTORY 文档内容级体检批行 | 2026-09-07
11. docs/ 目录已清空:引用 docs/*.md 前先核存在性,报告原文溯 git 史 | HISTORY 一次性报告清理行 | 2026-09-07
12. server*.log 已在 .gitignore,非敏感泄露 | HISTORY v0.0.13 后复盘复核行 | 2026-09-07
13. 过程文档已全量删除(2026-09-08 用户裁决「仓库只存当前框架/功能、未来方向、欠账;ADR-0027」):M1-M9 规格+回看 18 份、M2/M4 结算表、W1-W10+MUSIC-APP 规格 10 份、HISTORY、PENDING、debates 转录 6 份、deepwiki 验证报告,共 39 份——旧文档指向它们的注记均溯 git 史;辩论与验证结论仍可查 architecture/README 速览与基线 §17/§24;2026-09-07 审计的留档取舍由本条取代 | ADR-0027 | 2026-09-08
