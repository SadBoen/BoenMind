# HanaAgent（OpenHanako）全面研读与对标评估

> 状态：调研完成（2026-08-13）。对标报告，不含实现。结论与拍板点见文末。
> 上游：https://github.com/liliMozi/openhanako （6k stars，Apache-2.0，v0.446.6，TS/Electron 全栈）

## 一、项目概况

HanaAgent 是"有记忆、有灵魂的私人 AI 助理"，面向"坐在电脑前工作的每一个人"（作者自述是文员，为日常办公优化）。与 BoenMind 同为 pi 系产品：引擎用 `@earendil-works/pi-*`（Pi SDK，0.80.3），桌面 Electron + React + Hono server + better-sqlite3。核心卖点：**记忆、人格、自主性（心跳/定时/书桌）、多 agent 协作、多平台消息桥**。插件系统是与 BoenMind 同源的"目录即声明 + manifest 补充"模式。

## 二、对标总览

| 领域 | Hana 做法 | BoenMind 现状 | 借鉴价值 | 建议 |
|---|---|---|---|---|
| 消息内容搜索 | 投影缓存 + 标题优先 + jieba 分词 + 内存评分 | **无**（仅会话列表按名过滤） | ★★★★★ | **吸收**（见拍板点 1） |
| 记忆系统 | 传送带分级（today→daily→week→longterm）+ SQLite FTS5 事实库 + 无向量标签检索 | pi JSONL 记忆 + ctx-compactor 修剪 | ★★★★ | 吸收事实库/检索思路（拍板点 2） |
| 定时任务 | trigger+executor 统一模型，全部归一为受限隔离 agent session | 已有 cron | ★★★★ | 吸收"隔离执行纪律"（拍板点 3） |
| 书桌/笺 | 文件系统即任务队列（jian.md + 指纹 + 状态回写闭环） | 无 | ★★★★ | 新功能候选（拍板点 4） |
| Windows 沙箱 | 自写 C++ helper：受限令牌 + 合成 SID ACL 写隔离 + 私有 desktop | 无 OS 级隔离 | ★★★★ | 思路吸收（拍板点 5） |
| 插件系统 | 贡献全家桶（routes/providers/extensions/page/widget/后台任务）+ 能力门禁 + 市场 + dev 循环 | 仅 tools/skills（QuickJS） | ★★★★ | 门禁语义/回滚链/网络白名单（拍板点 6） |
| 会话分支 | append-only 树 + branch head 持久化 + 回滚语义 | 无分支 | ★★★ | 候选（拍板点 7） |
| 角色卡/技能包 | zip 打包导出导入（人格+记忆+技能），plan 两步导入 | 无 | ★★★ | 候选（拍板点 8） |
| 人格系统 | identity+yuan+ishiki 三段合成 + 回落链 + prompt cache 分区 | 专家团队角色 prompt | ★★ | 吸收"记忆规则约束行为"技巧（拍板点 2 内） |
| 心跳/主动行为 | 31min 巡检 + 笺变更触发 + 通知回执 | 无 | ★★ | 暂缓（定位不同，拍板点 9） |
| 移动 PWA / LAN | /mobile/ PWA + QR 扫码 + 设备注册 | 无 | ★★ | 暂缓（拍板点 9） |
| 消息桥（TG/飞书/QQ/微信） | Bridge 适配器体系 | 无 | ★ | 不做（本地优先定位） |
| 压缩 | cache-preserving 原地压缩 + 零模型有损投影 + fresh-compact 每日刷新 | 按模型水线注入（已实测省 78%） | ★★ | 可借鉴 fresh-compact 防"摘要过期"（拍板点 10） |
| 子代理 | child session + 线程门闩排队 + 断线续跑 | 专家团队已落地（P9 结构化返回） | ★★ | 对照补漏：reply 排队（拍板点 10） |
| 安全审计日志 | JSONL + 密钥掩码 + capability 决策摘要 | 无 | ★★ | 可选（拍板点 10） |

## 三、深度调研（机制要点）

### 3.1 记忆系统（lib/memory/）—— 最值得学的一块

**三层混合存储**：
- 会话摘要：每 session 一个 JSON（`memory/summaries/{id}.json`），滚动维护；
- 编译记忆：纯 Markdown 传送带（`agentDir/memory/`）：`memory.md`（最终汇编，注入 prompt）、`facts.md`、`today.md`、`week.md`、`longterm.md`、`daily/{date}.md`——全部可读可手改；
- 元事实库：`memory/facts.db`（SQLite + FTS5，表 `facts(fact, search_text, tags, time, session_id)`）。

