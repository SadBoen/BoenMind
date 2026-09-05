# 过夜作战台账·第二轮 2026-09-06(用户令:不需要决策的任务全部处理)

- **状态:进行中** ← 守卫自动化读这里;置"已完成"后自动空转
- **单写者锁**:本台账 mtime + `git log -1 --format='%ct'`;15 分钟内无活动才算主会话已死
- 明确不做(需用户拍板,勿动):发版 tag、ADR-0016 确认→Skill v0.2 二步、VPS 密码自查、core_loop exit(70) 运维裁决、上下文压缩/记忆注入(标"待后续讨论")、theme 毛玻璃选型、F-05/F-07/F-11/F-12/P3 大重构(历史裁决缓办,夜袭硬改风险>收益)

## 任务清单(按序执行)

- [x] R1 持久读错误折叠为空收口:handle.rs 启动恢复 8 处 + events_for_session 等——区分「文件不存在(合法空)」与「读/解析失败(拒开/上抛)」,消灭故障消音成假数据
- [ ] R2 emit 坏形状事件 seq 空洞:runtime.rs 校验前置,坏事件不占 seq(保 INV-3 连续)
- [ ] R3 MCP 治理四件:①reload 强杀旧子进程 ②respawn 去抖+上限(启用 restart_limit)③HttpMcpTransport 请求超时 ④死配置处置
- [ ] F1 工具白名单按需挂载:Role 配置可选 allowed_tools(缺省=全量,向后兼容),turn 按白名单过滤 chat_tools
- [ ] F2 意图门控软防线:挂载工具时系统提示追加「工具纪律」段(寒暄/纯问答禁动文件与系统工具)
- [ ] F3 skill.v0_1 Rust 强类型投影:SkillDefinition 结构体+镜像测试
- [ ] F4 context-inspector 插件工具名瘦身:去 context_ 前缀,重建 exe+数据目录换装+重载验证
- [ ] F5 web_multisearch Parallel Search 接入(数组参数特例解析,用户 Key 已在库)
- [ ] Q1 ESLint 接入前端(先最小规则集,修 autofix;若存量问题>50 只登记不硬修)
- [ ] V1 全量回归:validate.py + fmt + clippy --all-targets + workspace test + tsc/build + smoke
- [ ] V2 真模型浏览器验收(工具白名单生效/意图门控/MCP 重载) + 截图
- [ ] V3 收宫:台账置完成 + HISTORY/BACKLOG 结转 + push

## 下一步(主会话已死时,自动化从这里接手)

从第一个未勾选项继续;动手前读 AGENTS.md;单写者检查(git log -1 %ct 距今<15min = 主会话活着,立即退出)。

## 环境备忘(累计)

- 启动:taskkill 旧进程 → CARGO_TARGET_DIR=target-regress cargo build -p bm-runtime --bin boenmind-server → BOEN_SECRET_MASTER_KEY(读 .secrets/dev.env)+ BOEN_MODEL_STREAM=1 内联启动,--bind 127.0.0.1:8765 --web-dir runtime/webapp/dist --mcp-config C:/Users/Boen/AppData/Roaming/boenmind/mcp.json
- 前端:e2e 断言消息文本用 .msg.user/.msg.assistant 收窄(会话列表同名标题会撞严格模式)
- 插件重建:cargo build --release 于插件目录,产物换装 <数据目录>\mcp\,经 /admin 重载
- 合同变更必跑 validate.py;提交前 fmt;clippy 口径 --all-targets
- E2E 判「禁令类文本」防假阳性:模型可能读到 ADR/报告原文里引用的句子,按时间窗甄别

## 过程记录

- R1:handle.rs 启动面 10 处改 rows_or_die(含 last_log_seq expect 防 seq 回绕);events_for_session/task 改 CoreResult,handlers 两调用点透传 ?;EventsAll 诊断端口保留尽力语义(补注释)
- R2:emit 坏形状事件改 tombstone 占位(StoreWriteRejected 落在原 seq 槽,持久+总线),坏事件本体不再进总线;新增世界级回归测试(tombstone 后 seq 连续+落盘断言),键集须守合同注册表(key/reason), rejected_type 扩键被注册表精确断言拦下已回退
- bm-core 87 测全绿
