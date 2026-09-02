# BoenMind 安装说明

前置:64 位(x86_64);Linux 需 OpenSSL 3(Ubuntu 22.04+/Debian 12+ 默认自带);
**无需 Node/Python**(界面已预构建,搜索插件是单文件可执行)。

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

## 4. 安装官方搜索插件(plugins/web-multisearch)

    cp plugins/web-multisearch ~/.local/share/boenmind/mcp/
    # 网页 → 设置 → 插件 → 「扫描插件」→ 「批准接入」(免重启)

## 5. 访问

    http://127.0.0.1:7531/
    远程 VPS 建议 SSH 隧道:ssh -L 7531:127.0.0.1:7531 <你的VPS>

## 6. 在线升级(已装用户)

    网页 → 设置 → 关于 → 「检查更新」→「一键升级」。
    仅允许本机(回环)发起;升级会自动重启服务并换装前端。

注意:数据目录默认 `~/.local/share/boenmind/`(state.db、config/、mcp/、token);
Windows 默认 `%APPDATA%\Roaming\boenmind\`;
**同一数据目录禁止同时跑两个 boenmind-server 进程**。