**写入时机**：事件驱动 ticker（每 10 轮滚动摘要；日期翻篇跑每日流水线 compileDaily→rollDailyWindow→compileFacts→deepMemory）。每日步骤状态持久化在 `daily-state.json`（schemaVersion=4），**进程重启断点续跑，防重复计费**。

**"近期强保留、旧事件淡化" = 按天滚动传送带**，不是衰减算法：滚动摘要（覆盖式）→ compileToday 用**水位线增量**（只喂新增 delta）→ 日期翻篇 compileDaily 蒸馏成 2-3 句日记 → `week.md` 零 LLM 纯文件装配（6 日窗口，1200 字符硬上限）→ 滚出窗口的条目 LLM fold 进 `longterm.md`。所有产物带 `.fingerprint`（md5）防重复编译；**用户手改文件是权威改写**。

**检索（v2 无向量）**：`search_memory` 工具 = LLM 双侧生成标签（提取时与搜索时语言习惯一致）+ 标签匹配，结果不足 3 条时 FTS5 全文兜底（CJK 2/3-gram 分词）。零 embedding、零重排，成本极低。

**记忆使用规则（对模型行为的关键设计）**：记忆是"内化背景知识、用户不提就不主动翻、禁止'我记得'表述"、冲突时以对话为准——避免模型把记忆当对话历史背诵。

**借鉴亮点**：① 分级淡化比向量库便宜、可审计、可手改、纯文件可移植；② 指纹+水位线+断点续跑三件套防重复计费；③ 无 embedding 的标签+FTS5 检索路线；④ 记忆变更走 `[hana_reminder]` 环境账本注入（300 字符上限、游标去重、receipt 消费）。

### 3.2 自主性/自动化（hub/ + lib/desk/）

**调度器纯代码化**：cron 判定层不碰 LLM——60s 轮询 + nextRunAt 游标 + 手写 5 字段 cron 解析器 + 失败退避 `[0,1m,5m,15m,60m]` + per-job AbortController（20min 超时）+ 每轮读盘（跨实例感知）+ 原子写。

**trigger+executor 统一模型**：三种触发（at/every/cron）与"直接通知/插件动作"全部归一为 `trigger + executor({kind:"agent_session", agentId, prompt, ...})`。旧版通知/插件动作执行器在迁移时被包装成**受限 agent session prompt**（"只完成固定动作，不要扩展"）——所有自动化统一走后台隔离 session。

**隔离执行纪律**（最值得抄）：后台一律 `engine.executeIsolated()`（独立 session，跑在 `activity/` 目录，不进入前台会话、不污染记忆）+ `permissionMode:"auto"` + `approvalPolicy:"deny_on_prompt"` + `allowHumanApproval:false` + 权限上下文标注 `surface:"automation"`。结果只通过 ActivityStore + 事件 + 桌面通知回流。

**心跳（heartbeat）**：每 agent 一个实例，默认 31 分钟（对齐槽位边界、timer unref、5min 超时、手动触发 2min 冷却）。Phase 1 工作台巡检：根目录快照 diff（name+mtime，跳过点文件和产物目录）+ overwatch.md 注意力清单，prompt 明确"不是用户消息、独立判断、不要提问"，产物写 `activityDir/`，最后必须调 `patrol_update_log` 写日志。Phase 2 笺巡检：仅指纹变化时触发。

**文件变更 = 轮询快照对比，不是 fs.watch**：`jian-registry.json` 存 MD5-12 指纹（笺内容指纹 + 文件列表 `name:mtime` 指纹）；**执行后重新扫描并用执行后的指纹回写注册表，防止任务自身改动造成"自激振荡"**——这个细节很关键。

**书桌/笺（Desk）= "文件系统即任务队列"**：用户把任务写进工作区 `jian.md`（笺正文=任务指令），心跳扫描发现指纹变化即触发隔离执行（带该目录 cwd）。`<!-- exec-log -->` 标记块之后是程序写入的"上次执行状态"（任务快照），agent 通过 `jian_update_status` 工具只提交状态（in_progress/completed/skipped/failed）。prompt 内嵌行动规则：语义比较当前笺与快照——仅格式变化=同一任务继续、目标/次数/范围变化=新任务重来、已完毕且一致则本轮不调工具。**异步协作闭环**：用户放笺 → 心跳（31min 内）→ agent 独立执行 → 状态回写笺 → 下次心跳据新指纹继续推进（如 4/5→5/5）。

