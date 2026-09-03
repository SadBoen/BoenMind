# ADR-0018: 工作区注册表与会话级工作目录绑定

- 状态: Accepted（用户 2026-09-03 需求口述授权实施）
- 日期: 2026-09-03
- 关联: ADR-0005（万物皆插件）、ADR-0006（权限以合同显式化）、ADR-0012（配置文件>env 口径）、ADR-0014（W 序列技术路线）

## 背景

用户以「项目」心智使用 BoenMind：每个项目有自己的本机目录，希望 (1) 设置页新增
「常规」维护项目工作目录清单并检测系统 Python/Node.js 安装情况；(2) 聊天输入框
可上拉选择当前对话的工作目录；(3) 选择按会话保存、由后端校验，而非仅存
localStorage 的假选择。现状：文件浏览根（`BOEN_WORKSPACE_DIR` > `<data-dir>/workspace`）
是服务器启动期单值，无多项目注册；会话与 Agent 均无工作目录概念。

## 决策

1. **工作区注册表 = 数据目录配置文件**（ADR-0012 口径）：`config/workspaces.json`，
   形状 `{"workspaces":[{"id","name","path"}]}`。id 为不透明短 id（`ws_` 前缀），
   `default` 条目由管理面首次读取时按现役文件浏览根自动播种，保证既有文件树与
   旧用法零破坏。管理面 CRUD 走 `/admin/workspaces`（壳子私用 REST，沿 W2 裁决
   暂不入冻结合同）。写入前校验：路径存在、是目录、canonicalize、重复路径拒绝。
2. **会话绑定，不新增特权通道**：wire 合同 Minor 只增两字段——
   `AgentSpec.workspace_id`（会话创建时绑定）与 `SendInputParams.workspace_override`
   （对话中途切换，随下一条消息生效，同 `model_override` 先例）。核心在会话创建
   与覆盖时对注册表校验，未登记 id = validation_failed；绑定值为不透明 id，
   路径解析只在服务器侧发生，浏览器/模型均不得以任意绝对路径当权限凭据。
3. **作用域 = 进程内会话**：workspace_id 记录在核心内存 Session 上，不入
   SQLite、不发新事件。理由：Web 会话指针（v1_sessions）本就随进程重启失效
   （W1 口径「未知会话→重开」），持久化无用户可见收益；服务重启后浏览器首条
   消息即重新绑定。此边界写入 W8 规格并登记 BACKLOG（跨重启恢复时随会话列表
   真数据一并评估）。
4. **模型可见性 = 回合级 system prompt 注入**：回合组装时按会话绑定解析注册表，
   把「当前工作目录」追加到 system prompt；切换工作区下一条消息即换目录说明。
   解析失败（目录被删）静默降级不注入。能力执行面的 cwd 注入（MCP/context-mode
   的 allowed_roots 联动）留给 Skill v0.2 执行线，本 ADR 不扩权：任何能力调用
   仍走 Broker 七步管线，工作区选择不构成绕过审批的通道（ADR-0006）。
5. **运行环境探针 = 固定命令、无 shell**：`/admin/runtime/env` 对 Python
   依次尝试 `python3 --version` / `python --version` / `py -3 --version`、对
   Node 尝试 `node --version`，5 秒超时，只回传 `installed/version/program/error`
   脱敏结果；禁止拼接用户输入，无注入面。

## 后果

- 用户首次获得多项目工作目录：设置页维护清单，聊天按对话选择，模型自下一
  条消息起知晓当前目录；刷新页面后选择仍随消息重放（前端存最近选择）。
- 会话间工作区互不串扰；删除已被绑定的工作区条目后，其会话回合静默失去
  目录注入，不中断对话。
- 合同两字段为可选只增，旧客户端与黄金轨迹零改动；`validate.py` R1–R4 维持全绿。
- 遗留：能力执行 cwd 注入、workspace_id 跨重启持久、按目录过滤文件树面板，
  均登记 BACKLOG 不伪装为已交付。
