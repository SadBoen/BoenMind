---
status: accepted
date: 2026-09-13
summary: 前后端解耦——聊天流只承载模型正文,工具/审批元数据走结构化事件(删内联文本标记)
supersedes: []
superseded_by: []
---
# ADR-0055: 前后端解耦——聊天流纯文本化与结构化元数据通道

- 关联: ADR-0014(W 系列 WebUI)、ADR-0029(回喂忠实性)、ADR-0054(内核去能力名)、ADR-0031(单写者总线)
- 背景: 内核把 UI 信息以**文本标记**塞进模型正文流——`[调用 …]`/`[工具完成 …]`(spawn.rs)、`[BM_APPROVAL:{json}]`(runtime.rs/handlers.rs);前端正则反解析(parser.ts/runtime.tsx)并**按工具名子串猜分类**。这是自定义的、无 schema、无版本的私有文本协议:任何第三方 OpenAI 兼容客户端都会在 `delta.content` 看到它,且前端分类是猜的。真正的结构化通道(`GET /events/{session}` 类型化 SSE、`/admin/*` REST)本已存在却闲置。

## 决策

1. **聊天流只承载模型正文**。删除内核三处内联标记(`[调用 …]`/`[工具完成 …]`/`[BM_APPROVAL:…]`);`Cmd::ApprovalRequested` 变体(仅为推标记而设)删除。
2. **工具事件走结构化通道**。新增合同事件 `capability.started`(payload: operation_id/capability/principal/effect/target,session_id 随 `CallContext` 携带,故经 `/events/{session}` 与 `/v1` 流按会话送达)。工具分类据 `effect`(manifest 风险声明,ADR-0054 同源),不再按名猜。
3. **审批事件补 args**。`approval.requested` 增 `args` 字段(审批卡渲染所需,原由内联标记承载)。前端经结构化事件直读,不再解析正文标记。
4. **交付通道 = `/v1` 流的 `bm_event` 命名帧 + 持久 `/events` SSE**。`openai_compat` 在既有 OpenAI 兼容流中额外下发 `{"bm_event":{"type","payload"}}`——独立命名,标准客户端忽略、只在后端留痕;前端按 `type` 分派工具卡/审批卡。参数不从 `delta.content` 猜。
5. **前端去镜像**(同批):删 `BUILTIN_DESC`(内置描述硬编码副本)、`classifyTool` 名字子串分类、描述文案反推审批——改直读后端 `description`/`plugin_kind`/`effect`。

## 后果

- 聊天流变纯文本:第三方 OpenAI 客户端得到的正文不再混入内核 UI 标记。
- 合同 Minor 增发:`capability.started` + `approval.requested.args`(纯追加,消费方须忽略不认识的字段,旧消费方无害)。
- 前端工具卡由事件驱动渲染;工具组仍**仅实时可见**(历史回放本就不含工具事件,行为不变)。
- `openai_compat::should_backfill_content` 不再需绕过标记污染(ADR-0029 相关注释更新)。
- 测试:`chat_direct_tool` 增守卫(断言 `capability.started` 已发且带 session_id;断言 `model.content.delta` 不含任何内联标记);e2e 工具用例改结构化帧驱动。
- 遗留(不在本 ADR 范围):上下文透视页仍从 system prompt 反解析 `[工作目录]`/`[附加技能]` 标记(recipe.ts)——属 prompt 反解析,另议。