### 3.3 双层安全沙箱（lib/sandbox/）

**PathGuard（应用层，四级）**：BLOCKED / READ_ONLY / READ_WRITE / FULL。所有路径先 realpathSync 解符号链接；按操作要求最低级别（read≥READ_ONLY、write≥READ_WRITE、delete=FULL）。判定顺序：BLOCKED 名单 → agentDir 敏感文件 READ_ONLY → agent 数据目录 READ_WRITE → workspaceRoots FULL → hanakoHome 兜底 READ_ONLY → 其余系统路径 READ_ONLY（可关）。**策略常量单一来源（policy.ts 同时供给 PathGuard 与 OS 沙箱 profile），杜绝双份 ACL 漂移**。

**OS 级**：macOS Seatbelt / Linux Bubblewrap / **Windows 自写原生 C++ helper**（`desktop/native/HanaWindowsSandboxHelper/main.cpp` ~2400 行）：
- `CreateRestrictedToken`（DISABLE_MAX_PRIVILEGE | LUA_TOKEN | WRITE_RESTRICTED）+ restricting SIDs = Everyone + logon SID + **每个可写根哈希出的合成 SID** + `S-1-5-33`（WRITE_RESTRICTED code SID）；
- **写隔离** = 对每个可写根在真实文件系统加 ACL：GRANT 合成 SID（含 DELETE/DELETE_CHILD，带继承），`.git`/session-files 加 DENY ACE 覆盖；**旧 DACL 快照退出时恢复**（AclRestore）；
- 子进程 `CreateProcessAsUserW` 受限令牌 + 随机私有 desktop + kill-on-close Job（进程树清理）+ 退出状态走 stderr `terminal-v1` 协议；
- 缓存写隔离：TEMP/LOCALAPPDATA/APPDATA/npm/pip cache 重定向到 `hanakoHome/.ephemeral/`；PowerShell 沙箱内启动探针（防 Constrained Language Mode）；
- **网络隔离 Windows 不支持**（restricted-token 沙箱无 network-off，`assertSandboxNetworkSupported` 直接抛错）；macOS/Linux 默认断网，escalate 通道才开网。

**命令 preflight 双列表**：HARD 列表（`del /s`、`reg delete`、`takeown`、`icacls`、`net user`、`schtasks`、`powershell -ExecutionPolicy bypass`、`format`、`bcdedit` 等）**任何模式（含 escalated）都拦**；SANDBOX_ONLY 列表非 escalated 拦截且返回带升级出口的文案。escalated 通道跑 direct exec 但仍过 PathGuard + preflight。

**审计与凭据**：`security-audit-log.ts` JSONL（事件含 actor/capability 决策摘要，key 名匹配 apiKey/token/secret 自动掩码，500 字符截断）；`secret-custody.ts` 精确 key 名注册表掩码 + "********"占位回写保留原值；`pii-guard.ts` 硬正则（sk-/AKIA/gsk_/ghp_ 等）在持久化前替换 `[REDACTED]`。**已知边界：Windows 上不做文件权限收紧**（NTFS 无 POSIX 位，靠目录继承 ACL）——SECURITY.md 自认，BoenMind 的 Rust 后端可用 DPAPI/ACL 补强。

### 3.4 插件系统（core/plugin-manager.ts 1770 行）

**目录即声明 + manifest 补充**：`tools/ skills/ commands/ agents/ routes/ providers/ extensions/ index.js` 目录约定；manifest 增加 `trust/activationEvents/capabilities/sensitiveCapabilities/network{allowedHosts,methods,...}/ui.hostCapabilities/contributes{configuration,page,widget,settingsTab}/depends`。无 manifest = 目录名当 id、默认 restricted。未知字段/目录一律忽略（前向兼容）。

**两级权限**：`trust:"full-access"` + **用户设置总开关**（关着则完全不加载）；restricted 可 tools/skills/commands/agents/配置/bus.emit-subscribe-request，禁 bus.handle/routes/extensions/providers/registerTool/生命周期。**关键语义：权限管"系统给哪些扩展接口"，不是代码级沙盒**——restricted 工具在主进程跑，文件/命令边界由 PathGuard+OS 沙箱统一兜底。

