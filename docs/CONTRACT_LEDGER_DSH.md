# CONTRACT_LEDGER_DSH —— dsh 前端契约台账（已填满）

> 状态：**已填满**（实现清单 + 验收标准，全部条目从 dsh 源码逐字提取，可直接作为 Rust 兼容层的 conformance 勾销表）。
> 用途：实现清单 + 验收标准（v2 计划 §3.2/§3.4/§3.5）。来源：dsh `packages/` 源码，**逐条提取，逐条勾销**（勾销标准 = conformance harness 双后端 wire 轨迹 diff 一致）。
> 基线：`@deepseek-ai/dsh@0.1.0-rc.6`（2026-08-17 快照，与前端锁死同版本）。
> 提取基线：deepseek-harness master commit `47f9438`（`D:/96_CoderWorld/deepseek-harness`）。以下每个条目标注提取源（仓库相对路径）。
> 说明：本台账内的「任务简报数字」若与源码不符，一律以源码为准并在该条目标注「未在源码找到，待确认」。

## 1. 传输面（9 面 + 双栅栏）

> 全部 URL 前缀常量来自 `packages/client/connection/src/api-path.ts`：`API_PATH = '/api'`、`MUX_EVENTS_PATH = '/api/events.mux'`、`HOST_EVENTS_PATH = '/api/events.host'`。

