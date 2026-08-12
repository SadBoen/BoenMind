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
| 存储 | Turso（limbo，Rust 实现，SQLite 文件格式兼容：`~/.boenmind/boenmind.db`，会话与消息）；配置 `~/.boenmind/config.toml` |

## 目录结构

```
BoenMind/
├── backend/                  # Rust workspace
│   ├── crates/bm-core/       # 配置、SQLite、工作文件夹、pi agent 封装
│   ├── crates/bm-server/     # axum REST + SSE API（独立二进制，也可内嵌）
│   └── vendor/pi_agent_rust/ # pi agent Rust 版（vendored 全量入仓库，含 macOS 修补）
├── frontend/                 # React SPA（网页 / 桌面共用）
│   └── src-tauri/            # Tauri 2 桌面壳
└── gui-test-screenshots/     # 浏览器实测截图证据
```

## 快速开始

前置：Rust 1.95+、Node 20+（pnpm）。

```bash
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
3. **插件**：「设置」→「插件」。内置插件（hello / bookmark / ctx-compactor / web-search）已预装；
   启用后插件注册的工具与命令对 AI 助手立即生效（基于 pi 扩展机制，QuickJS 直接加载
   TypeScript，无需编译）。社区无原生依赖的插件可复制到 `~/.boenmind/extensions/` 后安装。
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
  `textDelta`（思考以 `<think>` 标签随正文下发）/ `toolCallStart` / `toolCallEnd` / `turnEnd` / `done` / `error`；
  `POST /api/chat/stop` 取消进行中的 prompt（已生成内容照常入库）。
- **自动更新**：Tauri updater + GitHub Releases（`latest.json`），签名密钥存于
  `~/.boenmind/tauri-update.key`（私钥与密码需妥善保管，丢失则无法发布更新）。
- **vendored 修补**：pi_agent_rust 在 macOS 上有两处类型不匹配（rustix `st_dev`/`st_mode`），
  已在 `backend/vendor/pi_agent_rust/src/tools.rs` 修补；上游 Google OAuth 公开凭据与测试假 key
  因 GitHub push protection 拦截已替换为占位符（BoenMind 走 API key 认证，不受影响）。

## 已知限制（v0.1）

- 服务重启后，进行中的 agent 会话句柄丢失，新对话上下文从当前消息开始（历史消息仍在 UI 展示）。
- 空闲超时（12 小时）后 agent 会话句柄被淘汰，下一轮对话从历史消息重建上下文。
- 移动端布局尚未适配（桌面优先，文件区在窄屏默认收起）。
- 图片 / PDF 预览为内嵌展示，大文件未做分页优化。
- RAG 知识库（来源引用）为下一期规划。

## 服务器部署（Linux）

服务器版是单文件二进制：前端页面已内嵌（`--features embed`），无需 Node / nginx。
数据（配置、数据库、插件、pi 密钥）存放于 `/var/lib/boenmind`
（可用 `BOENMIND_HOME` 环境变量覆盖）。

### 方式一：systemd 一键安装

```bash
tar xzf boenmind-server_0.1.1_linux-x86_64.tar.gz   # 或 linux-aarch64
cd boenmind-server_0.1.1_linux-x86_64
sudo bash install.sh
```

脚本创建专用用户 `boenmind`、安装 systemd 服务并开机自启，之后浏览器访问
`http://服务器IP:17321`。管理命令：`systemctl status/restart boenmind`。

### 方式二：Docker

```bash
docker run -d --name boenmind --restart unless-stopped \
  -p 17321:17321 \
  -v boenmind-data:/var/lib/boenmind \
  ghcr.io/sadboen/boenmind:v0.1.1
```

或使用仓库根目录的 `docker-compose.yml`（`docker compose up -d`）。

本地手动构建镜像（等价于 CI 的多架构产物，本机架构）：

```bash
docker build -t ghcr.io/sadboen/boenmind:v0.1.1 .
```

### 服务器版环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `BOENMIND_PORT` | `17321` | 端口 |
| `BOENMIND_BIND` | `127.0.0.1` | 监听地址，服务器部署设为 `0.0.0.0` |
| `BOENMIND_HOME` | `~` | 数据目录基础（`.boenmind` 与工作文件夹都在其下） |
| `BOENMIND_TOKEN` | 无 | 访问令牌：设置后所有 `/api` 请求需带 `Authorization: Bearer <token>`，前端首次访问会弹出令牌输入框；桌面版无需设置 |

> ⚠️ 安全：默认**无登录认证**，配置中的 API 密钥对能访问该端口的任何人可见。
> 公网/不可信内网部署请务必设置 `BOENMIND_TOKEN`，或通过反向代理（nginx / caddy）加访问密码 / HTTPS。

## 发布（GitHub Actions 自动构建）

打 tag 即发布：`git tag v0.1.1 && git push origin v0.1.1`，
`.github/workflows/release.yml` 自动构建并上传全部产物（draft Release，确认后发布）：

| 平台 | 产物 |
|---|---|
| macOS ARM / Intel | `BoenMind_0.1.1_{aarch64,x86_64}.dmg`、`.app.tar.gz`（含自动更新签名 latest.json + .sig） |
| Windows 便携版（免安装） | `BoenMind_0.1.1_x64_portable.zip`（依赖系统 WebView2） |
| Linux 服务器版 | `boenmind-server_0.1.1_linux-{x86_64,aarch64}.tar.gz` |
| Docker 多架构镜像 | `ghcr.io/sadboen/boenmind:v0.1.1` |

**一次性准备（发布前必须完成）**：仓库 Settings → Secrets and variables → Actions，添加：

- `TAURI_SIGNING_PRIVATE_KEY`：`base64 ~/.boenmind/tauri-update.key` 的输出（自动更新签名，macOS 构建必需）
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：签名私钥密码

没有这两个 Secret 时 macOS job 会构建失败（`createUpdaterArtifacts` 需要签名）。

**试跑**：Actions 页手动 Run workflow（workflow_dispatch）可构建全部平台产物但
不创建 Release、不推 Docker 镜像。

**已知发布限制**：

- macOS 未签名 / 未 notarize（无 Apple 开发者证书）：首次打开需右键 → 打开。
- Windows 便携版无法应用内自动更新（updater 需要 MSI/NSIS 安装器），请留意 Release 页新版本。
- 若更新了 tauri.conf.json 的 `version`，请同步 bump：`frontend/package.json`、
  `backend/crates/bm-core/Cargo.toml`、`backend/crates/bm-server/Cargo.toml`、
  `docker-compose.yml`（镜像 tag）。

## 许可

本项目基于 MIT 许可（vendored pi_agent_rust 保留其 MIT+OpenAI/Anthropic Rider 原许可文件）。