**能力门禁（"缺失=legacy、显式=严格"）**：`capabilities`/`sensitiveCapabilities` 两字段全缺 = legacy 等同声明全部（行为不回退）；一旦显式写出任一列表即严格校验，显式 `[]` 拒绝所有敏感能力。ResourceIO 每操作 `assertCapability("resource.read/...")`；`ctx.network.fetch` 需声明 `network.fetch` + allowedHosts（`*.` 通配）+ 仅 HTTPS + 私网需 allowLocalhost + 超时/大小上限；未声明返回 403 `PLUGIN_CAPABILITY_NOT_DECLARED`，错误带 `capability/permission/declared/granted` 字段可直接定位。

**安全机制**：拖拽安装（zip 解压→单层目录归一→`.installing-` 临时目录原子就位）；升级先备份到 `plugin-backups/<id>/`（最多 3 份），**加载失败自动回滚重载**；同版本降级被拒；安装记录 `plugin-installs.json`（版本/来源/URL/sha256/历史 20 条）；`plugin-format-guard.ts` 拒绝 OpenClaw 格式 zip。

**市场（Obsidian 式）**：官方社区目录独立仓库（OH-Plugins，PR 提交条目），app 只读市场不管理源；条目 `versions[]` 每版本独立 minAppVersion 兼容性 + sha256 校验下载；客户端按 SemVer 选"当前 app 兼容的最高版本"，状态（updateAvailable/downgrade/reinstall）给 UI。

**Dev 循环**：dev 源放 `plugin-dev-sources/`，`installFromSource` 拷到 `plugins-dev/<id>`（不污染正式目录）→ `reloadPlugin` 热重载（freshImport 清模块缓存）→ `invokeTool` smoke test → `diagnostics` → `dev.scenarios`（声明式 smoke 场景，destructive 需 `allowDestructive`）。Agent 可见的 dev 工具默认关闭，设置里开关。

**UI 注入（page/widget）**：iframe + 一次性 `pluginIframeTicket` 文档凭证 + 仅限 assets 路径的 HttpOnly 短会话 cookie + `X-Hana-Plugin-Surface-Session` 请求头（只授权本插件 route，不携带宿主 scope）。iframe 内 `@hana/plugin-sdk`（toast/clipboard/external.open/resources），敏感宿主调用须 `ui.hostCapabilities` 白名单。`assets/` 只放构建产物，拒绝路径穿越/隐藏文件/source map。

**其余贡献点**：routes 挂 `/api/plugins/{id}/...`（仅 full-access，请求级上下文 + onError 转 403 带明细）；providers 声明式（`runtime.kind:"local-cli"` 结构化参数绑定，禁 shell 拼接）；extensions（Pi SDK 事件拦截：tool_call/tool_result/before_provider_request/context 等，**全量插件可改 LLM 请求链**）；configuration schema（scope global/per-agent/per-session + sensitive 字段落盘掩码 + 设置页自动生成）；后台任务（task:register-handler/register/schedule，重启标 recovering 由插件 onload 恢复）+ deferred:* 结果送达；`activationEvents` 懒激活（onStartup/onPageOpen/onToolCall:name/onBusRequest，缺省等价 onStartup）。

### 3.5 会话/压缩/子代理（core/session-*）

**JSONL append-only 树**：每行一个 entry（首行 header），message 带 parentId；读取必须从物理末条沿 parentId 回溯到 root（线性扫描会混入已丢弃分支）。**分支 head 持久化**（`session_branch_heads` 表：leafId/observedTailLeafId/revision/reason），打开会话时物理 tail 是 persistedLeaf 后代且非丢弃观察则 `append_recovery` 自动续，否则沿用 persistedLeaf——支持"回滚到旧分支"语义。谱系哈希（sha256 链式）校验压缩摘要覆盖位置。

**压缩（与 BoenMind 路线不同，可互补）**：
- cache-preserving 原地压缩：`stableSerialize` 严格校验"Pi preparation 分区 == live 前缀"（前缀契约，不匹配拒绝压缩），摘要替代旧区（上下文不可逆）；
- **lossy-local-compaction：零模型零网络**重建检查点——从 branch 文本重渲染 user/assistant 纯文本，剔除 toolCall/toolResult/thinking，用 lineage hash cursor 校验覆盖边界；
- **fresh-compact：每日（日界+5min，空闲 60s 后）重跑压缩刷新滚动摘要**——解决"长期会话摘要过期"；policy.ts 存 systemPrompt+state 的 sha256 快照，"Already compacted" 视为满足。
- 动态 reserve = `max(16384, 10% 窗口)`；压缩后把早于摘要的 assistant usage 清零（防 provider 输出钳制误判）。