| # | 面 | 逐字形状 | 提取源（dsh 源码路径） | 状态 |
|---|---|---|---|---|
| 1 | HTTP POST /api/\<channel\>/\<endpoint\>（RPC 上行） | 请求体 = `ClientRequest` 全形 `{type:"client-request", rpcId:<uuid>, method:<endpoint>, payload:<业务负载>}`；HTTP `POST`，头 `content-type: application/json`；响应体 = `ServerResponse` 全形 `{type:"server-response", rpcId:<回显>, result:{ok:true,value} 或 {ok:false,error}}`。业务错误永远 HTTP 200 + result.error；**HTTP 状态只表达载体**：404 未知路径、415 非 application/json（`content-type` 前缀 split(';') 后 trim 小写比对）、400 非 JSON 体、500 实现崩溃（`handler failure: ...`）。方法不匹配路径 → 200 + `bad-request`（message `method "<m>" does not match path "<p>"`）。信封解析失败的 rpcId 兜底：能从原始体捞到 string rpcId 则回显之，否则固定哨兵 `INVALID_REQUEST_RPC_ID = RpcId('invalid-request')`。channel 形如 `/api`（客户端 `CHANNEL_PATTERN=/^\/[A-Za-z0-9._~-]+$/`，endpoint 段 `/^[A-Za-z0-9_$.-]+$/`，禁止空段/`.`/`..`）。body 上限 `DEFAULT_MAX_REQUEST_BODY_BYTES = 160 * 1024 * 1024`（超限 413 + `connection: close` + `req.destroy()`）。 | `packages/client/connection/src/client/rpc.ts`、`packages/host/apiproxy/src/fetch/handler.ts`、`packages/client/connection/src/http-bridge.ts`、`packages/host/apiproxy/src/api/rpc.ts`、`rpc.schema.ts` | 已提取 |
| 2 | WS /api/events.mux（宿主→浏览器 下行 MuxFrame） | 升级即调 `api.events.mux({rpcId: fresh, payload:{}}, signal)`；每帧以 JSON 文本发 `ServerRequest` 全形 `{type:"server-request", rpcId, method:<frame.type>, payload:<MuxFrame>}`。**上行拒绝**：socket 收到任意 `message` → `websocket.close(1008, 'downlink only')`。流内异常 → 补发一帧 `stream/error`（fresh rpcId）再关。`WebSocketServer({noServer:true})`。 | `packages/client/connection/src/websocket-downlink.ts` | 已提取 |
| 3 | WS /api/events.host（宿主→浏览器 下行 HostFrame） | 同上，换 `api.events.host({rpcId: fresh, payload:{}}, signal)`，帧方法名 = HostFrame.type。同样 1008 拒绝上行。WS 升级前过信任栅栏，不通过 → `rejectWebSocketUpgrade` 发裸 HTTP `HTTP/1.1 403 Forbidden` + `Connection: close` + `Content-Length: 9` + 体 `forbidden`。**对 WS 路径的普通 GET（非升级）→ 426** `upgrade required`，头 `connection: Upgrade`、`upgrade: websocket`（见 connection 插件 apply）。 | `packages/client/connection/src/websocket-downlink.ts`、`packages/client/connection/src/index.ts` | 已提取 |
| 4 | 静态 SPA（webserver fallback seat） | 越界 403（`resolve(normalize(join(distRoot, pathname)))` 不在 distRoot 下，`sep` 比较含 Windows）；命中 distRoot 或 index.html → 200 `text/html`（走 index taps）；miss（ENOENT/EISDIR）→ 200 回退 index.html（SPA 路由）；命中文件 → 200，扩展名 MIME 表（.html/.js/.css/.svg/.json/.map/.webmanifest）查不到 → `application/octet-stream`；非 GET/HEAD → 405。 | `packages/host/frontend-static/src/index.ts` | 已提取 |
| 5 | GET /plugins/\<id\>/client.js（及 .map） | 仅 GET/HEAD，否则 405；path 经 `decodeURIComponent`；`/plugins/` + id + `/client.js` 或 `/client.js.map`（id 可含 scope 斜杠）；未知资源 → 404（含 HMR 行缺失时的 /plugins/events）；命中 → 200 `text/javascript; charset=utf-8`（.map → `application/json; charset=utf-8`），`cache-control: no-cache`；已注册但文件不可读 → **404（响亮失败，绝不回退 SPA 页）**。bundle 端点即 `WebBootEntry.url = '/plugins/<id>/client.js?rev=<rev>'`。 | `packages/client/modules/src/index.ts` | 已提取 |
| 6 | boot 3 槽 | `window.__DSH_BOOT__`：宿主在 index.html 的 `<head>` 首部注入 `<script>window.__DSH_BOOT__ = {json}</script>`（JSON 中 `<` 转义为 `\u003c`；无 `<head>` 则前置）。图结构 `WebBootGraph={rev, entries:[{id, url, rev, inject?, immediately?}]}`，rev 为整体 12 位 hex sha1。`window.__ModuleLoader__`：`ClientModuleSystem` 构造时安装 `{load(handoff:{id, factory})}`（factory 为 CJS 工厂闭包）；已安装（双 boot）或重复 id 注册 → 抛错。`window.__DSH_MODULES__`：AppWebEntry.run() 构造后写入 `ClientModuleSystem` 实例，`./client` wrapper 插件据此提供 `ctx.modules`。解析规则 `parseBootManifest`：rev/entries 缺失或行缺 string id/url/rev → 抛错（fail-loud）。 | `packages/client/modules/src/client/manifest.ts`、`packages/client/modules/src/client/system.ts`、`packages/client/modules/src/index.ts`（injectBootManifest）、`packages/client/web/src/boot.tsx` | 已提取 |
| 7 | POST /api/respond（审批/提问应答上行） | 请求体 = `ClientResponse` 全形 `{type:"client-response", rpcId:<服务端请求帧回显>, result:{ok:true,value:<应答负载>} 或 {ok:false,error}}`。HTTP 响应体 = `RpcReceipt`：`{accepted:true}` 或 `{accepted:false, reason:'not-pending'|'bad-response'}`。rpcId 路由到 pending 表（approval 先、question 后）；approval 应答负载须 `{sessionId, approvalId, outcome:'allowed-once'|'rejected'}` 且 approvalId/sessionId 与登记一致，否则 `bad-response`；question 应答须 `{sessionId, answer:{answers:[{id, selected, custom?}]}}` 并整批匹配原问题（数量、id、selected 唯一性、multiSelect 约束、option label 集合），不匹配 → `bad-response`；无登记 → `not-pending`。question 侧允许 `result.ok:false 且 error.code==='cancelled'` 表示用户取消（accepted:true）。HTTP 层：媒体类型非 JSON → 415；体非 JSON → 400；信封解析失败 → `{accepted:false, reason:'bad-response'}`。 | `packages/host/apiproxy/src/fetch/handler.ts`、`packages/host/apiproxy/src/api-proxy.ts`（respond 实现）、`approvals.schema.ts`、`questions.schema.ts` | 已提取 |
| 8 | GET /api/session.export（会话日志 ZIP 下载） | 无信封；query 参数 `sessionId`（必填）+ `includeDescendants`（恰好 `true`/`false`/缺省，其他值 400 `missing or invalid sessionId query parameter`）。支持 GET 与 HEAD。成功 → `content-type: application/zip`、`content-disposition: attachment; filename="dsh-session-<id>.zip"`（id 非 `[A-Za-z0-9_-]` 字符替换为 `_`），流式 ZIP：根 artifact（原文件名 `session.jsonl`）→ `subagents/<id>/<filename>` 各后代 → `media/<attachmentId>.<ext>` 去重媒体。错误：服务缺失 → 500；后端不支持 raw artifact → 501；根缺失 → 404（`session not found`）；根准备失败 → 500。 | `packages/host/apiproxy/src/fetch/handler.ts`、`packages/host/apiproxy/src/api/downloads.schema.ts`、`packages/host/apiproxy/src/session-export.ts`、`packages/host/apiproxy/src/api-proxy.ts` | 已提取 |
| 9 | SSE 备选（events.mux / events.host GET）+ /plugins/events HMR | fetch/handler 对 `GET /api/events.mux` 与 `GET /api/events.host` 直接答 SSE：开流先发注释行 `: connected\n\n`，每帧 `data: {ServerRequest JSON}\n\n`（`\n\n` 分帧，客户端只取 `data: ` 前缀行合并）；流中断 → 一帧 `stream/error`（fresh rpcId）后关闭；响应头 `content-type: text/event-stream`、`cache-control: no-cache`。/plugins/events（`EVENTS_ENDPOINT = '/plugins/events'`）：SSE 帧 `{type:'graph', graph:WebBootGraph}`（连接即发）+ `{type:'rebuilt', id, rev}`；非 GET/HEAD → 405；开流注释行同前；默认轮询 500ms。浏览器客户端（WebApiClient）优先走 WS：`https:`→`wss:`/其他→`ws:`；两 parse（serverRequestSchema + frameSchema）失败即丢帧不杀流。 | `packages/host/apiproxy/src/fetch/handler.ts`（sseResponse）、`packages/host/apiproxy/src/fetch/client.ts`（readSse）、`packages/client/connection/src/client/web-api-client.ts`（readWebSocket）、`packages/client/hmr/src/index.ts`、`packages/client/hmr/src/events.ts` | 已提取 |

> 注：骨架原行 7/8 的提取源写为 `packages/client/connection/src/fetch/handler.ts`——该路径在源码中**不存在**；respond / session.export 的实际实现位于 `packages/host/apiproxy/src/fetch/handler.ts` 与 `packages/host/apiproxy/src/api-proxy.ts`。

### 栅栏 A：Host/Origin 信任栅栏（api-request-trust.ts，完整逻辑）

源：`packages/client/connection/src/api-request-trust.ts`（判定函数 `isTrustedApiRequest(request, trustedHosts)`）+ `loopback-hostname.ts`。

1. **Host 头栅栏（DNS-rebinding 防御，对每个 /api 请求生效，无任何跳过捷径）**：
   - 无 `host` 头 → 拒绝（false）。
   - `host` 头经 WHATWG 解析（`new URL('http://'+authority)`）失败 → 拒绝。
   - hostname 必须满足其一，否则拒绝：
     - `isLoopbackHostname(hostname)`：`'localhost'` 或 `'[::1]'`，或恰好 4 段点分且首段 `127`、每段 1-3 位数字且 ≤255（即整个 127/8）。
     - `isTrustedAuthority(hostUrl, trustedHosts)`：与某 trustedHosts 条目匹配。条目判定规则：带显式端口 = 精确 `host:port`；无端口 = hostname 匹配任意端口。双方都经 WHATWG 规范化（大小写、冗余 `:80` 不改变判定）。
