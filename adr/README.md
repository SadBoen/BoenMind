# BoenMind 架构决策记录(ADR)

> 第 0 层(架构基线)的决策日志。基线正文只保留稳定结论;增量裁决、修订与条件全部在这里。两者冲突时以更新的 ADR 为准。
> 现行 = accepted 且未被取代;`status: superseded/withdrawn` 者已归档于 `archive/`(ADR-0040)。
> **新增 ADR**:建 `adr/ADR-NNNN-<slug>.md`,头部带 front-matter(status/date/supersedes/superseded_by/summary);下方索引由 `scripts/gen_adr_index.py` 生成,勿手改。

<!-- ADR-INDEX:BEGIN 由 scripts/gen_adr_index.py 生成,勿手改 -->
| ADR | 标题 | 状态 | 一句话决策 |
|---|---|---|---|
| [ADR-0001](ADR-0001-registry-broker-bus.md) | Registry/Broker/Bus 三权分立 | accepted-with-conditions | 三权分立维持;Broker 授权数据面快路径合法化,binding_epoch 固化 |
| [ADR-0002](ADR-0002-butler-coordinator.md) | Butler 仅持协调权,Coordinator 为受限队长 | accepted-with-conditions | 协调动词按 Task 子树裁剪+safe/mutation 二分+Grant 物化 |
| [ADR-0003](ADR-0003-l0-generation-upgrade.md) | L0 独立控制面与 Runtime generation 升级回滚 | accepted-with-conditions | 回退限定工件/指针层;probation 观察窗;完整代际流程仅限 L1 Major |
| [ADR-0004](ADR-0004-task-state-in-l2.md) | Task 规范状态归 L2,任务板仅为投影 | accepted-with-conditions | 三层归属;task_epoch fencing;事件日志压实为强制义务 |
| [ADR-0005](ADR-0005-everything-is-plugin.md) | 万物皆插件:内核只含合同与最小机制 | accepted-with-conditions | 最小机制封闭清单+安全不变量四判据+改合同双重门槛 |
| [ADR-0006](ADR-0006-permission-by-contract.md) | 权限以合同显式化(元原则) | accepted | 未列入合同的权力视为不存在 |
| [ADR-0007](ADR-0007-l0-bootstrap-exemption.md) | L0 自举豁免与升级信任链 | accepted-with-conditions | L0 是唯一显式自举豁免体,豁免清单冻结 |
| [ADR-0008](ADR-0008-architecture-as-code-validation.md) | 架构即代码与外部实证验证 | accepted | C4 DSL 为唯一权威;C1-C8 验证成立;S1-S10 proposed |
| [ADR-0009](ADR-0009-deployment-and-surface.md) | 部署形态与 Surface 策略:VPS 托管、Web/TUI Surface、Windows 桌面壳 | accepted-with-conditions | VPS 托管+Web/交互式 TUI Surface+Windows 壳;受限解除「无远程访问」 |
| [ADR-0010](ADR-0010-third-party-model-gateway.md) | 第三方中转网关作为模型 Provider | accepted | 接受 NewAPI 型网关为真实模型通道;内容视同对网关可见,不改 input_trust 语义 |
| [ADR-0011](ADR-0011-first-apps-as-mcp-servers.md) | 首批真实 App 以 MCP Server 形态接入 | accepted | App 以进程外 MCP stdio server 接入,不新增内核「App 对象」合同 |
| [ADR-0013](ADR-0013-dsh-frontend-abandoned.md) | 弃用 dsh 复刻 Web 前端,换新参考界面重做 | accepted | dsh 复刻 Web 前端全量删除(归档分支),换新参考界面重做 |
| [ADR-0014](ADR-0014-webui-w-series-assistant-ui.md) | W 系列 WebUI 技术路线——assistant-ui 组件库 + OpenAI 兼容插座 | accepted | assistant-ui 组件库自建壳+OpenAI 兼容插座;W 序列独立编号 |
| [ADR-0015](archive/ADR-0015-doc-consolidation.md) | 文档体系整理——熔入式修订与三层附页 | superseded→ADR-0040 | 基线增补熔入正文不挂引注块;HISTORY/PLAYBOOK/BACKLOG 三附页;milestones/README 导航 |
| [ADR-0016](ADR-0016-skill-v0.2-scripts-broker-pipeline.md) | Skill v0.2 脚本执行架构与 Broker 管线覆盖 | accepted | wasmtime 运行时沙箱+WASM 为主零 Python+全量纳入 Broker 七步管线 |
| [ADR-0017](ADR-0017-context-mode-rust-mcp.md) | context-mode Rust MCP 官方插件 | accepted | 外部 MCP、官方随包但默认不启用、Linux 单 exe、显式批准与 Broker 风险边界 |
| [ADR-0018](ADR-0018-workspace-registry-and-session-binding.md) | 工作区注册表与会话级工作目录绑定 | accepted | config/workspaces.json 项目注册;AgentSpec/SendInput 只增 workspace 字段;回合级 system prompt 注入;探针无 shell |
| [ADR-0019](ADR-0019-system-exec-approval-tool.md) | system.exec 内置命令执行工具(审批类) | accepted | 常规 shell 能力+每条命令审批卡;内置异步能力(.async);真沙箱仍归 ADR-0016 |
| [ADR-0020](ADR-0020-internal-capability-freeze.md) | 内置能力冻结与执行载荷纪律 | accepted-with-conditions | 内置能力严格封闭(仅保留系统内置命令执行 system.exec 作为过渡态例外) |
| [ADR-0021](ADR-0021-fs-tools-internalization.md) | 文件工具集内置化(fs.*)与 code-tools 插件退役 | accepted-with-conditions | 查/读/改收编内核(修订单 ADR-0020 清单范围,用户裁决);沙箱=工作区注册表;审批分级不变 |
| [ADR-0022](ADR-0022-native-tool-protocol-and-description-governance.md) | 工具调用原生协议还原与工具描述治理 | accepted-with-conditions | role:tool+tool_call_id/tool_calls 回喂/删防复读禁令/manifest.description 自描述/fs_edit edits 数组(2026-09-06) |
| [ADR-0023](ADR-0023-bundled-plugin-default-install-and-lifecycle.md) | 官方随包插件默认安装与生命周期管控 | accepted | 启动播种默认安装(修订 0005/0006/0017 对官方件的显式批准要求)/墓碑防复活/批准即上线卸载即下线/purge 物理删除/弃用标记(2026-09-06) |
| [ADR-0024](ADR-0024-limits-config-surface.md) | 运行时限制集中配置面(limits.json) | accepted | ~40 项硬编码限制收敛单文件+安全钳制+env>文件>代码默认+LimitsCell 热生效;设置页全量可编辑;exec 默认 120s/上限 600s 对齐业界(2026-09-07) |
| [ADR-0025](ADR-0025-background-command-promotion.md) | 长命令后台转轨(run_in_background 与超限自动转轨) | accepted | exec 增 run_in_background+超限自动转轨(Hermes 式);system.job_output 轮询收取(DSH 式);回合 prompt 注入作业摘要;不做主动推注入(2026-09-07) |
| [ADR-0026](archive/ADR-0026-doc-discipline-normative-narrative-separation.md) | 文档纪律——规范与叙事分离(评审上下文减负) | superseded→ADR-0040 | 规范文档零编年史/HISTORY 单行制/SETTLED.md 唯一查重清单/评审必读入口收窄(2026-09-08) |
| [ADR-0027](archive/ADR-0027-docs-minimalism-delete-after-delivery.md) | 文档极简纪律——仓库只存规范、方向与欠账(过程文档交付即删) | superseded→ADR-0040 | 过程文档(规格/回看/转录/时间线)交付即删;交付全史=git;入库白名单四类(2026-09-08) |
| [ADR-0028](ADR-0028-conversation-limits-default-zero.md) | 对话链路限制默认全零(0=不限制) | accepted | 十项限制默认归零(0=不限)+0 语义显式实现+熔断只拦同命令同参 10 次(2026-09-08) |
| [ADR-0029](ADR-0029-feed-fidelity-no-kernel-coaching.md) | 回喂忠实性原则——内核只回事实,话术出内核 | accepted | 回喂只承载事实+错误原文保真(detail)+去内核指导性教练话术(2026-09-08) |
| [ADR-0030](ADR-0030-server-side-approval-adjudication.md) | 审批裁决后台化——权限模式为服务端会话状态 | accepted | 模式按对话记于服务端(默认ask)+yolo由服务端裁决放行并审计标注auto+前端降级选择器+硬拒绝/熔断不动(2026-09-09) |
| [ADR-0031](ADR-0031-agent-v02-comm-plane.md) | Agent v0.2 通信面——消息语义架于进程内事件总线,成员级身份 | accepted | 信件语义走单写者总线不破INV+成员级工牌(与#31合并)+共享面先行/点名与身份随批次2/远程网格留阶段二(2026-09-09) |
| [ADR-0032](ADR-0032-binding-epoch-continuity.md) | binding_epoch 代际连续性修复与 manifest 注册期冻结校验 | accepted | 注销墓碑化+注册按持久max+1续代+快照先行于落库+删物理删除端口+注册过冻结门禁+scope_label放行冒号分层(2026-09-11) |
| [ADR-0033](ADR-0033-skill-script-lifecycle-hot-reload.md) | 技能脚本生命周期——skill.* 异步分道归位与热重载 | accepted | provider 命名约定单源判定(mcp./.async/skill.)+unregister_skill+管理面热重载走墓碑续代+共享管理器实例(2026-09-11) |
| [ADR-0034](ADR-0034-plugin-protocol-sdk.md) | 插件协议最小 SDK——双语言收口与未知工具口径统一 | accepted | Rust(plugins/mcp/sdk)+Python(apps/mcp_sdk.py)最小 SDK 单源化协议循环+未知工具统一 -32602/--self-describe 独立合同化(2026-09-11) |
| [ADR-0035](ADR-0035-mcp-trust-chain-and-subprocess-limits.md) | MCP 插件信任链收口与子进程资源上限 | accepted | trust 显式消费+payload 合同字段令 sha256 指向真实载荷(解释器条目 fail-closed)+扫描执行面 UI 显式化+子进程 Job Object/rlimit(2026-09-11) |
| [ADR-0036](ADR-0036-execution-mode-contract-sourced.md) | 能力执行分道以合同声明为唯一真源 | accepted | manifest.execution_mode 落声明为分道真源(未声明时回退命名约定)+同步无 deadline 为显式非目标+CallContext 入端口记为方向不实施(2026-09-11;2026-09-12 校订措辞:前缀回退保留而非删除) |
| [ADR-0037](ADR-0037-provider-health-binding-split.md) | Provider 健康与 Binding 状态的分工收口 | accepted | BindingStatus=生命周期/代际持久门,provider_health=运行期健康进程内门,不合并+卸载走 Draining 排空+dispatch 查 binding+restore 保真(2026-09-11) |
| [ADR-0038](ADR-0038-memory-drawer-rule-contractized.md) | memory 抽屉授权规则合同化 | accepted | manifest 增 authorization 声明(主体系留+读放宽),Broker 步 4.5 变解释器删硬编码规则;memory 仍不生产可达(2026-09-11) |
| [ADR-0039](ADR-0039-stage2-candidates-disposition.md) | 阶段二候选产品面处置（壳扩展 / 协作网格 / 权限分级 / Surface 插件化） | accepted | #43 维持 deferred/#45 批次2 待并发夹具/#46 产品级 OPEN/#53 路由后关账/#59-5 远期不立项(2026-09-11) |
| [ADR-0040](ADR-0040-doc-governance-gate.md) | 文档治理以机器门固化——ADR 状态机 + 追踪白名单 | accepted | 整并取代 0015/0026/0027;ADR 状态机与取代双向一致入 CI;可提交面 .md 收归白名单;治理门自带自检防静默摘除(2026-09-12) |
| [ADR-0041](ADR-0041-plugin-identity-lifecycle-contract.md) | 插件身份与生命周期契约落地(去特化第一步) | accepted | 补插件身份契约(PluginKind/PluginMeta)+ CapabilityProvider 增 plugin_meta/shutdown 默认方法 + 注销调生命周期 + wasm 宿主去 skill. 前缀守卫(ADR-0040 治理首批架构改动) |
| [ADR-0042](ADR-0042-model-router-port-and-persist-schema-correction.md) | 模型路由端口上移(去具体依赖)与两处"分层越界"假阳性更正 | accepted | 模型路由关注点上移为 core 端口 ModelRouter,surface 去具体依赖;并更正早先架构报告的两处假阳性(持久化 schema 归属、bm-contract 策略提纯)与前缀派发清零 |
| [ADR-0043](ADR-0043-wasm-plugin-declaration-contract.md) | 通用 wasm 插件声明合同化(② 的真实内容) | accepted | 通用 wasm 插件声明合同化(plugins.json 的临时形状冻结为 wasm-plugin.v0_1 并纳入装载期 schema 门);②「WIT/Component 接口」核实为暂不需要,其真实内容是合同化 |
| [ADR-0044](ADR-0044-surface-service-boundary-review.md) | surface「服务层边界」核实与巨型 handler 拆分 | accepted | surface"服务层边界"核实——AppState 持 EventStore 是端口依赖非越层、SSE 直读日志是刻意性能设计;真实债仅 chat_completions 巨型 handler,已抽取会话寻址与派发(462→316 行) |
| [ADR-0045](ADR-0045-plugin-identity-wiring-and-kind-convergence.md) | 插件身份接线与过度建模收敛(PluginKind 6→2、移除空转 shutdown) | accepted | 接线插件身份——CapabilityDiscovery 承载身份成为真实消费者、MCP/内核能力补声明、PluginKind 收敛为 Tool/Connector、移除空转 shutdown 钩子 |
| [ADR-0046](ADR-0046-surface-provider-inversion-completed.md) | 补完 surface→providers 依赖反转(端口化管理面) | accepted | 补完 surface→providers 依赖反转——抽 core 端口 SkillHost/JobBoard.list/McpAdmin,AdminConfig 全改端口,bm-providers 降为 dev-dependency,surface 源码零具体类型 |
| [ADR-0047](ADR-0047-three-local-dedup.md) | 三处局部去重单源(default_data_dir / 无 TTL 哨兵 / mcp 配置读取) | accepted | 三处局部去重单源——default_data_dir 归 bm-persist、100 年无 TTL 哨兵归 core 常量、webadmin mcp 配置读取改用 json_store 原语 |
| [ADR-0048](ADR-0048-split-mcp-module.md) | 拆分 bm-providers/src/mcp.rs(四职责合一的巨物) | accepted | 拆 bm-providers/src/mcp.rs(2182→1056 行)——按 banner 边界抽出 shape/transport_http/transport_stdio 三模块,公共路径经 pub use 不变 |
| [ADR-0049](ADR-0049-wasm-declaration-unification.md) | wasm 声明格式合一(单一 manifest 合成路径) | accepted | wasm 声明格式合一——归一化 WasmDecl + 单一 synthesize,skills.json 与 plugins.json 两种磁盘形状共用一条 manifest 合成路径 |
| [ADR-0050](ADR-0050-capability-dispatch-route-table.md) | 能力执行面收口(声明式路由表取代内核 if-else) | accepted | 异步能力分派由硬编码 if-else 改为声明式路由表——新 provider 族 = 追加路由,不改内核分派 |
| [ADR-0051](ADR-0051-manifest-synthesis-single-source.md) | manifest 合成单源(ManifestSpec 全族收口) | accepted | manifest 合成收口为单一合同路径 ManifestSpec——内置/fs/exec/share/context/mcp/wasm 各族只声明差异,缺省集单源 |
| [ADR-0052](ADR-0052-json-store-single-source.md) | 配置文件 JSON 读写原语单源(bm-core::json_store) | accepted | 配置文件 JSON 读写原语单源至 bm-core,严格/宽容两种损坏策略显式命名,消除三份实现与同文件双策略 |
| [ADR-0053](ADR-0053-wasm-declaration-loading-single-entry.md) | wasm 家族声明装载单入口 | accepted | wasm 家族声明装载单入口——路径/形状/选择规则收口 bm-core,启动装配与整表重载共用一条装载路径 |
<!-- ADR-INDEX:END -->

> 编号说明:ADR-0012 于 2026-08-31 随 M10 dsh 前端线归档(见 ADR-0013 编号说明),编号不回收;主干引用配置管理口径时以 config_store 现状与 W2/W4 规格为准。
> 文体注:0001-0009 状态用小写 accepted(-with-conditions),0010/0011 起用 Accepted(日期)/已裁决(用户,日期)——语义等价,统一以本表为准。
