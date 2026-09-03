# BoenMind

个人生态的 AI Runtime / AI OS(阶段一:跨平台单软件)。

- **AI / 新会话入口**:[AGENTS.md](AGENTS.md) —— 工作规程、文件地图、硬纪律
- **人类阅读入口**:[BoenMind-CORE-ARCHITECTURE.md](BoenMind-CORE-ARCHITECTURE.md) —— 架构基线(§22 是大白话版)
- 进度与欠账:[milestones/HISTORY.md](milestones/HISTORY.md) —— 交付时间线 · [milestones/BACKLOG.md](milestones/BACKLOG.md) —— 未结事项总台账
- 实操备忘:[PLAYBOOK.md](PLAYBOOK.md) —— 启动命令、环境变量、踩坑清单
- 快速导览:[adr/README.md](adr/README.md) · [architecture/README.md](architecture/README.md) · [boenmind-contracts/README.md](boenmind-contracts/README.md) · [milestones/README.md](milestones/README.md)

## 安装(VPS / Linux 服务器,推荐发布包)

到 [Releases](https://github.com/SadBoen/BoenMind/releases/latest) 下载最新
`boenmind-<版本>-linux-x86_64.tar.gz`(校验和同名 `.sha256`),包内含:
`boenmind-server`(服务器+网页界面)、`plugins/web-multisearch`(官方聚合搜索 MCP)、`plugins/context-mode`(官方可选 Rust 上下文 MCP,默认不启用)、
`webapp/dist`(预构建前端)、`INSTALL.md`(离线安装说明)。

前置:x86_64 Linux;OpenSSL 3 运行库(Ubuntu 22.04+/Debian 12+ 默认自带);**无需** Node/Python。

### 1. 解压到固定目录

```bash
sudo mkdir -p /opt/boenmind
sudo tar xzf boenmind-<版本>-linux-x86_64.tar.gz -C /opt/boenmind --strip-components=1
```

之后所有在线升级都只换这个目录里的文件,数据(对话/配置/密钥)在数据目录,不受影响。

### 2. 数据目录准备(一次)

```bash
mkdir -p ~/.local/share/boenmind/mcp ~/.local/share/boenmind/config
echo '[]' > ~/.local/share/boenmind/mcp.json
```

### 3. 生成主密钥(一次,务必保存)

```bash
openssl rand -hex 24    # 输出即主密钥,≥32 字符
```

**这是加密凭据库的总钥匙:丢了=已存模型密钥作废;千万别用 changeme 之类弱串。**
把它记到只有你能看到的地方(密码管理器)。

### 4. 配置 systemd 常驻(推荐;开机自启+崩溃自动拉起+在线升级自动重启)

```bash
sudo tee /etc/systemd/system/boenmind.service >/dev/null <<'UNIT'
[Unit]
Description=BoenMind AI Runtime
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/boenmind
# ↑ 换成第 3 步生成的真随机主密钥
Environment=BOEN_SECRET_MASTER_KEY=把这里换成上面openssl生成的值
Environment=BOEN_MODEL_STREAM=1
ExecStart=/opt/boenmind/boenmind-server --web-dir /opt/boenmind/webapp/dist --mcp-config /root/.local/share/boenmind/mcp.json
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
UNIT
sudo systemctl daemon-reload
sudo systemctl enable --now boenmind
systemctl status boenmind --no-pager    # 应显示 active (running)
```

`Restart=always` 很重要:进程意外退出会自动拉起;在线升级也会自动经 systemd 重启。

模型接线(二选一):在 unit 里再加一行 `Environment=BOEN_MODEL_BASE_URL=... BOEN_MODEL_ID=... BOEN_MODEL_API_KEY=...`;或先不填,启动后进网页设置页添加模型并「设为当前」。

### 5. 首次使用(网页)

打开 `http://<服务器IP>:7531/`(公网服务器建议先用 SSH 隧道:`ssh -L 7531:127.0.0.1:7531 root@<你的VPS>`,本地开 `http://127.0.0.1:7531/`):

1. **创建访问密码**(登录页第一屏,≥6 位)——这一步保护整个网页界面,**务必先做**;
2. 设置 → 模型提供商:填网关地址/模型/密钥 → 「设为当前」;
3. 设置 → MCP:「扫描插件」→ 批准 `web_multisearch`(联网搜索)→ 「重载 MCP」;
4. 开始对话。让模型执行命令时(system.exec)每条命令会弹审批卡,你点批准才执行。

### 在线升级

网页 → 设置 → 关于 → 「检查更新」→「一键升级」。数据(对话/配置)在数据目录,升级不动它。

- **v0.0.6.1 起**:升级自动经 systemd 重启,无需任何手动操作;
- **v0.0.6 及更早**:升级后服务会停(已知缺陷),手动拉起一次即可:
  `sudo systemctl restart boenmind`,然后刷新页面。

### 排障速查

| 症状 | 处理 |
|---|---|
| 网页打不开/502 | `systemctl status boenmind` 看是否 active;没起就 `sudo systemctl restart boenmind`,再看 `journalctl -u boenmind -n 50 --no-pager` |
| 忘记网页密码 | `rm ~/.local/share/boenmind/config/portal.json` 后 `sudo systemctl restart boenmind`,登录页恢复「创建密码」 |
| 启动即退,日志有 Corrupt/位点 | 备份后 `mv ~/.local/share/boenmind/state.db* ~/boenmind-backup/` 再重启(投影库会从事件日志自动重建,对话记录不丢) |
| 升级后模型密钥失效 | 主密钥(Unit 里 BOEN_SECRET_MASTER_KEY)与当初加密时不一致;换回原密钥,或重新填写模型密钥 |

### 手动前台启动(调试用,不经 systemd)

```bash
BOEN_SECRET_MASTER_KEY="<主密钥>" BOEN_MODEL_STREAM=1   ./boenmind-server --web-dir webapp/dist --mcp-config ~/.local/share/boenmind/mcp.json
```

**注意**:同一数据目录禁止同时跑两个 boenmind-server(持久层会损坏);官方插件已随包在 `plugins/`,网页扫描即可见,免手动拷贝。

## 从源码构建(可选)

需要 Rust 1.98+、Node 24:

```bash
cd runtime/webapp && npm ci && npm run build && cd ../..     # 前端 dist
cd runtime && cargo build --release --bin boenmind-server    # 服务器
cd ../plugins/mcp/web-multisearch && cargo build --release   # 搜索插件(可选)
cd ../context-mode && cargo build --release                    # 上下文插件(可选)
```

发版:打 `v*` tag 推送即自动构建发布(Linux 包);开发规程见 [AGENTS.md](AGENTS.md)。