2. **Cross-site 栅栏**：`sec-fetch-site` 头 === `'cross-site'` → 拒绝（无论 Origin）。
3. **Origin 栅栏**：无 Origin → 放行（Host 栅栏已绑定）；有 Origin → 必须 `new URL(origin).host === hostUrl.host`（同一规范化），parse 失败 → 拒绝；字面 `"null"`（sandboxed iframe / file: 页的 opaque origin）→ 拒绝。
4. **配置边界**：`assertTrustedAuthority(entry)` 要求每条 `trustedHosts` 是 canonical bare `host[:port]`（经 WHATWG 解析后再序列化逐字不变，仅大小写除外）；任何会被解析器改写的形态（带路径、user@host、尾随空白、悬挂冒号、零填充端口、`0x7f.0.0.1`、百分号编码、未加括号 IPv6）在**插件加载时即抛错**。
5. 部署语义：`trustedHosts` 由部署提供（dsh CLI 自行推导本机 LAN IP 字面量）；`0.0.0.0` 部署不声明将 403。栅栏是 DNS-rebinding/跨站防御，**不是认证层**。
6. 应用点：`packages/client/connection/src/index.ts` 的 `/api` prefix 路由先过栅栏（不过 → `403` `forbidden`）；WS 升级同样先过栅栏（不过 → 裸 403）；`rpc-host.ts` 中通用 channel（`handle`）与 `/api` interceptor 也按其 `authority` 选项过栅栏（`'loopback'` 通道以空 trustedHosts 过）。HTTP `content-type` 非 `application/json` → 415（强制 CORS 预检，跨站盲写防线）。

### 栅栏 B：PRIVILEGED_METHODS 特权方法（loopback-pin）

源：`packages/client/connection/src/index.ts`（`PRIVILEGED_METHODS`，`apply()` 内：`pathname.startsWith('/api/')` 取方法段，命中特权表且 `!isTrustedApiRequest(request, [])` → `403` `forbidden`；即以**空 trustedHosts** 过栅栏 = 强制 loopback）。共 **15** 个，逐字如下（任务简报称 16，源码实为 15，**待确认**）：

```
agentPreset.read
agentPreset.copy
agentPreset.openDocument
agentPreset.remove
host.pickDirectory
host.openPath
settings.describe
settings.openDocument
settings.update
settings.replace
settings.mutate
credentials.describe
credentials.set
credentials.unset
llm.discoverModels
```

源码注释载明的取舍：`llm.providers` / `llm.models` 故意不在列（只带 provider id/display name/模型清单，无端点/密钥/密钥状态，LAN 客户端的模型选择器合法需要）；`agentPreset.list` 与 `agentPreset.select` 不在列（选预设与 `session.create` 携带 agentPreset 等价，会话创建权限已涵盖）。

## 2. RPC 方法面（RpcMethodMap，源码实为 **52** 个 client-request 方法）

源：`packages/host/apiproxy/src/api/rpc-map.ts`（52 键，方法名 = wire 路径段 `POST /api/<method>`）+ 各域 schema（`*.schema.ts`）+ 各域 contract（`*.ts`）。任务简报称 55，源码实为 52（**待确认**）。`respond` 是 client-response（不在 RpcMethodMap）。六宿主概念中 **jobs 没有 RPC 方法**——jobs 仅作为 `JobView` 出现在 mux 帧 `session/jobs`（见 §3），其形状来自 `packages/host/apiproxy/src/api/jobs.ts` + `jobs.schema.ts`。

统一信封（所有方法）：请求 `{type:'client-request', rpcId, method, payload}`，payload 见下表；响应 `{type:'server-response', rpcId, result}`，result.value 形状见下表（`ok:false` 时 error 码见 rpc.ts `RpcErrorDetailsMap`，下表列主要错误码）。

### workspace.*（7）
| 方法 | 请求 payload | 响应 value | 关键行为/错误 |
|---|---|---|---|
| workspace.list | `{}` | `{items: WorkspaceView[], archivedSessionIds: SessionId[]}` | WorkspaceView=`{workspaceId, path, title, sessionIds, createdAt, updatedAt}`（createdAt/updatedAt 为 ISO-8601 string） |
| workspace.create | `{path: string}` | `{workspace, created: boolean}` | 对**已存在目录**建/幂等解析；目录缺失/非目录 → `workspace-invalid-path`；已属某 workspace 则返回该 workspace（created:false） |
| workspace.rename | `{workspaceId, title}`（title trim 后非空） | `{workspace}` | 未知 id → `workspace-not-found`；标题冲突 → `workspace-name-conflict`；改回原名 = 空操作成功 |
| workspace.delete | `{workspaceId}` | `{deleted: true}` | 仅删注册，目录/文件/日志不动；未知 id → `workspace-not-found` |
| workspace.insertBefore | `{workspaceId, beforeWorkspaceId?}`（省略锚点 = 追加末尾） | `{workspaceIds: WorkspaceId[]}`（完整显示序） | DOM-insertBefore 语义 |
| workspace.insertSessionBefore | `{workspaceId, sessionId, beforeSessionId?}` | `{workspace}` | 未知 workspace → `workspace-not-found`；session/锚点不在账 → `workspace-move-invalid`；原位移动 = 空操作成功 |
| workspace.archiveSession | `{sessionId}` | `{archivedSessionIds: SessionId[]}`（完整新集合） | 会话既非 live 也不在持久化 → `session-not-found`；幂等 |