**会话搜索**：`GET /sessions/search?q=&phase=title|content`——title 相位查 title+firstMessage，content 相位查投影缓存的 `allMessagesText`。**纯内存评分无 FTS**：精确子串 1000/700 分，否则按 token 累加（80-120+长度奖励）；分词 `@node-rs/jieba`（cutForSearch 模式 + 自定义词典），NFKC 归一化，混合 CJK/字母查询必须有锚点 token。**列表投影缓存按 `${size}:${mtimeMs}` stat 签名**，签名即 API 的 `revision` 字段，web/mobile 靠比对决定是否补拉——搜索免扫全文件。JSONL 单行 >1MB 自动投影（截断为 `[omitted N chars]`），改前备份 `.repair.json`。

**子代理**：`subagent`（创建可续实例，立即返回 taskId+threadId，结果后台投递 via deferred-result）→ `subagent_reply(threadId)` 续跑（**每线程 gate 链，busy 时 reply 排队而非报错**）→ `subagent_close`。独立 child JSONL 会话（默认 .ephemeral 临时、persist 可复用），继承父会话 cwd/workspaceFolders/access，用目标 agent 身份跑；断线续跑 = 同一 child session 沿 leaf 继续 append；执行者归属写入 manifest 表。

**多 agent**：每个 agent 是独立文件夹（config.yaml/identity.md/ishiki.md/pinned.md/memory//skills//sessions/），AgentManager 只做注册表+焦点切换，不做跨 agent 调度；子代理可指定 `agent` 以该 agent 身份运行。

**CLI / 移动端**：CLI 是 HTTP 客户端（serve/status/sessions/continue/chat/bundle pull/data restore，全部 --url/--token 连远端 server）；/mobile/ 是 standalone PWA + service worker，接入 = `/access/mobile-qr.svg`（LAN URL 编码 QR）+ credential token 或本地账户密码登录 + 设备注册表。

**引用卡片**：实现很轻——选中历史文本随下条用户消息作为 `quotedText` 提交，纯文本引用无锚点跳转。BoenMind 可超越（带 messageId 的可跳转引用）。

## 四、拍板点（待用户决策）

按"先讨论后实施"原则，以下仅列选项不动手：

1. **消息内容搜索**（强烈建议做）：turso 已支持 FTS5，中文分词短板可学 Hana 的 jieba 路线（Rust 侧 tantivy+lindera 或 turso FTS5+自写分词）。范围：仅会话消息 / 含会话列表 / 文件内容？
2. **记忆事实库**：是否在 pi 记忆之外加"事实分层"（事实库 + 编译记忆传送带）？还是保持 ctx-compactor 现状、只吸收"记忆规则约束模型行为"的提示词技巧？
3. **定时任务执行纪律**：现有 cron 是否迁移到"隔离 session + deny_on_prompt + surface:automation"模型（对齐 Hana 防后台任务污染前台/越权）？
4. **书桌/笺**：是否做"文件系统即任务队列"（用户放任务文件 → 定时巡检 → agent 执行 → 状态回写）？如果做，巡检周期、目录约定（工作区 jian.md vs 独立目录）怎么定？
5. **Windows 执行沙箱**：是否给 exec 命令加 OS 级隔离（Rust 侧 Job Objects + 受限令牌/最小特权）？投入大，是否先只做命令 HARD 列表 + 审计日志？
6. **插件系统增强**：吸收哪些——能力门禁"缺失=legacy/显式=严格"语义？升级回滚链？网络出口白名单（allowedHosts）？dev 循环（install/reload/invokeTool）？市场（此前已定"商店不做浏览界面"，Hana 的独立仓库 PR 模式是否重新评估）？
7. **会话分支/回滚**：turso 的 messages 表目前是线性存储，改成 append-only 树 + branch head 成本不小，是否值得？
8. **角色卡/技能包导出导入**：是否做 agent/技能配置的 zip 打包交换（两步导入预览）？
9. **暂缓清单**：心跳主动巡检、移动 PWA、消息桥、人格模板——定位不符或投入产出比低，先不做。
10. **小项**：子代理 reply 排队（门闩）、fresh-compact 防摘要过期、安全审计日志、带 messageId 的可跳转引用卡片。

## 五、一句话结论

Hana 与 BoenMind 定位不同（个人助理 vs 开发者工具），但其**记忆分层、自动化隔离纪律、Windows 沙箱思路、插件安全工程（门禁/回滚/白名单）**四块是成熟的工程范式，直接对应 BoenMind 已知短板（无搜索、后台任务越权风险、exec 无 OS 隔离、插件无网络白名单）。建议优先吸收第 1、3、6 项，第 2、4、5 项论证后定。
