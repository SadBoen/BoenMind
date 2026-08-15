# Coding Memory（编程记忆）

编程 APP 专用的项目级记忆插件。与全局长期记忆（facts.md）**隔离**——
编程记忆按项目分桶存放，不污染聊天/全局事实。

## 工具

| 工具 | 用途 |
| --- | --- |
| `coding_remember {fact, project?}` | 记一条项目事实（自动去重，≤500 字符） |
| `coding_recall {query?, limit?, project?}` | 检索项目记忆（空 query = 最近若干条；中文按两字切词） |
| `coding_forget {fact, project?}` | 删除一条记忆（按原文精确匹配） |

编程会话中模型会自动使用：任务开始先 `coding_recall` 拿项目背景，关键
结论/踩坑/决策用 `coding_remember` 沉淀。

## 数据

- 存储：`~/.boenmind/coding-memory/<项目桶>/facts.jsonl`
- 项目桶：显式 `project` 参数优先，否则当前工作目录（cwd 消毒后分桶），
  多项目互不串写；每桶上限 2000 条，超出丢最旧。
- 持久化走宿主 `write`/`read` 工具（QuickJS node:fs 写是 VFS 内存层不落盘）。

## 架构

- 纯插件实现（无宿主改动）：工具面注册 + pi.tool 读改写真落盘；
- 目前工具对所有会话可见（插件加载是全局的）；按应用（scene）限定工具面
  随 manifest scopes（架构 §四·C）落地后收紧，编程场景专属。
- 注入方式为显式检索（Hermes 风格），不做全量注入——避免项目记忆稀释
  上下文；全局长期记忆（facts.md）的自动注入不受本插件影响。