### goals.*（6）
| 方法 | 请求 payload | 响应 value | 说明 |
|---|---|---|---|
| goal.create | `{sessionId, objective(≥1 字符), maxGoalRounds?(正整数)}` | `{ref: {id, revision(正整数)}}` | 建并武装目标 |
| goal.edit | `{sessionId, ref, objective?, maxGoalRounds?}`（至少一个） | `{ref}` | 不改阶段 |
| goal.pause | `{sessionId, ref}` | `{ref}` | 停用自动续跑 |
| goal.resume | `{sessionId, ref}` | `{ref}` | 恢复武装 |
| goal.complete | `{sessionId, ref}` | `{ref}` | 完成并解除 |
| goal.clear | `{sessionId, ref}` | `{cleared: true}` | 留墓碑与历史 |

> 注：goal 域**无读方法**——当前目标状态只在 `'goal'` session 投影（history tail 的 projections block + `session/projection` 帧）上走。

### skills.*（1）
| 方法 | 请求 payload | 响应 value |
|---|---|---|
| skill.list | `{sessionId}` | `{skills: [{name, description, whenToUse?, modelInvocable}]}` |

> 注：技能调用无独立 wire——就是 `session.prompt` 前导 `/name` 令牌。

### agentPresets.*（6）
| 方法 | 请求 payload | 响应 value | 说明 |
|---|---|---|---|
| agentPreset.list | `{}` | `{presets: [{id, trust:'system'\|'user', isDefault, name?, description?, broken?}], authorable: boolean, hasDocument: boolean}` | 未特权 |
| agentPreset.select | `{sessionId, agentPreset}` | `{agentPreset}` | 仅 blank 会话可换；已开跑 → `agent-preset-locked`；未知 id → `agent-preset-not-found` |
| agentPreset.read | `{agentPreset}` | `{agentPreset, trust, content: string, name?, description?}` | 特权（loopback） |
| agentPreset.copy | `{from, agentPreset, name?}` | `{agentPreset}` | 特权；唯一 authoring 写 |
| agentPreset.openDocument | `{agentPreset}` | `{opened: true} 或 {opened: false, path}` | 特权；无原生 opener 时回 path 文本 |
| agentPreset.remove | `{agentPreset}` | `{}` | 特权；内置 preset 拒绝（`agent-preset-read-only`） |

### subagent.*（4）
| 方法 | 请求 payload | 响应 value |
|---|---|---|
| subagent.list | `{parentSessionId}` | `{entries: SubagentListEntry[], parentAvailable: boolean}`；SubagentListEntry = child(one-shot: `{kind:'child', id, mode:'one-shot', activity:'running'\|'inactive', hasChildren, label?}` / continuable: `{..., mode:'continuable', label(必填)}`) 或 diagnostic(`{kind:'diagnostic', id, reason:'corrupt'\|'unsupported'\|'unavailable'}`) |
| subagent.history | `{parentSessionId, childSessionId, mode:'one-shot'\|'continuable', beforeSeq?, maxMessages?}` | `{events: [{event, view?}], hasMore, projections?}` |
| subagent.prompt | `{parentSessionId, childSessionId, mode:'continuable', content: ContentBlock[], clientTimeZone?}` | `{messageId}` |
| subagent.interrupt | `{parentSessionId, childSessionId, mode:'continuable'}` | `{accepted: true}` |

### llm.*（3）
| 方法 | 请求 payload | 响应 value |
|---|---|---|
| llm.providers | `{}` | `{providers: [{provider, displayName, settingsNs, settingsPath: string[], active, declared?}]}` |
| llm.models | `{}` | `{groups: ModelProviderGroup[], failures: ModelCatalogFailure[]}`（与 session.models 同构，无 per-session selection） |
| llm.discoverModels | `{settingsNs, provider?, baseURL?, api?, apiKey?}`（apiKey 写后即弃，绝不停留/返回） | `{models: [{id, name?, contextWindow?, maxTokens?}]}`；失败 → `model-discovery-failed {settingsNs, baseURL?}` |

### session.*（12）
| 方法 | 请求 payload | 响应 value | 关键行为/错误 |
|---|---|---|---|
| session.list | `{cursor?}`（v1 预留未实现） | `{items: SessionSummary[]}`；SessionSummary=`{sessionId, updatedAt, running, blank, parentSessionId?, origin?, cwd?, agentPreset?, projections?}` | updatedAt 降序 |
| session.search | `{query}`（trim 后 1-500 字符、禁 NUL） | `{items: [{sessionId, snippet}], hasMore}`；结果 ≤20、snippet ≤240 code points | `SESSION_SEARCH_RESULT_LIMIT=20`、`SESSION_SEARCH_SNIPPET_MAX_CODE_POINTS=240` |
| session.create | `{workspaceId?, cwd?, sessionId?, agentPreset?}`，workspaceId/cwd 至多其一 | `{sessionId, agentPreset?}` | 预分配 sessionId 重试幂等；cwd 冲突 → `session-conflict`；attach 失败 → `workspace-attach-failed`；preset 未知 → `agent-preset-not-found` / 挂载失败 → `agent-preset-invalid` |
| session.history | `{sessionId, beforeSeq?, maxMessages?}` | `{events: [{event, view?}], hasMore, projections?}` | 消息边界分页；tail 页才带 projections；默认 maxMessages=50 |
| session.models | `{sessionId}` | `{current: {provider, model, reasoningEffort?}, routable: boolean, groups, failures}` | subagent → `agent-busy` |
| session.selectModel | `{sessionId, provider, model, reasoningEffort?}` | `{selected: {provider, model, reasoningEffort?}}` | catalog 成员关系仅 advisory |
| session.rename | `{sessionId, title}` | `{title(≥1), seq(整数≥0)}` | 追加 `session/title`（source user）；归一化空 → `title-invalid`；subagent → `agent-busy` |
| session.fork | `{sessionId, atSeq?}` | `{sessionId}` | atSeq 锚定第一个 ≥atSeq 的 turn/end；in-log 锚点 turn 未闭 → `fork-unavailable`；省略/越界回退最后完成 turn |
| session.prompt | `{sessionId, mode:'queue'\|'steer', content: [{type:'text',text} 或 {type:'image',mediaType,data(base64 canonical),name?}], clientTimeZone?}` | `{accepted: true, command?: {kind:'success', text?}}` | 单 text 块前导 `/` = slash 命令（mode-agnostic）；命令使用/状态错误 → `command-error`；未知命令 → `unknown-command`；媒体类型限 image/png\|jpeg\|webp\|gif；base64 非 canonical → 拒绝 |
| session.attachment | `{sessionId, attachmentId}` | `{attachment: ImageAttachmentRef, data: string}` | 仅当会话日志引用该 id 才回 |
| session.updateQueue | `{sessionId, itemId, action:{kind:'edit',content} 或 {kind:'remove'} 或 {kind:'steer'}}` | `{accepted: true}` | 丢 item → `queue-item-not-found`；subagent → `agent-busy` |
| session.cancel | `{sessionId}` | `{accepted: true}` | 停 active turn，保留 pending inbox |

