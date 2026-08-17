# third-party/dsh-plugins —— DSH 插件选型快照（2026-08-17）

> 用途：BoenMind 扶正 dsh 内核的**插件选型快照**（见 `docs/design/DSH_PROJECT_2026-08-17.md` §三/四）。
> 快照策略：锁版本、源码级核验、只收"官方缝 + 我们缺且官方无"两类。**更新**：正式采用某插件前，重拉 latest 并更新本表（dsh 是 rc 生态，迭代极快）。
> 许可：分发/内置前逐包复核。官方 `@deepseek-ai` 包以各自 tgz 内 package.json 为准（主体 MIT）；社区见下表。

## 目录结构

```
official/   —— npm pack @latest 的 tgz 原件（44 包，1.4MB），文件名即版本
community/  —— GitHub 浅克隆快照（11 仓库，24MB，已剥 .git）
```

## 表 A：官方 npm 包（44，`@deepseek-ai/*`）

| 类别 | 包 | 版本 | 对应 BoenMind |
|---|---|---|---|
| 运行时 | dsh | 0.1.0-rc.6 | CLI/引擎 |
| | dsh-base | 0.0.1-rc.1 | profile 第一层 |
| | dsh-web-app | 0.0.1-rc.1 | 浏览器壳（GUI） |
| | dsh-headless | 0.0.1-rc.1 | 无头运行 |
| 回合/会话 | dsh-agent / dsh-agent-loop | 0.1.0-rc.6 / 0.1.0-rc.6 | bm-loop/bm-kernel |
| | dsh-session | 0.0.1-rc.1 | SessionEvent 会话 |
| 存储 | dsh-session-persistence-sqlite / -jsonl | 0.0.1-rc.1 | turso/JSONL |
| | dsh-storage / dsh-storage-sqlite / dsh-storage-json | 0.0.1-rc.1 | 通用存储 hub |
| 工具 | dsh-tools | 0.0.1-rc.1 | 工具注册/执行缝 |
| | dsh-tool-fs / dsh-tool-fs-search / dsh-tool-str-replace-editor | 0.0.1-rc.1 | 文件读写/检索/编辑 |
| | dsh-tool-terminal / dsh-tool-bash | 0.0.1-rc.5 / 0.0.1-rc.1 | 终端/bash |
| | dsh-tool-web / dsh-web-search-deepseek / dsh-web-fetch-http | 0.0.1-rc.1 | web 搜索/抓取 |
| MCP | dsh-mcp-client | 0.0.1-rc.1 | bm-mcp 替换 |
| 压缩/记忆 | dsh-compaction / dsh-compaction-basic / dsh-compaction-tool-result-pruner | 0.0.1-rc.5 / -rc.3 / -rc.3 | ctx-compactor |
| | dsh-spill / dsh-spill-local | 0.0.1-rc.1 | 大结果落盘 |
| 子代理/编排 | dsh-subagent / dsh-subagent-spawn-in-process | 0.0.1-rc.1 / -rc.3 | 专家团队子代理 |
| | dsh-tool-subagent-control | 0.0.1-rc.1 | send/interrupt/list |
| | dsh-workflow / dsh-tool-workflow / dsh-goal | 0.0.1-rc.1 | 队长编排/目标 |
| 专家/作用域 | dsh-agent-presets / dsh-scope | 0.0.1-rc.1 | agents/*.md、plugin_scopes |
| UI | dsh-client-ui-slots / dsh-client-ui-layout / dsh-client-ui-theme | 0.0.1-rc.1 | 槽位/布局/皮肤令牌 |
| | dsh-client-ui-tool / dsh-client-ui-trajectory | 0.0.1-rc.1 | 工具调用可视化 |
| | dsh-client-web-react | 0.0.1-rc.1 | React 壳桥 |
| 遥测/审计 | dsh-session-telemetry | 0.0.1-rc.1 | 审计/工具显示 |
| LLM | dsh-llm / dsh-llm-deepseek | 0.0.1-rc.1 | Provider 适配缝 |

## 表 B：社区 GitHub（11，浅克隆快照）

| 仓库 | 许可 | 作用 | 对应 BoenMind |
|---|---|---|---|
| No-PRM/dsh-explorer | MIT | Git-first 文件树（git 装饰） | FilePanel/ProjectSwitcher |
| WhitePlusMS/dsh-git-graph | MIT | 分支图 | GitGraph |
| SenryLee/dsh-frosted-window | MIT | 毛玻璃皮肤 | 玻璃皮肤（Aqua 参照） |
| LeemanCheung/dsh-task-dag | MIT | 子代理任务 DAG 可视化 | 专家团队编排可视化 |
| giiiiiithub/terminal | MIT | node-pty + xterm 终端 | 终端单元 |
| tsonglew/dsh-workspace-search | MIT | 工作区关键字搜索 | 工作区检索 |
| Js2Hou/dsh-mcp-manager | **未声明** | MCP 管理面板 | 设置中心 MCP 段（采用前须向作者确认许可） |
| zhijun-dai/Catppuccin-dsh-theme | MIT | 主题（令牌改造参照） | 皮肤系统 |
| dsh-market/dsh-market | MIT | 插件市场 | 插件市场（复用 ZCode marketplace.json 机制） |
| awesome-dsh-plugin/awesome-dsh-plugin | CC0 | curated 清单 | 持续跟踪入口 |
| ccq1/dsh-side-panel | BSD-3 | 侧栏面板 | 侧栏参照 |

**未入库**：`zhu1090093659/dsh-web-ui`（整站皮肤中心）——127MB 图片资产太重，仅作按需取用来源；`dsh-mcp-manager` 许可未声明，正式采用前必须解决。

## 用法与更新

- 离线安装：`npm install --offline <tgz 路径>`（dsh 插件即 npm 包）。
- 版本升级：快照内全部为 2026-08-17 的 latest；采用前重拉并在 `backend/vendor/UPSTREAM_TRACKING.md` 增补 dsh 区台账（上游 commit/版本/本地补丁登记）。
- 浏览器自动化为 dsh 生态缺口：**不在本快照**，自研排期（T4 续）。
