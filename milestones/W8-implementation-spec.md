# W8 实现规格:常规设置(工作区注册 + 运行环境探针)与工作目录会话绑定

- 序列:WEBUI W8(前置:W7 已收官;ADR-0014 技术路线、ADR-0018 决策)
- 状态:**已实现并通过真实浏览器可视化验收(2026-09-03,截图
  `milestones/shots-w8/`;验收门 1-5 逐条见 §6)**
- 需求来源:用户 2026-09-03 三条口述——①设置页左侧加「常规」,含工作目录管理与
  Python/Node.js 检测;②聊天输入框可选工作目录(上拉菜单,参考截图排版);
  ③LLM 无输出时空气泡不应提前出现。追问裁决:工作目录=「像项目一样,每个项目
  一个路径」(多目录注册,非单值);保存方式=按会话。

## 1. 数据与合同

### 1.1 工作区注册表(数据目录,不入 git)
- 文件:`<data-dir>/config/workspaces.json`
- 形状:`{"workspaces":[{"id":"ws_xxxxxx","name":"显示名","path":"绝对路径"}]}`
- `default` 条目:管理面首次 GET 时自动播种(现役文件浏览根),不可删除、可改名
  改路径;保证旧文件树/旧用法零破坏。
- 读取的唯一实现 = `bm-core::workspace`(读 + 按 id 解析);管理面 CRUD 复用同一
  读取并负责写盘(与 providers.json 同款壳子私用口径,W2 裁决)。

### 1.2 wire 合同 Minor(只增,ADR-0018)
- `wire/agent.v0_1.schema.json`:AgentSpec 增可选 `workspace_id`;
  send_input params 增可选 `workspace_override`。
- Rust 镜像:`AgentSpec.workspace_id`、`SendInputParams.workspace_override`
  (serde default + skip_serializing_if,缺省不出字段,旧载荷零影响)。
- 管理面 REST(/admin/workspaces、/admin/runtime/env)不入冻结合同(W2 先例)。

## 2. 后端

### 2.1 bm-core
- `src/workspace.rs`:read_workspaces(data_dir) / resolve(data_dir, id)。
- `Session.workspace_id: Option<String>`(内存;重启不保留,ADR-0018 决策 3);
  `handle_session_create` 校验(未登记 = validation_failed);
  send_input 应用 `workspace_override`(同上校验并更新会话绑定)。
- 回合组装:按会话绑定解析路径,追加 `[工作目录] <path>` 到 system prompt;
  解析失败静默跳过。

### 2.2 bm-surface-http
- 新模块 `src/workspace_admin.rs`:
  - GET /admin/workspaces(含播种 default + 每项 exists 探测)
  - POST /admin/workspaces {name,path}(校验:存在/是目录/canonicalize/去重)
  - PUT /admin/workspaces/{id}(改名/改路径,重校验)
  - DELETE /admin/workspaces/{id}(default 拒删)
  - POST /admin/workspaces/{id}/check(重新探测)
  - GET /admin/runtime/env(Python:python3→python→py -3;Node:node;各 5s 超时,
    无 shell;回传 installed/version/program/error)
- `openai_compat.rs`:body 可选 `workspace`(字符串,工作区 id);新会话→AgentSpec,
  续会话→workspace_override;校验失败 400 透出核心消息。

## 3. 前端(runtime/webapp)

- `api.ts`:workspaces CRUD/check + runtimeEnv 类型与调用。
- `storage.ts`:新增 `ACTIVE_WORKSPACE`(bm_active_workspace,最近选择)。
- `SettingsPage.tsx`:导航首位加「常规」(WrenchIcon);新 `GeneralPage.tsx`:
  - 上:Python / Node.js 两张环境卡(徽标 已安装/未安装、版本、program、错误) +
    「重新检测」;
  - 下:工作目录列表,**固定五行高度、超出滚动**;行 = 名称+路径(等宽)+可用徽标
    + 检测/编辑/删除;新增/编辑走 Dialog(名称+路径)。
