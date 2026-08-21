# BoenMind 前端重写设计文档

> 目的：**让一个不熟悉本仓库的资深前端工程师，仅凭本文档就能从零重写前端**，做到与当前前端功能等价（并预留扩展点）。
> **技术栈自由选择**——本文档只描述「需要哪些窗口/功能/契约」，不限定框架、UI 库或布局方案；§1.2 给出当前在用的方案**仅供参考**，实现者可自选更合适的。（唯一硬约束在 §1.1，都是与后端契约相关的。）
> 本文稀缺词汇：`RPC`=后端的 JSON 信封调用（§2.1），`帧`=WebSocket 下行帧（§2.2），`投影`=(sessionId, key)→最新值的会话级键值（§2.3）。

---

## 1. 产品定位与总体约束

**BoenMind** = 本地优先的 Rust 微内核 agent 平台。后端（Rust web-server）默认跑在 `127.0.0.1:3080`，**同时服务静态前端（`frontend/dist`）与全部 API**。前端是 SPA，**无构建期后端依赖**，除 Tauri 壳外所有能力都有浏览器降级。

### 1.1 必须遵守（违反 = 不可用）

1. **RPC 信封**：所有业务 JSON 交互走 `POST /api/<method>`，请求与响应都要包信封（精确格式见 §2.1）。方法名必须与 URL 尾段**逐字相同**。
2. **WS 下行帧**：`/api/events.mux` 的帧外层有 `type/rpcId/method/payload` 四字段（见 §2.2），**审批应答的 key 是外层 `rpcId`，不是 payload 里的 approvalId**——这是坑 10 级的高危点。
3. **两条独立 WS**：聊天实时流（只认 `session/event`）与全局事件总线（认 `approval/requested/resolved`、`session/projection`）**用两条连接**，各自独立管理。
4. **`content` 必为数组**：`session.prompt` 的 `content` 必须是 `[{type:"text", text}]` 数组（§2.4.2）。
5. **投影快照不带 seq**：`session.history` 的 `projections` 是"截止当时"的快照 `{values}`；增量帧 `session/projection` 自带单调 `seq`，**higher-seq-wins**（`seq ≤ 本地水位` 的丢弃）。GoalCard 必须实现：快照当基准、增量去重。
6. **goal 变更必须带 CAS ref**：`ref:{id, revision}`（来自投影），revision 每个写 +1；CSS 并发冲突 `goal-conflict` 靠投影回灌纠正（前端静默重读即可）。
7. **文件面事实源 = `settings host.workdir`**：文件管理器以它为根，所有路径都是**相对 path**；`host.listWorkdir/readFile/writeFile` 只允许 workdir 内路径。未配置 workdir 时文件面板应显示"请先设置目录"。
8. **auth 双态**：未装配 `--auth` 时 `auth.status` 返回 `auth-not-available`（**前端直接进**）；已装配但未认证返回 `auth-required`（弹登录）。客户端 token 存储、请求头 `x-boenmind-session` 由 `client.ts` 统一处理。
9. **免登录判定必须用字符串包含**：`Error.message.includes("auth-not-available")`（这是 client.ts 的既有行为，重构不得破坏）。

### 1.2 技术栈（当前在用的方案，仅供参考，实现者自选）

- 当前：React 19 + TypeScript（strict）+ Vite（dev 5173 代理 → 3080；生产 `frontend/dist` 由后端直接服务）。
- UI：`antd` v6（`prefixCls="bm"` 隔离命名空间）+ `@ant-design/icons`。
- 布局：`dockview-react` 8（可拖拽/折叠/悬浮/关标签的多面板）。
- Markdown：`react-markdown` + `remark-gfm`；**文件预览必须 `rehype-sanitize`，聊天消息不做 sanitize**（消息来自模型，该层不防 XSS）。
- 图标补足：`lucide-react`（非必须）。
- Tauri（桌面壳，可选）：`@tauri-apps/cli`；浏览器环境 `window.__TAURI_INTERNALS__` 不存在 → 更新检查/安装/无边框拖拽全部**优雅降级为空操作/隐藏**。
- 说明：以上均**非硬性**——你选定的栈只要能实现 §4 的窗口/交互、遵守 §1.1 契约即可。注意 dev 时 `/api` 代理到 3080 的规则（§6 坑 14）。

### 1.3 模块划分建议（新前端骨架参考，非强制）

> 只划分概念模块，与具体框架解耦。前端 AI 自行决定组件/页面如何组织。

- **传输层**：RPC 封装修饰（§2.1）+ token 管理 + WS 连接管理（§2.2）；文件下载/上传封装。
- **全局状态**：当前会话 id、审批豁免表（两个都是跨视图共享的小状态）。
- **窗口清单**（具体见 §4）：
  - 会话列表窗口
  - 聊天窗口（含消息流、目标卡片、输入区）
  - 文件/工作目录窗口
  - 设置窗口（全页或弹窗皆可，见 §4.4）
  - 登录窗口
  - 全局：底部状态栏 + 审批弹窗
