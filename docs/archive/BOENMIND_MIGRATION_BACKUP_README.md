# BoenMind 迁移备份（私人仓库）

本仓库存放 BoenMind 开发环境迁移备份，**请勿公开**（含发布签名私钥、插件 API Key、聊天数据库）。

## 备份内容（`BoenMind-migration-backup-<日期>.tar.gz`）

| 路径 | 内容 | 重要性 |
|---|---|---|
| `~/.boenmind/` | 发布签名密钥三件套（tauri-update.key/.password/.pub）、聊天数据库 boenmind.db、插件配置（web-search 的 5 个 API Key）、skills、config.toml | **必须恢复** |
| `~/.zcode/cli/memories/` | ZCode 代理的项目记忆（开发知识沉淀） | 强烈建议 |
| `~/BoenMind/` | 工作文件夹研究资料（ccs_*.html、HANDOFF 等） | 可选 |

代码无需备份：`https://github.com/SadBoen/BoenMind.git`（https 远程，无需 SSH 密钥）。

---

## macOS 恢复

```bash
# ① 基础工具
xcode-select --install
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
brew install node@24 git gh
npm install -g pnpm@11
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-apple-darwin   # 本机出 dmg 才需要

# ② 拉代码 + 恢复数据
git clone https://github.com/SadBoen/BoenMind.git
tar xzf BoenMind-migration-backup-20260813.tar.gz -C ~     # 恢复 ~/.boenmind/ 与 ~/.zcode/cli/memories/
# 工作文件夹路径若不同，在应用设置里改

# ③ 构建验证
cd BoenMind/backend && cargo build && cargo test -p bm-core -p bm-server
cd ../frontend && pnpm install && pnpm tsc -b && pnpm dev
```

## Debian / Ubuntu 恢复

```bash
# ① 基础工具
sudo apt update && sudo apt install -y git curl build-essential pkg-config libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# node 24（nodesource）
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash - && sudo apt install -y nodejs
sudo npm install -g pnpm@11
# gh CLI
(type -p wget >/dev/null || sudo apt install wget) \
  && sudo mkdir -p -m 755 /etc/apt/keyrings \
  && wget -qO- https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null \
  && sudo chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
  && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
  && sudo apt update && sudo apt install -y gh

# ② 拉代码 + 恢复数据
git clone https://github.com/SadBoen/BoenMind.git
tar xzf BoenMind-migration-backup-20260813.tar.gz -C ~
gh auth login

# ③ 构建（服务器版 = 后端 + 前端 embed）
cd BoenMind/backend && cargo build && cargo test -p bm-core -p bm-server
cd ../frontend && pnpm install && pnpm build          # 产出 frontend/dist
cd ../backend && cargo build --release -p bm-server --features embed   # 服务器版二进制
```

> 注：Linux 上默认开发形态 = 服务器版 + 浏览器访问 `http://localhost:17321`。
> 若要构建 Tauri 桌面壳（.deb/AppImage），需额外安装 webkit2gtk 等依赖，见 Tauri 官方 Linux 前置要求。

## Windows 恢复

```powershell
# ① 基础工具
# - 安装 Rust：https://rustup.rs （rustup-init.exe，选 default host x86_64-pc-windows-msvc）
# - 安装 VS Build Tools：https://visualstudio.microsoft.com/visual-cpp-build-tools/
#   （勾选 "使用 C++ 的桌面开发"，tauri 构建必需 MSVC）
# - 安装 Node 24 LTS：https://nodejs.org
npm install -g pnpm@11
# - 安装 git：https://git-scm.com ；gh：winget install --id GitHub.cli

# ② 拉代码 + 恢复数据（%USERPROFILE%\.boenmind 即 ~/.boenmind）
git clone https://github.com/SadBoen/BoenMind.git
# 解压备份包到 C:\Users\<你>\
tar xzf BoenMind-migration-backup-20260813.tar.gz -C $env:USERPROFILE
gh auth login

# ③ 构建验证
cd BoenMind\backend; cargo build; cargo test -p bm-core -p bm-server
cd ..\frontend; pnpm install; pnpm tsc -b
# 桌面壳（便携版）：pnpm tauri build --no-bundle（依赖系统 WebView2，Win11 自带）
```

---

## 注意事项

1. **签名私钥（~/.boenmind/tauri-update.key）是唯一不可再生的**：公钥硬编码在
   `backend/crates/bm-core/src/updates.rs` 和 `frontend/src-tauri/tauri.conf.json`，
   丢失后无法再发版。**建议另存一份到离线介质/密码管理器**，不要只放在 GitHub。
2. **主仓库为 private 后，应用内"检查更新"（热升级）会失效**：更新器用未认证的
   GitHub API，私人仓库返回 401。升级需登录 GitHub 手动下载安装包。
3. `gh auth login` 在新机器上重新执行（旧机器的 keyring 凭证不迁移）。
4. 工作文件夹（默认 `~/BoenMind`）路径不同时，在应用「设置 → 工作文件夹」里改。
5. 数据库恢复：备份包内的 boenmind.db/-wal/-shm 是停服状态的一致快照，直接解压即可。