- `thread.tsx` Composer:`🏠 Home` 占位升级为工作区 Select(上拉 side=top popper,
  与角色/模型同款;条目两行=名称+路径;哨兵 `__auto__`=不绑定);选择写
  ACTIVE_WORKSPACE,随每条消息发送。
- 空气泡修复(W8-3):`AssistantMessage` 无可见内容(纯空 text/空 parts)时不渲染
  `.text` 气泡容器,仅保留 model-tag;有正文/停止/连接失败文本照常。不改显式
  占位消息(appendDelta 锚点依赖它)、不改 SSE 协议与看门狗。
- `runtime.tsx`:sendUserText body 带 `workspace`(最近选择);400 且消息含
  「工作区」时清空本地选择防死循环。

## 4. 测试矩阵

- 合同:validate.py 全绿(两 schema 只增字段)。
- bm-core:未登记 workspace_id 拒绝;登记后创建/覆盖/回合注入(context-log 断言);
  缺 data_dir 宽容。
- bm-surface-http:workspaces CRUD+播种+default 拒删+重复路径拒绝;runtime/env
  形状;openai_compat workspace 字段两路径(新建/续聊)+未知 id 400。
- 前端冒烟(Playwright):常规页导航/探针卡/列表五行滚动;composer 工作区上拉、
  选中后请求体携带;空正文 SSE 不出现 `.msg.assistant .text`、正常正文不受影响。
- 硬纪律 7:真实浏览器手测验收(截图 milestones/shots-w8/),接口绿≠界面交付。

## 5. 验收门

1. 设置→常规:Python/Node 检测结果真实可见;工作目录可增/改/删/检测,列表五行
   固定高度带滚动;default 条目在且拒删。
2. 聊天:输入框工作区上拉可选;选择后下一条消息请求携带 workspace id;新建对话
   后换选另一目录互不影响;刷新后选择保持。
3. 模型侧:选定目录后模型 system prompt 含该目录(context-log/上下文页可见)。
4. 空气泡:发送后首 token 前无空 .text 气泡;空回复完成后仍无;停止/失败文本可见;
   正常流式回归不退化。
5. 回归:cargo test 全绿 + clippy 零警告 + fmt + validate.py 全绿 + 既有冒烟全绿。

## 6. 验收记录(2026-09-03 实测,截图 `milestones/shots-w8/`)

| 门 | 结果 | 证据 |
|---|---|---|
| 1 常规页 | **过** | 导航首位「常规 W8」;Python 3.13.15 / Node v24.20.0 真实检测(01-general-page.png);「添加」对话框登记演示项目成功;default 播种=文件浏览根且删除按钮禁用;列表容器固定 285px(5 行)带滚动 |
| 2 聊天选择 | **过** | composer 工作区 chip 上拉菜单两行条目(名称+等宽路径,02-composer-workspace-menu.png);选择「演示项目」后请求体携带 workspace id(Playwright 断言);默认不携带;刷新后选择保持 |
| 3 模型侧 | **过** | 真模型问「我现在在哪个工作目录」精确回答 `D:\96_CoderWorld\boenmind-demo-project`(03-chat-workspace-reply.png);上下文页 system prompt 可见 `[工作目录] 本对话的工作目录:…` |
| 4 空气泡 | **过** | 空正文 SSE:首 token 前(04-empty-before-first-token.png)与完成后(05-empty-after-done.png)均无 `.text` 空气泡、tag「生成中」保留;工具调用文本/停止/失败文本照常显示;正常流式回归不退化 |
| 5 回归 | **过** | cargo test --workspace 304 全绿;clippy --all-targets -D warnings 零警告;fmt;validate.py 全绿(两 schema 只增字段);Playwright 冒烟 9/9(含 W8 三用例) |

实现期修正:Windows canonicalize 的 `\?\` 扩展前缀入库前剥离(pretty_normalized),
注册表旧数据已手工清洗。