- **主题系统**：三档风格（黑白/卡通/玻璃）+ 背景/字号/强调色（§4.4.1）。
- **Tauri 壳**（可选）。

> 注：**编程/代码视图（CodingApp）本次不做**（当前也是纯占位）。需要时后续再加。

---

## 2. ★ 数据契约（硬约束，字段名必须逐字一致）

### 2.1 RPC 信封（`/api/<method>`）

**请求**（客户端→服务端，`fetch` POST，`Content-Type: application/json`）：
```json
{ "type": "client-request", "rpcId": "r1730000000000", "method": "session.list", "payload": {} }
```
- 有 token 时加请求头 `x-boenmind-session: <token>`。

**响应**（服务端→客户端，**恒 HTTP 200**；登录成功时额外 `Set-Cookie`）：
```json
{ "type": "server-response", "rpcId": "<回显>", "result": { "ok": true,  "value": {} } }
{ "type": "server-response", "rpcId": "<回显>", "result": { "ok": false, "error": { "code": "<code>", "message": "<msg>", "details": {} } } }
```
- `rpc(method, payload)` 成功返回 `result.value`；失败抛 `Error`。两个特殊错误：HTTP 401/403 或 code `auth-required` → 抛 `AuthRequiredError`（触发登录流转）；code `auth-not-available` → 抛 `Error("auth-not-available")`（App**用字符串包含判定**免登录）。

### 2.2 WebSocket 下行帧（`/api/events.mux`，downlink-only）

**连接**：`new WebSocket(`ws://${location.host}/api/events.mux`)`。注意：**WS 无上行**（客户端只发一次 Upgrade GET，之后不发任何帧，收到即被关）。断线重连由前端自行 `setTimeout(reconnect, 2000)`。

**所有下行帧统一信封**：
```json
{ "type": "server-request", "rpcId": "<uuid>", "method": "<帧type>", "payload": { "type": "<帧type>", ... } }
```
（`payload` 内会自动注入 `type` 字段 = method。）

**帧 type 清单**（客户端按 `method` 分发）：

| method | payload 字段 | 备注 |
|---|---|---|
| `session/event` | `{sessionId, event:{type, seq, time, surfaceOp?, data}}` | 聊天实时流（§2.4）；`surfaceOp:"append"` 仅 user/message、assistant/message、tool/result 带 |
| `session/subscribed` | `{sessionId, lastSeq}` | 连接时基线；`lastSeq=历史长度-1`（空为 -1），告诉前端"从这里接增量" |
| `approval/requested` | `{sessionId, approvalId, toolName, callId?, reason?}` | **应答 key = 外层 rpcId**（不是 approvalId！） |
| `approval/resolved` | `{sessionId, approvalId, outcome:"allowed-once"\|"rejected"}` | respond accepted 后广播 |
| `question/requested` | `{sessionId, questions:[任意JSON数组]}` | 前端当前未用（问答式审批未实现为弹窗），契约兼容 |
| `question/resolved` | `{sessionId, questionRpcId, outcome:"answered"\|"cancelled"}` | 同上 |
| `session/projection` | `{sessionId, key, value, seq}` | key 现仅 `"goal"`；`seq` 每 key 单调 +1；higher-seq-wins |

**连接时先发**：每个 attached（running 或非 blank）会话一帧 `session/subscribed`；仍 pending 的 `approval/requested`/`question/requested` 会**重放**（rpcId 原样，断线可续答）。

**分发规则**：聊天流只处理 `session/event`（且 `payload.sessionId === 当前会话`）；全局总线只处理 `approval/requested`、`approval/resolved`、`session/projection`。**两者都用这条 mux 连接**（当前实现是 useChat 与 useMuxEvent 各建一条到同一 URL 的独立连接，不共连——重写也可合并为单连接 + 统一分发，无协议障碍）。

### 2.3 会话投影（goal 卡片的数据源）

- `session.history` 返回 `projections:{asOfSeq, values}`：`values["goal"]` 为快照（**不带 seq**），`asOfSeq` 说明快照截至的 wire 水位。前端快照后把本地 seq 水位置 0，**只收 `seq>0` 的增量**。
- 增量 `session/projection` 的 `value`：`{goal:{id, revision, objective, phase, maxGoalRounds}, roundsStarted, createdAt, updatedAt}`；**`value === null` = 墓碑**（goal 被 clear，前端清空展示并退出编辑）。
- 客户端硬道理：**永远以投影为真相**，本地操作（pause/resume/edit）失败时静默、等投影纠偏；成功也靠投影广播更新 UI。

### 2.4 Wire 事件（`session/event` 的 `event.data`）

信封 `{type, seq, time(epoch ms), surfaceOp?, data}`。type 与 data：