### settings.*（5，全部特权）
| 方法 | 请求 payload | 响应 value |
|---|---|---|
| settings.describe | `{}` | `{writable, hasDocument, namespaces: SettingsNamespaceView[]}`；SettingsNamespaceView=`{ns, schema(序列化 schemastery), value(脱敏), base?, user?, applies:'live'\|'restart', secrets:[{path: string[], set}] , revision: number}` |
| settings.openDocument | `{}` | `{opened: true}` |
| settings.update | `{ns, patch: object, expectedRevision?}` | `SettingsNamespaceView`（新脱敏视图） |
| settings.replace | `{ns, section: object, expectedRevision?}` | `SettingsNamespaceView` |
| settings.mutate | `{ns, ops: [{op:'set',path,value} 或 {op:'unset',path}], expectedRevision?}` | `SettingsNamespaceView` |

> 错误：schema 校验/未知 ns/只读 provider/存储失败 → `settings-rejected {ns}`；ns 在配置平面边界外 → `settings-not-exposed {ns}`；`expectedRevision` 落后 → `settings-conflict {ns, expected, actual}`。脱敏铁律：`role('secret')` 字段永不随任何响应出域，`secrets` 槽只报路径+是否已配置。Web 可写命名空间白名单（api-proxy.ts `WEB_SETTINGS_NAMESPACES`）：`agent-loop, shell, locale, permission, ui-conversation, ui-theme, web-search-deepseek`；产品命名空间额外允许 `ui-onboarding` 与 agent-preset 命名空间。

### credentials.*（3，全部特权）
| 方法 | 请求 payload | 响应 value |
|---|---|---|
| credentials.describe | `{refs: string[]}`（≤64，正则 `/^[A-Za-z_][A-Za-z0-9_]*$/`，非法名 → bad-request） | `{credentials: Record<ref, {configured, source?, writable}>}`（永不带值） |
| credentials.set | `{ref, value(≥1)}` | `{}` |
| credentials.unset | `{ref}` | `{}`（无引用也成功） |

> 错误：写被只读层（live env）遮蔽 → `credential-rejected {ref}`。无枚举方法。

### host.*（5）
| 方法 | 请求 payload | 响应 value |
|---|---|---|
| host.describe | `{}` | `{version, cwd, provider?, model?, attachedSessions(≥0), canOpenPath}` |
| host.pickDirectory | `{}` | `{path: string \| null}`（null=用户取消；特权） |
| host.listDirectory | `{path?}`（缺省 = 家目录） | `{path, home, crumbs: [{name, path, hidden:false}], entries: [{name, path, hidden}], truncated}` | 
| host.createDirectory | `{path, name}`（name 单路径段、非空非 . 非 ..、无 / 或 \\） | `{path}`（创建后的绝对路径） |
| host.openPath | `{path(≥1)}` | `{opened: true}`（特权） |

### 错误码全集（`RpcErrorDetailsMap`，rpc.ts，共 35 码）
`bad-request {issues}`、`cancelled {}`、`session-not-found {sessionId}`、`model-unavailable {provider,model}`、`session-conflict {sessionId,requestedCwd,existingCwd?}`、`invalid-time-zone {value}`、`workspace-attach-failed {sessionId,workspaceId}`、`workspace-not-found {workspaceId}`、`workspace-invalid-path {path}`、`workspace-name-conflict {name}`、`workspace-move-invalid {workspaceId,sessionId,beforeSessionId?}`、`directory-unreadable {path}`、`directory-exists {path}`、`directory-create-failed {path}`、`directory-picker-unavailable {capability}`、`agent-preset-read-only {agentPreset,reason}`、`agent-preset-locked {sessionId,agentPreset}`、`agent-preset-conflict {sessionId,requestedPreset,existingPreset?}`、`agent-preset-not-found {agentPreset,available[]}`、`agent-preset-invalid {agentPreset,reason}`、`agent-busy {reason}`、`attachment-error {reason}`、`queue-item-not-found {itemId}`、`steer-unavailable {itemId}`、`command-error {}`、`unknown-command {}`、`settings-rejected {ns}`、`settings-not-exposed {ns}`、`settings-conflict {ns,expected,actual}`、`credential-rejected {ref}`、`model-discovery-failed {settingsNs,baseURL?}`、`title-invalid {sessionId}`、`fork-unavailable {sessionId}`、`subagent-parent-unavailable {parentSessionId}`、`subagent-not-found {parentSessionId,childSessionId}`、`subagent-catalog-diagnostic {parentSessionId,childSessionId,reason}`、`subagent-not-resumable {childSessionId}`、`subagent-unauthorized {childSessionId}`、`subagent-delivery-unavailable {childSessionId}`、`internal {}`。

## 3. 事件词汇（三层）

### 3.1 wire 层：MuxFrame（**10** 帧，源码事件数）与 HostFrame（**10** 帧）

源：`packages/host/apiproxy/src/api/events.ts`（类型）+ `events.schema.ts`（wire schema，逐字字段约束）。任务简报称 MuxFrame 13 / HostFrame 9，源码实为 10 / 10（**待确认**）。

