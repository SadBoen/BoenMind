# HANDOFF：前端对话渲染 gap 修复（2026-08-18）

> 状态：**挂账 #1（高优先）已修复并验证**。承接 `docs/HANDOFF_PLUGIN_BASE_2026-08-18.md`
> 的挂账 #1（前端对话视图不渲染消息，影响可用性）。本轮完成根因修复 + 四套补跑验证 +
> GUI 会话列表渲染验证。挂账 #2（插件分类分组折叠）后置。

---

## 1. 一句话交接

**前端"对话空白"根因 = WS 下行帧缺 `type` 判别字段**：官方前端 `web-api-client.ts`
`readWebSocket` 对每条 WS 帧执行 `frameSchema.parse(full.payload)`（校验 MuxFrame/HostFrame
判别联合），Rust 的 payload 不带 `type` → 解析抛错 → `[client-connection] dropping
malformed WebSocket frame` → **所有 mux 帧被前端静默丢弃**，对话/轨迹/会话列表全部空白。
修复 = payload 注入 `type` 判别字段 + wire 事件契约对齐官方 + workspace-changed 单
workspace 形状。验证：会话列表刷新后从空白变为完整渲染；mux 帧经 Node 客户端实测过
schema；全量测试/clippy/gate1/四套补跑全绿。

---

## 2. 根因链（GUI 实测定位）

1. **现象**：后端 81 事件 completed，前端对话/轨迹全空白（交接已记录）。
2. **逐层排查**：
   - `session.history` wire 事件 → 缺 `surfaceOp`/`id`/`source`（官方 Message 契约必需，
     前端 `isAppendSurfaceEvent` 与 `source.kind` 判断直接访问）→ **第一层修复**。
   - 修完仍空白 → 查前端消费链路 → 官方 `web-api-client.readWebSocket`：
     `frame = frameSchema.parse(full.payload)`，**payload 必须带 `type` 判别字段**
     （MuxFrame/HostFrame 判别联合），Rust 发 `{sessionId,event}` 无 type →
     解析抛错丢帧 → **第二层修复（真正根因）**。
   - 附带发现：`host/workspace-changed` 发全量 `{items:[...]}`，官方契约是
     `{workspace: WorkspaceView}` 单对象 → 一并修复（会话列表渲染依赖它）。

## 3. 修复清单

| 文件 | 改动 |
|---|---|
| `kernel/web-server/src/rpc.rs` | `ServerRequestFrame::new` 把 method 注入 payload 的 `type` 判别字段（对象时）。外层信封保持官方 `server-request` 四元判别；两层都过 schema |
| `kernel/web-server/src/events.rs` | wire 事件契约对齐官方：信封加 `surfaceOp`（append，仅 user/message、assistant/message、tool/result；`skip_serializing_if`）；`user/message` 加 `id`+`source.kind:'user'`（原 'human' 会被前端误分类为注入上下文）；`assistant/message` 的 message 加 `id`/`role`/`source.kind:'model'`；`tool/result` 改官方 ToolResultMessage 形状（`content:[{type:'tool-result',toolCallId,content,isError}]` + `source:{kind:'tool',callId}`）；translator 加 `emitted` 计数保证 id 跨 history/实时一致 + `with_emitted(seed)` 实时续数 |
| `kernel/web-server/src/api.rs` | `attach_event_bus` 预填用 `EventTranslator::with_emitted(seed)`（实时 message id 与 history 一致）；`host/workspace-changed` 全部改单 workspace 形状 `{workspace}`（create/rename/attach/insertSession）；delete 只发 `workspace-removed` 增量（官方语义，删全量快照帧） |
| `scripts/hot-replace-verify.mjs` | debug exe → release exe（debug 2GB 超 PE 限制，见 build-debug-exe-2gb-pitfall） |
| `.tmp/gate25-verify.mjs` | 断言过时：M3 对齐官方后回合序列 5→7 帧（`turn/start`/`assistant/chunk` 为 M3 新增，官方 agent-loop 实证一致）；改 7 帧基线 |

## 4. 验证矩阵

| 项 | 结果 |
|---|---|
| cargo test --workspace | **全过**（+events/rpc 断言更新） |
| clippy --workspace --all-targets -D warnings | 零警告 |
| verify-gate1.sh | ALL PASS |
| conformance（3081，17 条） | 17/17 PASS |
| gate25（3079 mock，更新后） | PASS（7 帧基线） |
| m3-r3（3079，40 条） | 40/40 PASS |
| hot-replace（3082，release exe） | ALL PASS（keyless→热补 key→热切 baseURL 全链路） |
| mux 帧形状（Node WS 直连实测） | payload 带 `type:"session/event"` + `event/sessionId`（官方 schema 兼容） |
| GUI 会话列表（真实 MiniMax，3080） | **修复前空白 → 修复后完整渲染**（工作区 BoenMind + 未分组 + 会话带时间戳） |
| GUI 对话消息渲染 | **未最终确认**——会话项点击被 IAB 限制卡住（环境问题，见 §5），wire 层已全通（真实回复 13 事件完整） |

## 5. 遗留/环境

- **GUI 对话消息渲染待用户确认**：本会话 IAB 点击会话项/工作区项持续失败（Playwright
  role/text 点击超时、dom_cua 对 treeitem 无效、截图 `browser screenshot activity capture
  failed`）——记忆 iab-browser-testing-limitations 已知限制。wire 层证据完整（mux 帧
  过 schema + 会话列表渲染 + history 13 事件），**下次真屏/浏览器确认对话视图**。
- **挂账 #2（前端插件按 category 分组折叠）**：数据层（plugin.core.list）已就位，
  前端 patch 需同步升级成本，后置。
- **坑实录**：JSON 序列化断言键序差异（serde_json Map 默认 preserve_order 不保证键序）→
  改语义比较；IAB 上 base-ui 交互限制导致 GUI 导航耗时长。

## 6. 文件地图（本轮）

- `kernel/web-server/src/rpc.rs` —— ServerRequestFrame type 注入
- `kernel/web-server/src/events.rs` —— wire 事件契约（surfaceOp/id/source/tool-result）
- `kernel/web-server/src/api.rs` —— attach_event_bus 种子 + workspace-changed 形状
- `scripts/hot-replace-verify.mjs` / `.tmp/gate25-verify.mjs` —— 验证脚本更新