| type | data 关键字段 |
|---|---|
| `user/message` | `{id:"user-<seq>", content:[{type:"text",text}], source:{kind:"user"}}` |
| `turn/start` / `turn/end` | `{turn}` / `{turn, reason:{kind}}`（kind∈completed/aborted/blocked/error/max-tokens/interrupted） |
| `step/start` / `step/end` | `{turn, step}` |
| `assistant/chunk` | `{turn, step, chunk:{type, index, text\|id\|name\|argumentsDelta\|usage}}`（见下） |
| `assistant/message` | `{turn, step, message:{id:"assistant-<seq>", role:"assistant", content:[blocks], source:{kind:"model"}}, usage?}` |
| `tool/call` | `{turn, step, callId, name, arguments}`（arguments = 模型原始 JSON **字符串**） |
| `tool/result` | `{turn, step, message:{id:"tool-<seq>", role:"user", content:[{type:"tool-result", toolCallId, content:[{type:"text",text:output}], isError}], source:{kind:"tool", callId}}, error?}` |

`assistant/chunk` 的 `chunk.type` 子类型：
- `block-start` `{index, blockType}`
- `text-delta` `{index, text}` / `reasoning-delta` `{index, text}`
- `tool-call-delta` `{index, id, name?, argumentsDelta}`（增量拼接 arguments）
- `block-end` `{index, block}`（block 完成体）
- `usage` `{inputTokens, outputTokens, cacheReadTokens?, cacheWriteTokens?, reasoningTokens?}`（可多次，后到覆盖）
- `finish` `{reason:{kind}}`（kind∈stop/max-tokens/tool-calls/error/aborted）

content block wire：`{type:"text",text}` / `{type:"reasoning",text}` / `{type:"tool-call",id,name,arguments}` / `{type:"tool-result",callId,output,isError}`。

**前端渲染约定**：User 消息原文（不渲染 Markdown，气泡右对齐）；Assistant 消息 Markdown（remark-gfm）；`tool/call` → 可展开的工具调用卡（状态 …/✓/✕，展开显示 arguments JSON 与 output）；`pending` 消息末尾闪烁光标 `▌`。**流式实现**：收到 `assistant/chunk` 置 streaming，把 text-delta 拼进最后一条 assistant 消息；收到 `assistant/message` 结束流式（若末条 assistant 是 pending 则用最终 text 替换）。

### 2.5 HTTP 直连（非 RPC）

| 路径 | 方法 | 说明 |
|---|---|---|
| `/api/respond` | POST | 审批应答（§2.6） |
| `/api/host.download?path=<encodeURIComponent>` | GET | workdir 文件下载/内嵌预览（图片 `Content-Disposition:inline`；svg/html 恒附件） |
| `/api/host.upload` | POST multipart | 字段 `dir`（相对目录）+ `file`（须带原始文件名）；`x-bm-overwrite:true` 头可覆盖；409 同名冲突、413 超 100MiB |

### 2.6 审批应答（`POST /api/respond`）

请求：
```json
{ "type": "client-response", "rpcId": "<来自 approval/requested 帧的 rpcId>", "result": { "ok": true, "value": { "sessionId": "...", "approvalId": "...", "outcome": "allowed-once" | "rejected" } } }
```
响应（**直接是 receipt，无信封**）：`{"accepted":true}` 或 `{"accepted":false, "reason":"bad-response"|"not-pending"}`。
- `accepted !== true` → 前端提示"审批应答未送达"并保留弹窗。
- allowed-once 唤醒 loop 执行；rejected 拒绝。超时（600s 兜底）后端自动 Rejected（**不广播 resolved**，前端靠本地移除）。
- 疑问取消用 `result:{ok:false, error:{code:"cancelled"}}`。

### 2.7 认证

- 默认密码 `adminadmin`；改密 `auth.changePassword{currentPassword,newPassword}`（<4 报 `password-too-short`）。
- token 时长 30 天，`sessions.jsonl` 持久化（重启保活）；`auth.status` 探测 `{authenticated}`。

---

## 3. RPC 全量方法与前端当前使用面

> 这张表是**唯一事实源**。前端当前只用下面前半段；后半段是后端已有但 UI 未接（**重写时可选用**，见 §6）。

### 3.1 前端当前实际使用（实现优先级：必须）