**MuxFrame（payload 槽，载于 ServerRequest；method 字段 = frame.type）：**

| 帧 type | 逐字字段 | 性质 |
|---|---|---|
| session/event | `sessionId, event: SessionEvent, view?: ToolEventView` | 纯推送；view=`{for:'call'\|'result', view:{card: string}}` |
| session/subscribed | `sessionId, lastSeq: number`（= session.seq - 1；空日志 = -1 约定） | 订阅基线（open 时每 attached session 一帧） |
| approval/requested | `sessionId, approvalId, toolName, callId?, reason?` | **answerable server-request**（稳定 rpcId，重放复用） |
| approval/resolved | `sessionId, approvalId, outcome: 'allowed-once'\|'rejected'\|'cancelled'\|'unavailable'` | 纯推送 |
| question/requested | `sessionId, questions: [{id, question, header?, detail?, options?: [{label, description?}], multiSelect?, intent?: {kind:'plan-review', approve}}]`（≥1 项，wire schema 强制非空） | **answerable server-request**（rpcId = 问题稳定逻辑 id） |
| question/resolved | `sessionId, questionRpcId, outcome: 'answered'\|'cancelled'` | 纯推送 |
| session/queue | `sessionId, items: [{id, placement: 'queued'\|'steering'\|'context', message: {id, role, content, source: {kind}}}]` | 全量快照（入队/变更/认领/丢弃/重连收敛） |
| session/jobs | `sessionId, jobs: JobView[]` | 全量快照；JobView=`{id, kind(open string), label, status:'running'\|'stopping'\|'completed'\|'killed'\|'failed', detail?, startedAt, finishedAt?}` |
| session/projection | `sessionId, key: string, value: unknown, seq: number(≥0)` | 单投影单元变更推送；客户端 higher-seq-wins |
| stream/error | `error: RpcError` | 流终止帧 |

**HostFrame（payload 槽，同上）：**

| 帧 type | 逐字字段 | 触发 |
|---|---|---|
| host/session-added | `sessionId, blank, parentSessionId?, origin?: 'subagent', cwd?, agentPreset?` | session/created（blank 恒为 true，首个 running 时翻转） |
| host/session-removed | `sessionId` | session/disposed |
| host/session-status | `sessionId, running: boolean` | agent/status |
| host/agent-error | `sessionId, message` | agent/error（无轮次位置的实时失败唯一出口） |
| host/workspace-changed | `workspace: WorkspaceView` | 每次持久化 workspace 变更后全量新快照 |
| host/workspace-removed | `workspaceId` | 注册删除增量 |
| host/workspace-order-changed | `workspaceIds: WorkspaceId[]` | 重排后完整持久序 |
| host/archived-sessions-changed | `archivedSessionIds: SessionId[]` | 归档集每次持久化变更后全量 |
| host/remote-event | `event: string, args: unknown[]`（wire schema args 为 `z.array(z.unknown())`；类型层 JsonValue[]） | 白名单宿主事件逐字转发（见下） |
| stream/error | `error: RpcError` | 流终止帧 |

**白名单转发宿主事件**（`API_REMOTE_FORWARDED_EVENTS`，`packages/api/remotes/src/remote-events.ts`，共 11 个）：`agent-preset/selected`、`commands/change`、`credentials/updated`、`cordis/request-run`、`cordis/request-run-resolved`、`cordis/dynamic-package`、`cordis/dynamic-retract`、`cordis/inspect-query`、`cordis/inspect-query-resolved`、`llm/adapters-updated`、`settings/document-updated`。逐字转发，无投影/无脱敏/无改名；负载契约归各 owner 包的 cordis `Events` 声明。

### 3.2 持久化层：SessionEvent 信封与 46→44 清单

**信封**（`packages/core/session/src/types.ts` + `sessions.schema.ts` 的 `sessionEventSchema`）：`{type: string, seq: number(int≥0), time: number(epoch ms), data: unknown(wide), sourceEventSeqs?: number[], surfaceOp?: unknown(实际 'append' 或 {op:'replace',start,end}), ignorable?: true}`。surface 事件仅限 `user/message`、`assistant/message`、`tool/result` 三种（`SurfaceEventType`）可携带 `surfaceOp`/`sourceEventSeqs`。

**KNOWN_SESSION_EVENT_TYPES**（`packages/core/session/src/known-event-types.ts`，生成式，源码实为 **44** 种 = core **13** + 插件扩展 **31**；任务简报称 46 = core 14 + 插件 32，**未在源码找到，待确认**）。未知类型且无 `ignorable: true` 的事件在持久化读路径被拒读（可能由更新版 harness 写入）。

**core 13 种**（`packages/core/session/src/types.ts` 的 `SessionEventMap` 本体）与负载要点：

| 事件 type | 负载要点 |
|---|---|
| turn/start | `{turn: number}` |
| turn/end | `{turn, reason: TurnEndReasonMap}`（completed/aborted{reason}/blocked/error{LlmFailure}/max-tokens/interrupted） |
| step/start | `{turn, step}` |
| step/end | `{turn, step}` |
| user/message | `UserMessage`（source 区分人类/注入/目标轮；浏览器 wire 经 `user-rpc` source 带 `rpcId` 与 `clientTimeZone?`） |
| assistant/chunk | `{turn, step, chunk: StreamChunk}` |
| assistant/message | `{turn, step, message: AssistantMessage, usage?}` |
| tool/call | `{turn, step, callId, name, arguments: string(模型原始 JSON 文本，未解析)}` |
| tool/result | `{turn, step, message: ToolResultMessage, error?: {name, code}, meta?: JsonValue}` |
| todo/write | `{todos: [{content, status:'pending'\|'in_progress'\|'completed'}]}`（全量快照，last-write-wins） |
| request/header | `{header: {config, adapterDefaults?, system?, tools?}, reason:'initial'\|'resume'\|'change'}` |
| request/context | `{provider, model, contextWindow?}` |
| session/end-seed | `{}`（定位 LAST 一帧） |

