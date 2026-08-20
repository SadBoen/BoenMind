# BoenMind

个人知识管理软件：LLM-WIKI 路线 + 个人 Agent。
Rust 微内核（`kernel/` submodule）+ 产品层（`bm/`）+ 功能插件（`plugins/`）+ React 前端（`frontend/`）。

## 形态

| 形态 | 产物 | 说明 |
|---|---|---|
| **Windows 便携版** | `BoenMind_*_x64_portable.zip` | 免安装，多文件（BoenMind.exe + dist/），解压即用 |
| **Windows 桌面版** | `BoenMind_*_x64-setup.exe` | Tauri 无边框窗口 + 应用内自动更新（热更新） |
| **Linux 服务器版** | `boenmind-server_*_linux-*.tar.gz` | web-server + 前端 dist + systemd 三件套，`sudo bash install.sh` 安装 |

## 构建

```bash
# 前端
cd frontend && npm install && npm run build

# Rust workspace（web-server 二进制）
cd .. && cargo build --release -p web-server

# 统一打包脚本（Windows 便携版 / Linux 服务器版）
bash scripts/package.sh --win
bash scripts/package.sh --linux

# 桌面版（Tauri，需 MSVC；详见 frontend/src-tauri/）
cd frontend && npx tauri build
```

## 桌面版（Tauri 壳）

- 无边框窗口（`decorations: false`），底部状态栏即拖拽区（`data-tauri-drag-region`）
- 应用内热更新：`tauri.conf.json` 的 `plugins.updater.endpoints` 指向更新清单
  - 默认 GitHub Releases `latest.json`
  - 自建服务器：web-server 加 `--update-dir <dir>` 参数，`GET /update/*` 静态托管
- 更新清单生成：`bash scripts/gen-latest.sh <ver> <签名> <URL>`

### 签名密钥（一次性）

```bash
# 生成密钥对（私钥守好，勿入库）
npx @tauri-apps/cli signer generate -w ~/.boenmind/tauri-update.key
# 将公钥（signer 输出）写入 frontend/src-tauri/tauri.conf.json 的 plugins.updater.pubkey
```

## 服务器部署（Linux/Debian）

```bash
tar xzf boenmind-server_*_linux-x86_64.tar.gz
cd boenmind-server_*/ && sudo bash install.sh
# 浏览器访问 http://服务器IP:17321
```

详见 `packaging/linux/README.md`。

## 发布（GitHub Releases）

打 tag 触发 `.github/workflows/release.yml` 自动构建三平台产物并创建 Release：

```bash
git tag v0.1.4 && git push origin v0.1.4
```

仓库 Secrets 需配置 `TAURI_SIGNING_PRIVATE_KEY`（base64 私钥）与密码（桌面版 updater 签名用）。

## 架构

```
kernel-contracts / kernel-session / kernel-storage / kernel-supervisor  ← kernel/ submodule（纯内核库）
bm/ports          产品级契约层（Compactor / ToolRegistryPort / ToolGatePort）
plugins/*         产品插件（只依赖契约层）
bm/assembly       组合根（唯一装配点）
bm/web-server / bm/headless / bm/quickjs-bridge   ← L0 最终程序
frontend/         React 19 + dockview + theme 四档
frontend/src-tauri/   Tauri 桌面壳（无边框 + updater）
```

分层纪律「依赖只许向下」由 `bm/assembly/tests/crate_boundaries.rs` 硬守卫（`cargo test --workspace`）。

## 验证

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
bash scripts/verify-gate1.sh
```

详见 `docs/HANDOFF.md`。