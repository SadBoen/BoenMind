# BoenMind

个人生态的 AI Runtime / AI OS(阶段一:跨平台单软件)。

- **AI / 新会话入口**:[AGENTS.md](AGENTS.md) —— 工作规程、文件地图、硬纪律
- **人类阅读入口**:[BoenMind-CORE-ARCHITECTURE.md](BoenMind-CORE-ARCHITECTURE.md) —— 架构基线(§22 是大白话版)
- 进度与欠账:交付史 = git tag/提交说明 · GitHub Issues(`gh issue list`)—— 未结事项总台账 · [docs/architecture/decisions.md](docs/architecture/decisions.md) —— 已裁决查重清单
- 实操备忘:[docs/development/PITFALLS.md](docs/development/PITFALLS.md) —— 启动命令、环境变量、踩坑清单
- 快速导览:[adr/README.md](adr/README.md) · [architecture/README.md](architecture/README.md) · [boenmind-contracts/README.md](boenmind-contracts/README.md) · [milestones/README.md](milestones/README.md)

## 安装(VPS / Linux 服务器,推荐发布包)

完整安装说明(数据目录/主密钥/systemd 常驻/首次使用/排障)统一见
**[packaging/INSTALL.md](packaging/INSTALL.md)**(发布包内同名文件,离线可读)。最小路径:

1. 到 [Releases](https://github.com/SadBoen/BoenMind/releases/latest) 下载最新
   `boenmind-<版本>-linux-x86_64.tar.gz`(校验和同名 `.sha256`),包内含:
   `boenmind-server`(服务器+网页界面)、`plugins/web-multisearch`(官方聚合搜索 MCP)、
   `plugins/context-inspector`(官方 Rust 上下文透视 MCP)、`webapp/dist`(预构建前端)、
   `INSTALL.md`(离线安装说明)、`apps/`(可选独立演示 App,Python 脚本,需自备 Python 3,核心运行不依赖);
2. 前置:x86_64 Linux;OpenSSL 3 运行库(Ubuntu 22.04+/Debian 12+ 默认自带);**无需** Node/Python;
3. 解压到固定目录:`sudo mkdir -p /opt/boenmind && sudo tar xzf boenmind-<版本>-linux-x86_64.tar.gz -C /opt/boenmind --strip-components=1`(之后在线升级只换这个目录里的文件,数据不受影响);
4. 按包内 `INSTALL.md` 完成剩余步骤:数据目录准备 → 生成主密钥(务必保存) →
   systemd 常驻 → 网页首次使用(**第一件事是创建访问密码**)。

在线升级:网页 → 设置 → 关于 → 「检查更新」→「一键升级」(v0.0.6.1 起自动经 systemd 重启,数据不动)。

## 从源码构建(可选)

需要 Rust 1.98+、Node 24:

```bash
cd runtime/webapp && npm ci && npm run build && cd ../..     # 前端 dist
cd runtime && cargo build --release --bin boenmind-server    # 服务器
cd ../plugins/mcp/web-multisearch && cargo build --release   # 搜索插件(可选)
cd ../context-inspector && cargo build --release              # 上下文透视插件(可选)
```

发版:打 `v*` tag 推送即自动构建发布(Linux 包);开发规程见 [AGENTS.md](AGENTS.md)。
