# BoenMind 安装说明

前置:64 位(x86_64);Linux 需 OpenSSL 3(Ubuntu 22.04+/Debian 12+ 默认自带);
**无需 Node/Python**(界面已预构建，官方 MCP 插件是单文件可执行；context-mode 的执行工具只调用用户自行安装的宿主程序，不会自动下载运行时)。

## 1. 解压

    tar xzf boenmind-<版本>-<平台>.tar.gz
    cd boenmind-<版本>-<平台>

## 2. 环境变量(与启动命令写在同一条命令里)

    export BOEN_SECRET_MASTER_KEY="<至少32字符随机串>"  # 加密密钥库主密钥,必带;丢失=已存凭据作废
    export BOEN_MODEL_STREAM=1                         # 开模型流式(当前版本必带,配置文件流式字段尚未接线)

    模型接线二选一:
    A. BOEN_MODEL_BASE_URL / BOEN_MODEL_ID / BOEN_MODEL_API_KEY 三个环境变量;
    B. 先不填,启动后进网页设置页新增模型提供商并「设为当前」(重启生效)。

## 3. 首次启动

    mkdir -p ~/.local/share/boenmind/mcp ~/.local/share/boenmind/config
    echo '[]' > ~/.local/share/boenmind/mcp.json
    BOEN_SECRET_MASTER_KEY="<同上>" BOEN_MODEL_STREAM=1 \
      ./boenmind-server --web-dir webapp/dist --mcp-config ~/.local/share/boenmind/mcp.json

## 4. 官方 MCP 插件(已随包,免手动拷贝)

官方插件就在安装目录的 `plugins/` 里(聚合搜索 `web-multisearch`、上下文模式
`context-mode`),**无需手动拷贝**:解压后首次启动(以及后续在线升级)都能被
插件扫描直接发现。

    网页 → 设置 → MCP → 「扫描插件」→ 「批准接入」→ 「重载 MCP」(免重启)

如偏好把插件收进数据目录统一管理,拷贝亦可(同名候选以数据目录优先):

    cp plugins/web-multisearch ~/.local/share/boenmind/mcp/

context-mode 提供 `ctx_index`、`ctx_search`、会话快照/恢复和受限执行工具。索引/执行只能访问它配置的 `allowed_roots`；执行类工具仍由 Broker 审批，缺少用户本机运行时则返回 `runtime_unavailable`。

## 5. 访问

    http://127.0.0.1:7531/
    远程 VPS 建议 SSH 隧道:ssh -L 7531:127.0.0.1:7531 <你的VPS>

## 6. 在线升级(已装用户)

    网页 → 设置 → 关于 → 「检查更新」→「一键升级」。
    仅允许本机(回环)发起;升级会自动重启服务并换装前端。

注意:数据目录默认 `~/.local/share/boenmind/`(state.db、config/、mcp/、token);
Windows 默认 `%APPDATA%\Roaming\boenmind\`;
**同一数据目录禁止同时跑两个 boenmind-server 进程**。

## 7. systemd 常驻(推荐)与排障

生产部署建议用 systemd 管理(开机自启+崩溃自动拉起+在线升级自动重启),完整单元文件
模板见仓库 README「安装」一节;要点:`Restart=always`、真随机主密钥
(`openssl rand -hex 24`,勿用弱串)、`ExecStart` 指向解压目录的 boenmind-server。

排障速查:
- 网页打不开/502:`systemctl status boenmind`;没起就 `systemctl restart boenmind`,
  再看 `journalctl -u boenmind -n 50 --no-pager`;
- 忘记网页密码:删除 `<数据目录>/config/portal.json` 并重启服务,登录页恢复「创建密码」;
- 启动即退且日志见 Corrupt/位点:备份后移除 `<数据目录>/state.db*` 再重启
  (投影库会从事件日志自动重建,对话记录不丢);
- 在线升级:v0.0.6.1 起自动经 systemd 重启;更早版本升级后需手动
  `systemctl restart boenmind`。