| 方法 | 请求 payload | 成功 value | 说明 |
|---|---|---|---|
| `auth.status` | `{}` | `{authenticated}` | 启动探测 |
| `auth.login` | `{password}` | `{token}` | 存 localStorage；失败 `wrong-password` |
| `auth.logout` | `{}` | `null` | 幂等 |
| `auth.changePassword` | `{currentPassword,newPassword}` | `null` | 设置页 |
| `session.list` | `{}` | `{items:[{sessionId, updatedAt, running, blank, cwd}]}` | **updatedAt 恒假值 1970-01-01**，别用 |
| `session.create` | `{}`（可选 sessionId/cwd/workspaceId） | `{sessionId}` | 无 payload 则服务端生成 uuid |
| `session.history` | `{sessionId}` | `{events:[{event}], projections:{asOfSeq, values}}` | 含快照投影 |
| `session.prompt` | `{sessionId, content:[{type:"text",text}]}` | `{accepted:true}` | **异步非流式**：HTTP 立即返回，回合后台跑，增量走 WS |
| `session.selectModel` | `{sessionId, provider, model}` | `{selected:{provider,model}}` | 会话级模型选择（advisory） |
| `session.models` | `{sessionId}` | `{current:{provider,model}, groups:[...]}` | 模型下拉数据源 |
| `host.describe` | `{}` | `{version, cwd, provider, model, attachedSessions}` | 状态栏心跳 |
| `host.listWorkdir` | `{path}`（相对，空=根） | `{path, workdir, entries:[{name,path,isDir,size,hidden}], truncated}` | 目录优先+名排序，单目录≤2000 |
| `host.readFile` | `{path}` | `{path, content, size}` | UTF-8 ≤2MiB |
| `host.writeFile` | `{path, content, overwrite?}` | `{path, overwritten}` | 原子写；缺省不覆盖 |
| `host.createWorkdirDirectory` | `{path, name}` | `{path}` | name 单段 |
| `llm.providers` | `{}` | `{providers:[{provider, displayName, settingsNs, settingsPath, active}]}` | 设置页模型区 |
| `llm.discoverModels` | `{settingsNs}` | `{models:[{id, name, contextWindow?, maxTokens?}]}` | 发现模型 |
| `settings.describe` | `{}` | `{namespaces:[{ns, value, applies:"restart", secrets, revision}]}` | 前端取 host/compaction/provider ns |
| `settings.update` | `{ns, patch, expectedRevision?}` | `SettingsNamespaceView` | patch 深合并；host.workdir 校验绝对路径 |
| `credentials.describe` | `{refs:[...]}` | `{credentials:{ref:{configured, writable}}}` | 值永不出域 |
| `credentials.set` | `{ref, value}` | `{}` | ref 形如 `OPENAI_API_KEY` |
| `goal.create` | `{sessionId, objective(≥1), maxGoalRounds?}` | `{ref:{id, revision:1}}` | |
| `goal.edit` | `{sessionId, ref:{id,revision}, objective?, maxGoalRounds?}` | `{ref}` | 至少改一项 |
| `goal.pause` / `goal.resume` / `goal.complete` | `{sessionId, ref}` | `{ref:{id, revision+1}}` | CAS；冲突 `goal-conflict` 静默等投影 |
| `goal.clear` | `{sessionId, ref}` | `{cleared:true}` | 投影写 null 墓碑 |

### 3.2 后端已有、前端未接（重写可选用；实现优先级：可选/后做）

- `session.cancel {sessionId}`（abort 当前回合）——适合加"停止生成"按钮。
- `session.search {query}`、`session.fork {sessionId, atSeq?}`、`session.rename {sessionId, title}`、`session.export`（GET 下载 ZIP）。
- `workspace.*`（list/create/rename/delete/insertBefore/insertSessionBefore/archiveSession）——侧栏可能的分组结构。
- `host.listDirectory`/`createDirectory`/`pickDirectory`/`openPath`、`credentials.unset`、`settings.replace/mutate`、`llm.models`。
- `/api/events.host` WS 流（workspace/session 状态增量，可驱动会话列表 running 态实时刷）。
- `plugin.core.list`（插件清单）、`agentPreset.*`（preset 管理）、`skill.list`、`subagent.*`（子 agent 管理）——多为后端 stub/未实装，慎重。

---

## 4. 页面/交互规格（功能主线）

### 4.0 应用壳（App）

> 布局**不指定分栏**：下面只列"需要哪些窗口/元素"，怎么排版由实现者自选（当前方案是把聊天 + 文件并排组成 dockview 双面板，仅供参考）。

**登录门**：
- `authed: boolean | null`。null → 全屏「载入中…」；false → 登录窗口（§4.1）；true → 主界面。
- 启动 flow（mount 一次）：① `rpc("auth.status",{})` → `authenticated===true` 直进；`AuthRequiredError` → false；`message.includes("auth-not-available")` → true（免登录）；其它异常 → 保持 null。② `session.list` → 启动恢复最近会话（§4.5）。

**导航**：左侧窄图标栏（含入口）：
- 顶部：`聊天`（对应聊天视图）
- 底部：`设置`（对应设置视图）、`退出登录`（danger 强调）
- 当前方案无"编程"入口（§1.3 注明 CodingApp 本次不做）；若你希望给未来留位可空着，本次不实现。

**窗口组成**：
- **聊天视图**：会话列表窗口 + 聊天窗口 + （可选）文件窗口（见 §4.2）。
- **设置视图**：设置窗口（全页替换主区）。
- **全局常驻**：底部状态栏（§4.5）+ 全局审批弹窗（§4.5）。

**失败边界**：后端不可达 → 永久 `boot-screen`（authed=null）；建议新前端加全局 ErrorBoundary + 连接错误提示（当前没有）。

### 4.1 登录页（Login）

- 居中卡片：标题"BoenMind"，副标题"请输入密码以继续（默认 adminadmin，首次登录后请在设置中修改）"。
- 一个 `Input.Password`（LockOutlined 前缀，autoFocus，必填）+ 登录按钮（primary block，loading=busy）。Enter 提交。
- 提交：`auth.login{password}` → 成功存 token + 进主壳；失败 `wrong-password` → "密码错误"，否则显示原始 message（Alert error）。
- 无"记住密码/验证码/找回"。Token 由 App 统一管理。

