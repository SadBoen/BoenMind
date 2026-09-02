# BoenMind

个人生态的 AI Runtime / AI OS(阶段一:跨平台单软件)。

- **AI / 新会话入口**:[AGENTS.md](AGENTS.md) —— 工作规程、文件地图、硬纪律
- **人类阅读入口**:[BoenMind-CORE-ARCHITECTURE.md](BoenMind-CORE-ARCHITECTURE.md) —— 架构基线(§22 是大白话版)
- 进度与欠账:[milestones/HISTORY.md](milestones/HISTORY.md) —— 交付时间线 · [milestones/BACKLOG.md](milestones/BACKLOG.md) —— 未结事项总台账
- 实操备忘:[PLAYBOOK.md](PLAYBOOK.md) —— 启动命令、环境变量、踩坑清单
- 快速导览:[adr/README.md](adr/README.md) · [architecture/README.md](architecture/README.md) · [boenmind-contracts/README.md](boenmind-contracts/README.md) · [milestones/README.md](milestones/README.md)

## 安装(Linux 服务器 / VPS,推荐发布包)

到 [Releases](https://github.com/SadBoen/BoenMind/releases/latest) 下载最新
`boenmind-<版本>-linux-x86_64.tar.gz`(校验和同名 `.sha256`),包内含:
`boenmind-server`(服务器+网页界面)、`plugins/web-multisearch`(官方自带聚合搜索插件)、
`webapp/dist`(预构建前端)、`INSTALL-linux.md`(同下文,离线版)。

前置:x86_64 Linux;OpenSSL 3 运行库(Ubuntu 22.04+ / Debian 12+ 默认自带);**无需** Node/Python。

```bash
# 1. 解压
tar xzf boenmind-<版本>-linux-x86_64.tar.gz && cd boenmind-<版本>

# 2. 首次准备:数据目录 + 空 MCP 插件清单
mkdir -p ~/.local/share/boenmind/mcp ~/.local/share/boenmind/config
echo '[]' > ~/.local/share/boenmind/mcp.json

# 3. 启动(环境变量必须与命令同一行;主密钥至少 32 字符,丢失=已存凭据作废)
BOEN_SECRET_MASTER_KEY="<至少32字符随机串>" BOEN_MODEL_STREAM=1 \
  ./boenmind-server --web-dir webapp/dist --mcp-config ~/.local/share/boenmind/mcp.json

# 4. 安装搜索插件:把 plugins/web-multisearch 拷进插件目录
cp plugins/web-multisearch ~/.local/share/boenmind/mcp/
#   然后网页 → 设置 → 插件 → 「扫描插件」→ 「批准接入」(免重启)
```

打开 `http://127.0.0.1:7531/`;远程 VPS 建议 SSH 隧道:`ssh -L 7531:127.0.0.1:7531 <你的VPS>`。

**模型接线(二选一)**:
- 环境变量:`BOEN_MODEL_BASE_URL` / `BOEN_MODEL_ID` / `BOEN_MODEL_API_KEY`(加在上面启动命令同一行);
- 或启动后进网页设置页新增模型提供商并「设为当前」(写入配置,重启生效)。

**注意**:
- 数据目录默认 `~/.local/share/boenmind/`(state.db、config/、mcp/、token);
- **同一数据目录禁止同时跑两个 boenmind-server**(持久层会损坏);
- 停止服务 = 对进程 Ctrl+C / kill,重启后网页带旧会话号会自动重开新会话。

## 从源码构建(可选)

需要 Rust 1.98+、Node 24:

```bash
cd runtime/webapp && npm ci && npm run build && cd ../..     # 前端 dist
cd runtime && cargo build --release --bin boenmind-server    # 服务器
cd ../plugins/mcp/web-multisearch && cargo build --release   # 搜索插件(可选)
```

发版:打 `v*` tag 推送即自动构建发布(Linux 包);开发规程见 [AGENTS.md](AGENTS.md)。

