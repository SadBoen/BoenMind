# BoenMind v0.1.2 回归测试报告（真实页面模拟）

> 2026-08-13 · 浏览器真实页面访问（IAB）+ 视觉/API 交叉验证 · 对应 commit `601b6f6`

## 一、测试方式

- 环境：`bm-server`（debug）+ Vite dev server + IAB 浏览器（1280×720）
- 方法：真实 GUI 操作（点击/输入/拖拽/键盘），DOM 快照 + 截图（MiniMax-M3 视觉复核）双轨验证
- 证据截图：`gui-test-screenshots/`（t1~t11）

## 二、测试结果汇总（24 项）

| 组 | 测试项 | 结果 |
|---|---|---|
| **A 聊天** | A1 三栏布局+导航+状态栏 | ✅ |
| | A2 新建对话→流式回复（思考/工具块/Markdown 列表） | ✅ |
| | A3 Enter 发送 ✅；Shift+Enter 换行（组件逻辑正确，IAB 无法模拟组合键默认行为=环境受限） | ✅/⚠️ |
| | A4 多轮上下文（AI 记得第一轮问题） | ✅ |
| | A5 停止按钮（部分内容保留、输入框恢复） | ✅ |
| | A6 模型切换+思考档位（MiniMax 4 档 / deepseek 6 档动态出档） | ✅ |
| | A7 新建/自动命名/重命名（编辑→保存→还原）/删除 | ✅ |
| | A8 会话切换消息回显 | ✅ |
| | A9 任务状态条+subagent 结构化表格汇报 | ✅ |
| | A10 权限询问（extension-permissions.json 未配置默认放行，UI 入口存在） | ⚠️环境 |
| **B 文件区** | B1 列表 / B2 目录导航 / B3 预览（md+图片） | ✅ |
| | B4 最大化（文件区 63% 宽、主区折叠） | ✅ |
| | B5 分栏拖拽（960→1020） | ✅ |
| **C 设置** | C1 主题（暗/亮）+ 语言（zh/en/ja/ko）切换与持久化 | ✅ |
| | C2 提供商列表/预设/编辑删除渲染；表单交互=环境受限 | ✅/⚠️ |
| | C3 工作文件夹 | ✅ |
| | C4 插件列表/开关/权限模式/安装入口 | ✅ |
| | C5 Skills 列表/开关/随机获取（skills.sh 真实请求） | ✅ |
| | C6 改进建议（筛选 tabs + 审批→skill 更新→回滚闭环 API 验证） | ✅ |
| | C7 关于+检查更新（真实 GitHub：已是最新 v0.1.1） | ✅ |
| **D 其他** | D1 专家团队文档页 / D2 占位导航 / D3 状态栏 | ✅ |
| **E 边界** | E1 空输入禁用 / E2 代码块渲染 / E3 暗色一致性 / E4 长标题截断 / E5 语言即时生效 | ✅ |

## 三、发现并修复的问题

### BUG-1（P1，已修）：跨提供商切换模型后发送消息 401
- **现象**：minimax 会话把模型切到 deepseek-chat 后发送，回复 `Provider error: minimax: Anthropic API error (HTTP 401) invalid api key`；连带 subagent 任务全部失败
- **根因**：前端只传 `model`，后端仍按会话原 provider 解析 → pi 在 minimax 模型表找不到 deepseek-chat → 降级 Anthropic 默认路由 + minimax key → 401
- **修复**：`ChatRequest` 增 `provider` 字段；`get_or_create_agent` 按请求级 provider 优先解析；新增 `db::set_session_model` 持久化会话 provider/model；前端 `ChatInput`/`sendMessage`/`api.chat` 传递 provider
- **验证**：API 复现（401）→ 修复后路由正确（deepseek + openai-completions）+ 会话持久化正确；subagent 链路恢复（diag 验证子进程环境正确）

### BUG-2（P2，已修）：添加提供商预设列表 MiniMax 显示 i18n key
- **现象**：`settings.providers.kinds.minimax` 原样显示
- **根因**：4 个语言包 `kinds` 均缺 `minimax` 键
- **修复**：zh/en/ja/ko 补齐 `minimax: "MiniMax"`
- **验证**：浏览器确认预设显示 "MiniMax"

### 误报撤销
- 会话重命名菜单点击无效：新 tab 复测正常（旧 tab IAB 状态异常所致），非产品 bug

## 四、环境受限项（非产品问题）

- **IAB 组合键**：Shift+Enter 的浏览器默认换行行为无法由自动化模拟（裸 textarea 对照实验证实）
- **base-ui Dialog/Portal 交互**：添加提供商表单等 dialog 内点击在 IAB 中不稳定（真实浏览器无此问题）
- **A10 权限询问**：当前配置无 extension-permissions.json（默认放行），无法自然触发询问弹窗

## 五、质量门

- 后端测试 67+10 全过；clippy -D warnings 零告警
- 前端 tsc strict 0 错误；lint 0 错误（12 个既有 warning）
- 生产构建成功

## 六、发布

- version 0.1.1 → 0.1.2（tauri.conf.json + Cargo.toml ×2）
- **取消 macOS Intel 构建**（用户要求）：release.yml 移除 x86_64 构建与 latest.json 合并逻辑，release 文案更新
- tag `v0.1.2` 触发全量发布（macOS ARM / Windows 便携 / Linux 服务器 ×2 架构 / Docker 多架构）