### 4.2 聊天视图（会话列表 + 聊天 + 文件可选）

> 布局自由：当前方案是 dockview 把「聊天」与「文件」做成可拖拽/折叠/悬浮的两个面板——仅参考，你自选排版（如文件窗口可做成可开关的侧栏）。

**会话列表窗口（SessionList）**
- 头部"会话" + `+`(title"新建会话") → `session.create` → 置顶插入并设为当前。
- 条目标题：`blank===true` → `新会话 · <cwd 取最后两级路径 / 连接>`（如 `新会话 · src/api`；无 cwd 只"新会话"）；否则 → `sessionId.slice(0,8)`。
- 状态：loading 用 Spin；空 → Empty"暂无会话"；**不能删除/重命名**（刷新失败静默，无错误 UI）。
- 列表项可选中（当前会话高亮）。

**聊天窗口（ChatPanel）**
- 自上而下：`MessageList`（滚动消息流，变化即滚底）→（有会话时）`GoalCard` → 错误条（若有 error）→ 输入卡片。
- 输入卡片（圆角卡片）：
  - 多行输入：placeholder 有会话"输入消息，Enter 发送 / Shift+Enter 换行"，无会话"请先选择或新建会话"；`disabled={!sessionId || streaming}`。Enter 发送（`!shiftKey && !isComposing`）。输入自动增高，上限 ~160px。
  - 工具条左下 hint：有字"`N 字符`"，无字"Enter 发送 / Shift+Enter 换行"。
  - 工具条右下（从左到右）：附件(PaperClip, **disabled**,title"附件（待实现）")；常用语言(Translation,**disabled**,title"待实现")；语音(Sound,**disabled**,title"待实现")；**模型选择**`Select`(value 形如 `provider::model::name`，选项按 provider 分组，onChange split(`::`)→`session.selectModel`)；**思考档位**`Select`(🐲 off/low/medium/high，**仅本地 state，不调 API**，tooltip"开发中")；**发送**（`disabled={!canSend}`）。
  - `canSend = text.trim()!=='' && !streaming`；提交调 `session.prompt`。
- **模型**：切会话 `session.models{sessionId}` → groups + current（默认 `mock-1`）。
- **消息渲染**：见 §2.4 约定。User 头像方块 accent 浅调、名"我"；Assistant 头像 accent 底白字、名"B"。气泡：assistant=面板色带边框、user=accent 混色右对齐。pending 消息闪烁光标。

**目标卡片（GoalCard，位于消息与输入之间）**
- 数据源：`session.history` 投影快照（`projections.values["goal"]`，快照后 `seqRef=0`）+ `useMuxEvent("session/projection")` 增量（key==="goal" && sessionId 匹配 && `seq > seqRef`；value null = 墓碑清空）。
- 阶段 Tag：active=绿"进行中"、paused=默认"已暂停"、blocked=橙"受阻"、complete=蓝"已完成"。
- 展示态：`🎯 目标` + Tag + `第 rounds/max 轮`（仅 active）+ 右侧"编辑"（disabled when complete/blocked）；objective 文本；active → 进度条 `pct=min(100, rounds/max*100)%`。
- 操作（全部 CAS，`ref` 来自投影；**失败静默、靠投影纠偏**）：
  - active → "暂停" `goal.pause`；paused → "恢复" `goal.resume`；非 complete/blocked → Popconfirm("完成并解除此目标？")包"完成" danger `goal.complete`；blocked → 灰字"受阻（等待处理）"。
- 新建空态：文本按钮"🎯 新建目标"→ 表单（objective TextArea 2 行 + 轮次 number min1 max64 默认 8 + 创建/取消）；`goal.create{sessionId, objective, maxGoalRounds:max(1,round(maxRounds))}`。
- 编辑态：同表单预填 + "保存"/"取消"；`edit()` 只有改了一项才调 `goal.edit`（maxGoalRounds max≥1），无变化直接退出。

### 4.3 文件 / 工作目录窗口（FileManagerUnit）

**不是占位**：真实可用（浏览/预览/编辑/上传/新建文件夹；**删除是占位**——点击仅 toast"删除功能待后端支持"）。可放在聊天视图的一个独立面板，也可做成可开关侧栏（当前方案是 dockview 右侧面板）。

