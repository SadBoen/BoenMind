# BoenMind 安装说明

前置:64 位(x86_64);Linux 需 OpenSSL 3(Ubuntu 22.04+/Debian 12+ 默认自带);
**无需 Node/Python**(界面已预构建，官方 MCP 插件是单文件可执行，零外部运行时依赖;
`apps/` 目录是可选的独立演示 App,Python 脚本需自备 Python 3,核心运行不依赖,可无视)。

## 1. 解压

    tar xzf boenmind-<版本>-<平台>.tar.gz
    cd boenmind-<版本>-<平台>

生产部署(VPS)建议固定到 `/opt/boenmind`,之后所有在线升级都只换这个目录里的文件,
数据(对话/配置/密钥)在数据目录,不受影响:

    sudo mkdir -p /opt/boenmind
    sudo tar xzf boenmind-<版本>-<平台>.tar.gz -C /opt/boenmind --strip-components=1

## 2. 环境变量(与启动命令写在同一条命令里)

    export BOEN_SECRET_MASTER_KEY="<至少32字符随机串>"  # 加密密钥库主密钥,必带;丢失=已存凭据作废
    # export BOEN_MODEL_STREAM=1                       # 可选:开模型流式;config/model.json 的 stream 字段优先(设置页保存即写),env 仅兜底

主密钥生成一次即可:`openssl rand -hex 24`(**千万别用 changeme 之类弱串**;
这是加密凭据库的总钥匙,记到只有你能看到的地方,如密码管理器)。

    模型接线二选一:
    A. BOEN_MODEL_BASE_URL / BOEN_MODEL_ID / BOEN_MODEL_API_KEY 三个环境变量;
    B. 先不填,启动后进网页设置页新增模型提供商并「设为当前」(重启生效)。

## 3. 首次启动

    mkdir -p ~/.local/share/boenmind/mcp ~/.local/share/boenmind/config
    echo '[]' > ~/.local/share/boenmind/mcp.json
    BOEN_SECRET_MASTER_KEY="<同上>" \
      ./boenmind-server --web-dir webapp/dist --mcp-config ~/.local/share/boenmind/mcp.json

生产部署建议直接用第 8 节的 systemd 常驻,可跳过手动启动。

## 4. 官方 MCP 插件(已随包,开箱即用)

官方插件就在安装目录的 `plugins/` 里(聚合搜索 `web-multisearch`、上下文透视
`context-inspector`),**免手动操作**(ADR-0023):首次启动(以及后续在线升级)
会自动把未登记的随包插件按「批准接入」同款落盘并装载上线。你卸载/删除过的
官方插件经墓碑记录永不复活;想恢复=设置 → MCP → 「扫描插件」→ 「批准接入」
→ 「重载 MCP」(免重启)。

如偏好把插件收进数据目录统一管理,拷贝亦可(同名候选以数据目录优先):

    cp plugins/web-multisearch ~/.local/share/boenmind/mcp/

context-inspector 提供 `context_inspect_snapshot`、`context_diagnose_spikes`、`context_track_file_effects`、`context_search_history` 四只读透视工具:会话上下文结构快照、token 尖刺诊断、文件行数效应追踪、跨会话历史检索。纯只读诊断,不修改任何状态,不影响压缩与遗忘策略。

## 5. 访问

    http://127.0.0.1:7531/
    远程 VPS 建议 SSH 隧道:ssh -L 7531:127.0.0.1:7531 <你的VPS>

## 6. 首次使用(网页)

1. **创建访问密码**(登录页第一屏,≥6 位)——这一步保护整个网页界面,**务必先做**;
2. 设置 → 模型提供商:填网关地址/模型/密钥 → 「设为当前」;
3. 设置 → MCP:「扫描插件」→ 批准 `web_multisearch`(联网搜索)→ 「重载 MCP」;
4. 开始对话。让模型执行命令时(system.exec)每条命令会弹审批卡,你点批准才执行。

## 7. 在线升级(已装用户)

    网页 → 设置 → 关于 → 「检查更新」→「一键升级」。
    仅允许本机(回环)发起;升级会自动重启服务并换装前端。

注意:数据目录默认 `~/.local/share/boenmind/`(state.db、config/、mcp/、token);
Windows 默认 `%APPDATA%\Roaming\boenmind\`;
**同一数据目录禁止同时跑两个 boenmind-server 进程**。

## 8. systemd 常驻(推荐)

开机自启+崩溃自动拉起+在线升级自动重启。按第 1 节 `/opt/boenmind` 路径的完整单元文件:

```bash
sudo tee /etc/systemd/system/boenmind.service >/dev/null <<'UNIT'
[Unit]
Description=BoenMind AI Runtime
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/boenmind
# ↑ 换成第 2 节生成的真随机主密钥
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

## 9. 排障速查

- 网页打不开/502:`systemctl status boenmind`;没起就 `systemctl restart boenmind`,
  再看 `journalctl -u boenmind -n 50 --no-pager`;
- 忘记网页密码:删除 `<数据目录>/config/portal.json` 并重启服务,登录页恢复「创建密码」;
- 启动即退且日志见 Corrupt/位点:备份后移除 `<数据目录>/state.db*` 再重启
  (投影库会从事件日志自动重建,对话记录不丢);
- 升级后模型密钥失效:主密钥(Unit 里 BOEN_SECRET_MASTER_KEY)与当初加密时不一致;
  换回原密钥,或重新填写模型密钥;
- 在线升级:v0.0.6.1 起自动经 systemd 重启;更早版本升级后需手动
  `systemctl restart boenmind`。