**插件扩展 31 种**（owner 包）与负载要点：

| 事件 type | owner 包 | 负载要点 |
|---|---|---|
| agent/inbox/spliced | core/agent | `{target, start, removedCount?, inserted: UserMessage[], outcome?: 'canceled'}` |
| tool/code-dispatch-start | core/tools | `{subCallId, parentCallId, name, arguments}`（规范化后再记） |
| tool/code-dispatch | core/tools | `{subCallId, parentCallId, name, arguments, result}`（每个 start 恰有一个 settle，abort 也算 isError 结果） |
| compaction/start | compaction | `{compactionId, sourceCommandId?, turn: number\|null}` |
| compaction/summary | compaction | `{compactionId, sourceCommandId?, summary: ContentBlock[], shadowedRange, shadowedSeqs, shadowedTokenCount, provider, model, maxTokens?, usage?, rawOutput?, llmStreamCall}` |
| compaction/end | compaction | `{compactionId, sourceCommandId?, turn, error?}` |
| compaction/prune | compaction | `{shadowedRange, shadowedSeqs, shadowedTokenCount}`（替换事件必须紧随其后同步追加） |
| hook/invoked | hooks/hook-protocol | `{turn, point, dialect, matcher?, handlerId}` |
| hook/result | hooks/hook-protocol | `{turn, point, handlerId, decision, exitCode?, stderrSummary?, durationMs}` |
| command/run | interaction/commands | `{commandId, name, args?, source}` |
| command/done | interaction/commands | `{commandId, kind:'success'\|'error', text?, sourceEventSeq?}` |
| llm/retry | llm/llm-retry | LlmRetryEventData |
| llm/retry-started | llm/llm-retry | LlmRetryStartedEventData |
| schedule/change | schedule/schedule | ScheduleChange |
| feedback/record | feedback/command-feedback | `{text: string}` |
| goal/change | goal/goal | GoalChangeMeta（完整状态或清空墓碑） |
| permission/preset | interaction/permission-presets | `{preset: string}` |
| approval/asked | interaction/user-approval | `{id, toolName, callId?, reason?}` |
| approval/decided | interaction/user-approval | `{id, outcome: ApprovalOutcome}`（含 cancelled/unavailable fail-closed） |
| approval/policy | interaction/user-approval | `{policy, source?: 'delegation'}` |
| plan/mode | plan/plan-mode | `{active: boolean}` |
| agent-preset/selected | preset/agent-presets | `{agentPreset: string}` |
| sandbox/mode | sandbox/sandbox-policy | `{mode, source?: 'delegation'}` |
| session/title | session/session-title | SessionTitleEventData（latest-wins） |
| session/title-llm-request | session/session-title-llm | SessionTitleLlmRequestEventData（log-only 预派发） |
| subagent/descriptor | subagent/subagent | SubagentDescriptorData |
| tool-workflow/run-start | workflow/tool-workflow | ToolWorkflowRunStartData |
| tool-workflow/agent-start | workflow/tool-workflow | ToolWorkflowAgentStartData |
| tool-workflow/agent-end | workflow/tool-workflow | ToolWorkflowAgentEndData |
| tool-workflow/run-end | workflow/tool-workflow | ToolWorkflowRunEndData |
| web/deepseek-search-llm-request | web/web-search-deepseek | DeepSeekSearchLlmRequest（免密） |

### 3.3 扩展槽
- **session/projection**：投影单元变更帧（MuxFrame） + history tail 的 `projections` block `{asOfSeq(-1 空日志), values: Record<key, unknown>}`；客户端 per-session 通用值仓 higher-seq-wins。宿主注册的投影单元：`sessionListMetadata`、`imageLimits`（api-proxy.ts）及插件各自单元（如 `goal`）。
- **host/remote-event**：白名单宿主事件逐字转发（见 3.1 的 11 事件表）；消费面 `ctx.remote.$on`。

## 4. 语义细节（行为逐字对齐）