- 初始：`settings.describe` → `host.workdir`；无 → 空树；有 → `host.listWorkdir{path:""}` 过滤 hidden 建树。跨页同步：监听自定义事件 `bm-workdir-changed`（设置页保存目录时 `window.dispatchEvent`）→ 关预览 + 重刷根。
- **目录树**：antd Tree `loadData` 懒加载（展开才 `host.listWorkdir{path}`）；点击非目录 → 打开文件；右键 → 菜单：打开(文件)/下载(文件)/复制路径(`navigator.clipboard`) / — / 上传到此目录 / 新建文件夹 / 删除(**占位**)。
- **工具栏**：行1 HomeIcon + workdir 路径；行2 刷新 + 上传到当前目录(Upload，`disabled={!workdir}`，beforeUpload 返回 false 自己走 `uploadFile(dir, file)`，无覆盖开关) + spacer + 门型预览开关(DoubleLeft/Right，on 态 accent)；容器 <720px 窄模式 → 打开文件后预览覆盖树，出现"返回目录树"。
- **预览区**：header(返回+路径+✕)；空态 Empty（无 workdir 提示去设置）；图片 → `<img src={downloadUrl(path)}>`；可预览文本(md/txt/log/json/toml/yaml/rs/ts/tsx/js/jsx/css/html/py/sh) → `host.readFile`，md 用 ReactMarkdown **+rehypeSanitize**，其它 `<pre>`；读失败（>2MiB 等）→ 转"仅下载"视图。
- **编辑**：文本预览点"编辑"→ textarea（onChange 置 dirty）；"保存" → `host.writeFile{path, content, overwrite:true}`；dirty 守卫：返回/关闭前 `window.confirm("有未保存的修改，确定…吗")`。
- **新建文件夹** Modal：Input 名称单段 + 位置显示，`host.createWorkdirDirectory{path:parent, name}`。
- 边界：无重命名/移动/多选；上传不支持覆盖开关（409 提示文案来自 client.ts）；编辑无语法高亮。

### 4.4 设置窗口（SettingsPage）

> 形态自由：当前是全页视图（左导航 + 右内容随分区切换，改动即时生效），也可做成弹窗/模态；不影响功能规格。

样式：左导航 220px（设置标题 + Menu 五分区 + 底部"完成"按钮回 chat）；右内容区随分区渲染，改动即时生效。

**通用（general）**：
- 工作目录 Input + 保存 → `settings.update{ns:"host", patch:{workdir}}`（空=清空）+ dispatch `bm-workdir-changed`；desc"文件管理器以此目录为根（绝对路径）"。
- 界面语言 Select `defaultValue="zh-CN"` **disabled**（占位，desc"即将推出"）。
- 启动行为 Switch：`localStorage bm.autoRestore !== "0"`（默认开）→ `setAutoRestore`。
- 遥测 Switch **disabled**（占位）。

**模型与 API（models）**：
- mount：`llm.providers` → provider 列表；`settings.describe`（读各 ns baseURL）+ `credentials.describe{refs:[`${P}_API_KEY`]}`（读 configured）。
- 每 provider 一张 Card：显示 name + 启用 Tag + 等宽 id；Base URL 输入+保存（`settings.update{ns:settingsNs, patch:{baseURL}}`，空=恢复默认，desc"下一请求生效"）；API Key 输入+保存（`credentials.set{ref,value}`，Enter 同触发；desc"已配置（密钥不回显）"/"未配置——请求将报 MISSING_CREDENTIAL"）；发现模型按钮（`llm.discoverModels{settingsNs}`，已发现显示"重新发现（N）"）+ 默认模型 Select（**仅本地 state，不调 API**）。
- 底部纯文案：添加自定义 provider 写 config.toml 需重启（无添加 UI）。

**外观（appearance）**（详见 §4.4.1）：
- 风格 Segmented（黑白/卡通/玻璃）→ 即时应用；背景 Segmented（默认/渐变/图片）；图片背景 URL 输入+应用 + 上传(≤2MB→dataURL)；强调色 ColorPicker → 存 + 应用；玻璃透明度 Slider(20-95%，仅 glass 档显示)；字号 Slider(12-18px)。

**账号与数据（account）**：修改密码两框 + 修改（`auth.changePassword`，成功"✅ 密码已修改"清空，失败显示 ❌）；会话数据静态"本地"desc"本地存储（boenmind.db + settings.json）"。

**高级（advanced）**：
- **上下文压缩**（仅当 `settings.describe` 返回含 `compaction` ns 时渲染）：启用 Switch(enabled) + 水线 Slider 10-95%(watermark) + 尾部保留比例 Slider 2-50%(keepRecentRatio) + 尾部保留下限 number(keepRecentFloor≥256) + 中部压缩下限 number(minMiddleTokens≥128) + "生效时机：下一回合生效"静态 + 保存（`settings.update{ns:"compaction", patch}`，全数值钳制）+ 重置为默认（回出厂值并保存）。
- **工具审批**（只读展示）：危险工具红 Tag 列表（`host.run_command, code.compile, code.python, code.shell, web.fetch, goal.create, goal.update, schedule.create`）+ 安全工具等宽文本（`host.list_dir · host.read_file · host.write_file · goal.get · web.search · schedule.list · schedule.cancel`）+ 本会话豁免（按会话列 sessionId+工具名，空"暂无豁免"；有任一 → "清除豁免"按钮 `clearAllTrusted()`）。
- **重置布局**：desc"dockview 布局刷新后回到默认两栏" + 按钮 `location.reload()`。
- **关于**：静态 `v0.1.0`。

#### 4.4.1 主题系统（theme，三档风格 + 正交背景）

