# W1 实现规格:WEBUI 壳子地基——三栏布局 + 对话闭环(流式)

- 序列:WEBUI(W1 起;ADR-0014 技术路线;布局蓝本由用户 2026-09-01 指定)
- 状态:实现中
- 硬纪律:先合约后动工;每个组件组必须标注 assistant-ui 原型或「自有组件」

## 1. 目标与范围

**W1 = 对话闭环 + 三栏布局骨架**(可运行检查点):
- 用户在输入框发消息 → BoenMind Agent(自研,经 OpenAI 兼容插座)流式回复上屏
- 多轮对话历史保持;刷新页面后续接同一会话(localStorage 记忆)
- 三栏布局骨架完整呈现(图标栏/会话列表/对话区/工作区面板),列表与
  右面板 W1 为静态骨架

**W1 不做**(登记后续):会话列表真数据(W2)、设置页(W2)、右面板真数据
(W2)、停止生成按钮(W2,需自定义取消端点)、审批卡片(W3)、任务看板(W3)、
移动端适配(W3+)。

## 2. 架构

```
[壳子 runtime/webapp](Vite+React+TS 静态包)
   │  POST /v1/chat/completions (SSE 流式,OpenAI 兼容)
   ▼
[boenmind-server (Rust)] ──► RuntimeHandle.session_create / send_input
   │                            └► 自研 Agent 回合(model.chain 配置驱动)
   └◄ 事件日志轮询(ModelContentDelta/终态)──► SSE delta chunks
```

- 壳子为纯静态包,由 boenmind-server `--web-dir` 托管,单进程无 Node 依赖
- Agent 完整保留:会话/事件/审批/预算全部走既有 runtime,壳子只做投影

## 3. 设计系统(令牌表,蓝本=用户指定截图)

| 令牌 | 值 | 用途 |
|---|---|---|
| --bg-page | #ffffff | 对话区/主内容底 |
| --bg-panel | #fafafa | 会话列表/图标栏/右面板底 |
| --bg-hover | #f4f4f5 | 悬停态 |
| --bg-select | #eff6ff | 选中态(浅蓝) |
| --border | #e5e7eb | 全站 1px 细边框(以边代影) |
| --fg-1 | #171717 | 主文字 |
| --fg-2 | #525252 | 次文字 |
| --fg-3 | #a3a3a3 | 弱文字/占位 |
| --accent | #2563eb | 主色(选中/主按钮/链接) |
| --danger | #dc2626 | 危险动作(清空/删除) |
| --radius | 8px(卡片/输入框)/ 6px(小按钮) | 圆角 |
| --font-ui | system-ui, "Segoe UI", "Microsoft YaHei", sans-serif | 界面正文 13-14px |
| --font-mono | ui-monospace, Consolas, monospace | 技术身份特征:会话标题/路径/模型名/表头 |
| 密度 | 紧凑(列表项 32-36px 高) | 开发工具气质 |
| 布局 | 图标栏 52px + 会话列表 260px + 对话区自适应 + 工作区面板 320px | 三栏+右面板 |

空态口径:居中灰字一句(如「此会话暂无活动任务列表。」),无插画。

## 4. 后端合同:`POST /v1/chat/completions`(OpenAI 兼容插座)

- 请求:JSON `{ "model"?: string, "messages": [{role,content}...], "stream"?: bool }`;
  请求头 `X-Bm-Session`(可选,续聊时回传)
- 行为:取 messages **最后一条 user** 文本 → `send_input`;无 X-Bm-Session
  则先 `session_create`(模型=服务器配置默认,即 config/model.json 驱动);
  其余历史消息 W1 接受但忽略(会话历史由 runtime 侧维护),文档明示
- 响应头:`X-Bm-Session: <会话id>`(壳子存 localStorage 续聊)
- `stream:true` → SSE:`data: {"choices":[{"delta":{"role":"assistant"}}]}` 起手,
  每 ModelContentDelta → `data:{"choices":[{"delta":{"content":"…"}}]}`,
  终态 → `data: {"choices":[{"delta":{},"finish_reason":"stop"}]}` + `data: [DONE]`
- `stream:false` → 聚合完整 `chat.completion` 一次返回
- 失败 → OpenAI 错误形状 `{"error":{"message":…,"type":…}}`(HTTP 200 内
  或 5xx,按 OpenAI 惯例)
- 鉴权:W1 免鉴权(与 /api/* 同款**已登记欠账**,公网前必须补,沿
  ADR-0009 T-13/T-14)
- model 字段 W1 接受但忽略(单一配置模型);多模型路由随 W2

测试:协议级(流式帧形状/会话续接/非流式聚合/错误形状),mock 模型驱动。

## 5. 组件组 → assistant-ui 原型映射(合约核心)

| 组件组 | assistant-ui 原型 | 备注 |
|---|---|---|
| 对话消息流 | `ThreadPrimitive.Root/Viewport/Messages` | 滚动容器+虚拟列表由其承载 |
| 回到底部 | `ThreadPrimitive.ScrollToBottom` | |
| 用户消息 | `MessagePrimitive.Root`(role=user)+自有样式 | |
| 助手消息(流式) | `MessagePrimitive.Content`(自动流式) | Markdown 渲染器 W1 内置简版 |
| 空态欢迎 | `AuiIf condition={s=>s.thread.isEmpty}` | |
| 输入框 | `ComposerPrimitive.Root/Input/Send` | |
| 运行时接线 | `useExternalStoreRuntime({messages,setMessages,onNew,onCancel?})` | onNew=POST /v1(stream) |
| 会话列表(W2) | `ExternalStoreThreadListAdapter`(threadList adapter) | W1 静态骨架 |
| 审批卡片(W3) | 工具调用部件 + `onRespondToToolApproval`/`onAddToolResult` | 一等公民 |
| 图标栏/工作区面板/设置页 | **自有组件**(不在 assistant-ui 语义内,纯 React) | 依 §3 设计令牌实现 |

## 6. 验收门(W1 完成定义)

1. 后端协议测试全绿(流式帧形状/会话续接/非流式/错误形状)
2. 真实浏览器手测(铁律):发「你好」→ 流式回复可见;连发第二句 →
   两轮全部上屏;刷新 → 续聊同一会话;F12 无报错
3. 全仓 cargo 测试 + clippy + validate.py 全绿
4. 布局与蓝本逐栏对照(截图留档)
