# BoenMind

个人知识管理软件 —— LLM-WIKI 路线 + 个人 Agent 特色，专注工作与知识。

- **知识助手**：答疑 + 来源引用（规划中）
- **干活代理**：执行任务（规划中）
- **会话管理**：多会话、自动命名、重命名/删除

## 技术栈

| 层 | 技术 |
|---|---|
| 后端 | Rust（workspace：`bm-core` 领域层 + `bm-server` axum API），基于 [pi_agent_rust](https://github.com/Dicklesworthstone/pi_agent_rust)（vendored，MIT+OpenAI/Anthropic Rider 许可） |
| 前端 | React 19 + Vite + TypeScript + Tailwind v4 + shadcn/ui（base-nova）+ Zustand + react-resizable-panels |
| 桌面 | Tauri 2（内嵌启动后端，同一套前端代码） |
| 存储 | SQLite（`~/.boenmind/boenmind.db`，会话与消息）；配置 `~/.boenmind/config.toml` |

## 目录结构

```
BoenMind/
├── backend/                  # Rust workspace
│   ├── crates/bm-core/       # 配置、SQLite、工作文件夹、pi agent 封装
│   ├── crates/bm-server/     # axum REST + SSE API（独立二进制，也可内嵌）
│   └── vendor/pi_agent_rust/ # pi agent Rust 版（git submodule，含 macOS 修补）
├── frontend/                 # React SPA（网页 / 桌面共用）
│   └── src-tauri/            # Tauri 2 桌面壳
└── gui-test-screenshots/     # 浏览器实测截图证据
```

## 快速开始

前置：Rust 1.95+、Node 20+（pnpm）。

```bash
# 0. 首次克隆后拉取 submodule
git submodule update --init

# 1. 启动后端（http://127.0.0.1:17321）
cd backend && cargo run -p bm-server

# 2. 启动前端（http://localhost:5173，自动代理 /api 到后端）
cd frontend && pnpm install && pnpm dev

# 3. 桌面版（自动拉起后端 + 前端）
cd frontend && pnpm tauri dev

# 4. 桌面版构建（debug，不打包安装器）
cd frontend && pnpm tauri build --debug --no-bundle
```

## 使用

1. **配置模型提供商**：点击左下角「设置」→「模型提供商」→「添加提供商」。
   支持预设：MiniMax（国内）、DeepSeek、OpenRouter、OpenAI 兼容、Anthropic、Gemini、Ollama、llama.cpp、
   自定义 OpenAI 兼容。多提供商可同时配置（各自独立端点与密钥，互不覆盖）；本地服务需填端点。
2. **对话**：会话列表「新建对话」→ 输入消息（Enter 发送，Shift+Enter 换行）。
   模型可在聊天标题栏右侧切换。
3. **插件**：「设置」→「插件」。内置示例（hello / bookmark）已预装；启用后插件注册的
   工具与命令对 AI 助手立即生效（基于 pi 扩展机制，QuickJS 直接加载 TypeScript，无需编译）。
   社区无原生依赖的插件可复制到 `~/.boenmind/extensions/` 后安装。
4. **文件浏览**：右侧文件区展示工作文件夹（默认 `~/BoenMind`，可在设置修改）。
   点击文件进入预览（Markdown / 代码 / 图片 / PDF）；右上角可最大化，占据自身+主区。
5. **自动更新**：「设置」→「关于」→「检查更新」。桌面版通过 GitHub Releases 分发，
   后台下载完成后提示重启（macOS 可后台下载，重启生效）。
6. **布局**：分栏可拖动（有上下限），底部状态栏显示后端连接、当前模型、工作目录。

## 架构要点

- **同一前端，三端可扩展**：前端通过 `ApiClient` 访问后端——网页版走 Vite 代理（同源 `/api`），
  桌面版由 Tauri 注入 `window.__BOENMIND_API__` 指向内嵌后端（固定 127.0.0.1:17321）。
- **pi agent 集成**：`bm-core::agent` 封装 pi 的 SDK（`create_agent_session` + `prompt` 事件流），
  会话句柄按聊天会话缓存在服务端；`PI_CODING_AGENT_DIR` 指向 `~/.boenmind/pi`，
  提供商端点/模型/密钥通过自动生成的 `models.json` 注册（密钥以 `file:` 引用存于
  `~/.boenmind/pi/keys/`，不落盘 JSON），与用户自己的 `~/.pi` 配置互不干扰。
  自定义 OpenAI 兼容提供商（MiniMax/DeepSeek/OpenRouter 等）以独立 provider 名 + `openai-completions`
  路由注册，多个端点可共存。
- **插件**：`bm-core::plugins` 管理 `~/.boenmind/extensions/` 下的 TypeScript 扩展
  （pi QuickJS 运行时加载，无需转 Rust）；启用列表在 config.toml，会话创建时经
  `SessionOptions.extension_paths` 加载。
- **SSE 流式协议**：`POST /api/chat` 返回 `text/event-stream`，事件类型：
  `textDelta` / `thinkingDelta` / `toolCallStart` / `toolCallDelta` / `turnEnd` / `done` / `error`。
- **自动更新**：Tauri updater + GitHub Releases（`latest.json`），签名密钥存于
  `~/.boenmind/tauri-update.key`（私钥与密码需妥善保管，丢失则无法发布更新）。
- **vendored 修补**：pi_agent_rust 在 macOS 上有两处类型不匹配（rustix `st_dev`/`st_mode`），
  已在 `backend/vendor/pi_agent_rust/src/tools.rs` 修补；上游 Google OAuth 公开凭据与测试假 key
  因 GitHub push protection 拦截已替换为占位符（BoenMind 走 API key 认证，不受影响）。

## 已知限制（v0.1）

- 服务重启后，进行中的 agent 会话句柄丢失，新对话上下文从当前消息开始（历史消息仍在 UI 展示）。
- 模型在会话创建时确定，切换模型仅影响新会话。
- 移动端布局尚未适配（桌面优先，文件区在窄屏默认收起）。
- 图片 / PDF 预览为内嵌展示，大文件未做分页优化。
- 工具调用（干活代理）与 RAG 知识库（来源引用）为下一期规划。

## 许可

本项目基于 MIT 许可（vendored pi_agent_rust 保留其 MIT+OpenAI/Anthropic Rider 原许可文件）。