- **三档**（明暗维度已删除，勿引入新"明暗"档）：
  | id | 标签 | 定位 | antd | 主色 |
  |---|---|---|---|---|
  | `ant` | 黑白 | Graphite Editorial 浅石墨 | defaultAlgorithm | `#2563EB`，bg `#F5F6FA`，圆角 6px |
  | `cartoon` | 卡通 | Kraft Journal 暖米牛皮纸 | defaultAlgorithm | `#3E6B5E`，bg `#E8DDC9`，圆角 16/24 |
  | `glass` | 玻璃 | 深炭黑 + α 分层 + 白边 | darkAlgorithm | `#9AABB7`，danger `#FB7185` |
- 每档导出 `{antd, cssVars}`：`--bm-bg/bg-2/bg-3/bg-glass/panel-mid/border/border-strong/border-subtle/fg/fg-dim/accent/accent-hover/accent-2/accent-soft/danger/radius/font/blur-shell`；写 `<style id="bm-theme">:root{...}</style>`；dockview 桥接 `--dv-*`（active/inactive tab 颜色覆盖成 --bm 色系，否则非激活 tab 落回黑底）。
- localStorage keys：`bm_preset`、`bm_background`(JSON)、`bm_accent`、`bm_fontsize`(12-18 默认14)、`bm_glass_opacity`(0.2-0.95 默认0.68)。兼容旧 key `"mui"`→`cartoon`。
- 背景：`default`(跟风格档底色) / `gradient`(固定深色渐变) / `image`(URL 或 ≤2MB 上传 dataURL)；`applyBackground` 写 body。
- 防闪：`index.html` 内联脚本在 bundle 前按 `bm_preset` 写 `:root{--bm-bg}`。

### 4.5 全局壳（两个常驻组件）

**StatusBar（底部）**：
- 左：WiFi 图标（三弧+圆点 svg，class .ok 绿/.pending 黄/.down 红）＋ 文本 已连接/连接中…/连接断开；有版本 `v{version}`；有 provider `{provider}/{model}`。
- 心跳：mount 立即 + 每 15s `host.describe`；成功置 connected + 记录版本；新版本（localStorage `bm_seen_version` ≠ 当前）→ 闪烁"发现新版本 {v}"；失败 → disconnected。
- 右（Tauri 专属，浏览器隐藏）：检查更新→checking/ready（"下载更新 v"）→installUpdate/none/idle/error；都走 `invokeTauri("check_update"/"install_update")`。
- 有 `attachedSessions>0` 显示 `会话 {n}`。整条 `data-tauri-drag-region`。

**ApprovalModal（全局审批弹窗）**：
- 订阅 `useMuxEvent("approval/requested")`：无 approvalId/sessionId/toolName 忽略；命中豁免 `isTrusted(sessionId, toolName)` → 不弹窗直接 respond allowed-once；否则按 ts 排序入队（去重）。
- Modal：`closable=false, maskClosable=false, keyboard=false, width=420`，title"工具调用审批"。
- 内容：工具名（放大等宽）+ 若在危险名单加红色"危险"徽标 + `调用 {callId}` + reason（有则 accent-soft 底）+ 队列提示"另有 {n} 个待审批" + 豁免提示"本会话已信任 {n} 个工具（同名调用自动放行）"。
- 三按钮：`拒绝`(danger) / `本会话信任该工具`(trustTool→respond allowed-once) / `仅本次允许`(primary)。
- 应答调 §2.6；`accepted!==true` → warning notification 保留弹窗；rejected → info"已拒绝"；网络异常 → error 保留。成功后本地移除该条（`approval/resolved` 事件可选消费来兜底，当前实现没订阅也能工作）。
- **豁免表**（`approvalTrust.ts` 模块 store + `useSyncExternalStore`）：`Record<sessionId, string[]>`，localStorage `bm.approvalTrust` 持久化（隐私模式降级内存态）；`trustTool/clearTrusted/isTrusted/useApprovalTrust`；设置页高级可查看/清除。

### 4.6 编程 / 代码视图（CodingApp）——本次不做

当前源码里 CodingApp 是**纯占位**（自建 dockview 六区 + 全部硬编码示例数据，无任何后端调用、无交互）。本次重写**不含此视图**：导航不留"编程"入口，也不实现任何代码编辑/终端面板。若未来要做，再单独设计（它可以复用聊天/文件窗口的既有模式）。

---

## 5. 状态管理与数据流要点

- **模块级 store + `useSyncExternalStore`**（不用 redux/zustand，这两个都小）：当前会话 id（`sessionStore`）、审批豁免表（`approvalTrust`）跨组件共享且要即时更新（无论你用哪种布局方案，跨面板都要共享）。
- **WS 单例**：全局事件总线内部 `ensureStarted()` 单例（首次用才建连，2s 自动重连）；订阅 Set 分发。聊天流建议独立 hook 管理自己那条连接（可把两条合并成一条连接 + 统一 handler 分发——协议无阻碍，但**两条独立连接是已验证路径**，别冒险合）。
- **切会话清理**：`useChat` 在 sessionId 变化时重置 messages/error、`ws.close`、`session.history` 重建；GoalCard 同理重置并重读投影。
- **错误哲学**：RPC 失败大多抛 `Error`；审批/目标操作"静默 + 投影纠偏"；聊天 send 失败 setError 显示在 ChatPanel 错误条。