- **rpcId 回显校验**：发起方铸 rpcId（client-request 由客户端铸；server-request 由宿主铸——可应答帧用稳定逻辑 id，纯推送每帧新铸）。响应**必须**回显请求 rpcId，绝不新铸（`rpc.ts` 类型契约）。客户端校验：`createWebConnectionRpc.call` 与 `AbstractApiClient.callUnary` 均 `if (full.rpcId !== rpcId) throw`（`rpcId mismatch for <method>: sent ... got ...`）。信封 rpcId 不可读时，服务端以固定哨兵 `RpcId('invalid-request')` 回 200+bad-request（`fetch/handler.ts` 与 `rpc-host.ts` 两处同款）。rpcId schema 无最小长度（不透明回显令牌）。
- **事件序与 seq 语义**：会话内事件 seq 单调且**从 0 连续**（`seq = log.length` 契约，`packages/core/session/src/index.ts` `get seq()`；seed 校验要求 `snapshot.seq === index`）。`session/subscribed.lastSeq = session.seq - 1`（空日志 -1）。`session/end-seed` 为构造种子末尾标记，事件前 seq 来自 seed（replay/fork/resume），本进程首个 live seq = `firstLiveSeq`。history 分页：`beforeSeq` 向后翻、消息边界对齐（`MESSAGE_TYPES = {user/message, assistant/message}` + `isAppendSurfaceEvent`），chunks 经 `sourceEventSeqs` 分组不切消息；tail 页含 in-flight 半条。tool/result 的 view 配对靠 openCalls 表（turn/end 清空）+ 页内 backscan。`session/projection` 帧 seq = 投影单元 watermark，客户端按更高 seq 覆盖。
- **断连恢复语义**（`packages/client/connection/src/client/connection.ts`）：`ConnectionController` 双流（mux+host）泵送 + 指数退避重连。默认 `backoffBaseMs=500, backoffFactor=2, backoffMaxMs=10000`（抖动 cap/2..cap）。**严格就绪握手**：`host.describe` + 两流 `onOpen` 齐了才 `onConnected`，`streamOpenTimeoutMs=3000` 兜底（不等待也按已连接推进）。状态仅 `'connected'/'reconnecting'`（去重触发）。`stream/error` 帧在泵侧即断流并触发重连。重连=重开流+重取 history（`events.mux` 的 `since` 参数 v1 未实现、忽略）。**mux 重开基线**（api-proxy.mux）：每 attached session 一帧 `session/subscribed` → 重放仍 pending 的 `question/requested` 与 `approval/requested`（rpcId 原样复用——刷新恢复基线）→ 有 pending inbox 的会话补 `session/queue` 全量 → 有任务补 `session/jobs` 全量。host 流无基线（`workspace.list` 在重连侧重打底）。
- **WS 上行拒绝**：mux/host 均为 downlink-only。socket 收到任意客户端消息 → `websocket.close(1008, 'downlink only')`（政策违规）。升级请求未过信任栅栏 → 协议协商前裸 HTTP `403 Forbidden`（体 `forbidden`）。WS 路径的普通 GET（非升级）→ `426 upgrade required` + `connection: Upgrade`/`upgrade: websocket`。客户端 WS 收到二进制帧/解析失败 → 丢帧记日志不杀流。
- **SPA 兜底**：越界 403 / 非 GET/HEAD 405 / miss 回退 200 index.html（SPA 路由）/ 未知扩展名 `application/octet-stream`（见 §1 行 4）。
- **RPC 载体 HTTP 状态**：404（未知路径）/ 400（非 JSON 体）/ 413（超 `maxRequestBodyBytes`）/ 415（非 application/json 媒体类型）/ 500（实现崩溃）；业务错误一律 200 + result.error。`session.export`：400（query 非法）/ 404（根缺失）/ 500（服务缺失或准备失败）/ 501（后端不支持 raw artifact）。
- **上行/下行信封**：HTTP 上行 `client-request`、响应 `server-response`、应答 `client-response`、流帧 `server-request`（method = frame.type）；四元判别并集 `type` 字面量。客户端 rpcId 由 carrier 铸（业务不铸）。
- **pristine 会话模型**：`host.describe` 的 `attachedSessions` 为当前带 live agent 的会话数；`blank` 位 = 日志中无 `turn/start`，插件独立事件（命令/plan/title/goal）不清除。

## 5. 验收（conformance harness）

- [ ] wire 轨迹录制：dsh Node 后端 + 同一前端 → 请求/响应轨迹
- [ ] Rust 兼容层重放同一轨迹 → diff 一致
- [ ] 皮肤/第三方插件 UI 不改即用

### 双后端 wire 轨迹 diff 的具体比对点清单

录制器应按下面清单逐点断言，diff 失败即红：

1. **路径与方法**：`POST /api/<method>` 与 `GET /api/events.mux`、`GET /api/events.host`、`POST /api/respond`、`GET /api/session.export`、`GET /plugins/<id>/client.js`、`GET /plugins/events`（HMR）逐字符一致；HTTP 状态码含载体层 404/400/413/415/426/500。
2. **信封逐字段**：`type`/`rpcId`/`method`/`payload`/`result`/`ok`/`value`/`error.code`/`error.message`/`error.details` 字段名与嵌套逐字一致；`rpcId` 回显正确（含 `invalid-request` 哨兵分支）。
3. **55/52 方法全量**：52 个 RpcMethodMap 方法逐一录制合法请求→成功/错误响应；特别录制每域至少一个业务错误（如 `session-conflict`、`workspace-not-found`、`settings-conflict`、`agent-preset-locked`、`subagent-not-found`、`model-discovery-failed`）。
4. **MuxFrame 10 帧**：open 基线序（subscribed 逐会话 → pending approval/question 重放 → session/queue → session/jobs）、`session/event`（SessionEvent 信封 type/seq/time/data/sourceEventSeqs/surfaceOp/ignorable 原样透传，view 仅 tool/call 与 tool/result）、`session/queue` 全量快照、`session/jobs` 快照、`session/projection`（key/value/seq）、`approval/requested` 稳定 rpcId 跨重连不变、`stream/error` 形状。
5. **HostFrame 10 帧**：`host/session-added`（blank/lineage/origin/cwd/agentPreset 字段有无）、`host/session-status(running)` 翻转、`host/workspace-changed` 等全量快照帧、`host/remote-event` 11 白名单事件的 event/args 逐字。
6. **respond 双向**：`RpcReceipt` 的 `accepted:true/false + reason` 三分支（not-pending / bad-response）逐字；approval 应答与 question 应答的整批校验拒绝路径。
7. **session.export**：query 解析（includeDescendants 的 true/false/缺省与 400 分支）、响应头 `application/zip` 与 `content-disposition` 文件名、zip 内路径（`session.jsonl` / `subagents/<id>/...` / `media/<attachmentId>.<ext>`）、404/500/501 分支。
8. **WS 上行拒绝与栅栏**：对 mux/host socket 发消息 → close 1008 `downlink only`；伪造 Host（非 loopback 非 trustedHosts）→ /api 403 与裸 WS 403；`sec-fetch-site: cross-site` → 403；Origin 不匹配 → 403；15 个特权方法在 trusted-host 部署下仍 403。
9. **boot/plugins 面**：`/plugins/<id>/client.js` 的 405/404 分支、rev 参数存在性；`window.__DSH_BOOT__` 注入脚本的 `<head>` 首部位置与 `\u003c` 转义；`/plugins/events` 的 graph/rebuilt 帧。
10. **时序语义**：同会话 `session/event` 的 seq 严格递增且与 lastSeq 基线衔接；重连后的基线重放与 live 帧无重叠、无缺口；`session/projection` 按 seq 覆盖后的终态一致。

> 疑点登记：任务简报中「55 方法 / 特权 16 / MuxFrame 13 / HostFrame 9 / SessionEvent 46(core14+插件32)」与源码（52 / 15 / 10 / 10 / 44(core13+插件31)）不一致，均以源码为准并标「待确认」。