---

## 6. 已知坑 / 边界（实现时逐条对照）

1. **信封 method 必须逐字**匹配 path 尾段，否则 200 + `bad-request`（不是 4xx）。
2. **审批应答 key = 外层 rpcId**（掉坑：只传 payload 会恒 `bad-response`）。见 §2.6、坑 10。
3. **projection seq 去重**：快照不带 seq，增量 `seq > 水位` 才收；否则挂接错乱。
4. **session.list 的 updatedAt 是假值**（恒 1970-01-01）——**会话列表排序只能用服务端返回顺序**，别按 updatedAt 排序。
5. **`session.prompt` 是异步非流式**：HTTP 立返 `{accepted:true}`，回合后台跑；**别等 HTTP 响应里的消息**，全靠 WS `session/event` 增量刷新 UI。`agent-busy` = 上一回合还在跑，前端应禁输（streaming 态）。
6. **chunk 的 arguments 是增量字符串**：tool-call-delta 需累计拼接；`tool/call` 事件才是完整 arguments（JSON 字符串，展示时尝试 pretty-print，失败回退原样）。
7. **文件面失败无静默**：readFile 失败要 toast（"无法直接预览…已转为下载"）；上传 409/413 有专门文案。
8. **Markdown 安全**：文件预览必须 sanitize；聊天消息不 sanitize（保持现状，若要加强再议）。
9. **Tauri 能力必须优雅降级**：无 `window.__TAURI_INTERNALS__` 时所有 Tauri 分支隐藏/空操作，不能崩。
10. **主题/背景写入**：CSS 变量、body 背景、字号、防闪脚本要协同（防闪在 index.html，bundle 前生效）。
11. **goal-conflict**：并发下 CAS 失败常见，前端静默、等最新投影覆盖（别 toast 刷屏）。
12. **未装配功能要降级**：`--goal/--approval/--compact` 未开时，对应 UI 要么隐藏要么诚实提示（如审批徽标"装配 --approval 时生效"）。
13. **本地持久化键**：`bm_session_token / bm_recentSession / bm_autoRestore / bm_preset / bm_background / bm_accent / bm_fontsize / bm_glass_opacity / bm_approvalTrust / bm_seen_version`——重写沿用，别改键名（用户已有数据在）。
14. **dev 代理**：vite dev server proxy `/api` 与 `/ws`（实际 WS 连 `/api/events.mux`）到 3080；生产直接同源（后端服务 dist）。
15. **登录态边界**：后端不可达时启动探测异常 → authed=null 永久"载入中"——可加超时/重试 UI（当前没有）。

---

## 7. 验证清单（重写完成后的验收）

- [ ] `yarn dev` 起 5173（代理 3080），或 `vite build` 后由 web-server 静态服务 3080 直接打开。**看界面必须识图**（自检：视觉核对截图，不只看 DOM）。
- [ ] 未登录（没开 --auth）直接进主壳；`auth.status` 免登录路径命中。
- [ ] 新建会话、发送消息（文字显示、流式光标、工具调用卡、Markdown）、切会话历史重建、停止/空会话输入禁用。
- [ ] 审批：装配 `--approval` + 危险工具调用 → 弹窗 → 仅本次允许/拒绝/信任该工具；豁免命中自动放行；设置页豁免可查看/清除；刷新后豁免仍在（localStorage）。
- [ ] goal：新建/编辑/暂停/恢复/完成 + CAS 冲突时投影纠偏；刷新后投影快照正确（seq 去重不抖）。
- [ ] 文件/工作目录窗口：列目录/懒加载子树、预览(md/text/图片)、编辑保存、上传/新建文件夹、右键菜单、workdir 未设置提示、md sanitize 不弹 XSS。
- [ ] 设置窗口：工作目录保存 → 文件窗口 `bm-workdir-changed` 重载；模型/API 配置、外观三档 + 背景 + 强调色 + 字号 + 玻璃透明度即时生效；压缩策略保存；重置布局 = 布局回默认。
- [ ] 状态栏心跳 + 版本提示；会话数。
- [ ] Tauri（桌面壳）更新检查；浏览器下整块隐藏不崩。
- [ ] 后端跑 `cargo build --workspace && cargo test --workspace -- --test-threads=1` 全绿（前端改动不影响后端，但接口语义按 §2/§3 严格对齐）。

---

## 附：快速参考

- 后端默认端口 3080；`--auth` 开鉴权、`--goal` 开目标、`--approval` 开审批、`--compact` 开压缩、`--web-tools/--code-runtime/--schedule` 开对应插件。
- 前端当前版本 v0.1.4、后端 v0.1.0；`frontend/dist` 是后端静态服务的产物。
- 文档配套：本文档 + 后端契约在 `bm/web-server/src/`（rpc.rs/ws.rs/api.rs/api/*）；主题三档设计稿 `docs/themes/{heibai,karton,boli}/DESIGN.md`。