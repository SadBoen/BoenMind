# BoenMind

个人知识管理软件：LLM-WIKI 路线 + 个人 Agent。
Rust 微内核（`kernel/` submodule）+ 产品层（`bm/`）+ 功能插件（`plugins/`）+ React 前端（`frontend/`）。

## 形态

| 形态 | 产物 | 说明 |
|---|---|---|
| **Windows 便携版** | `BoenMind_*_x64_portable.zip` | 免安装，多文件（BoenMind.exe + dist/），解压即用 |
| **Linux 服务器版** | `boenmind-server_*_linux-*.tar.gz` | web-server + 前端 dist + systemd 三件套，`sudo bash install.sh` 安装 |

> 桌面壳（Tauri）已移除：前端现为纯 Web 应用，由 web-server 直接托管 `frontend/dist`，
> 浏览器访问即可（本地 `http://127.0.0.1:3080`）。

## 构建

```bash
# 前端（React 19 + Vite 8 + Tailwind v4 + daisyUI 5）
cd frontend && npm install && npm run build

# Rust workspace（web-server 二进制）
cd .. && cargo build --release -p web-server

# 统一打包脚本（Windows 便携版 / Linux 服务器版）
bash scripts/package.sh --win
bash scripts/package.sh --linux
```

本地开发：`cd frontend && npm run dev`（Vite 反代 `/api` → `127.0.0.1:3080`，
`changeOrigin:false` 以过后端信任栅栏），另起 `cargo run -p web-server`。

## 服务器部署（Linux/Debian）

```bash
tar xzf boenmind-server_*_linux-x86_64.tar.gz
cd boenmind-server_*/ && sudo bash install.sh
# 浏览器访问 http://服务器IP:17321
```

详见 `packaging/linux/README.md`。

## 发布（GitHub Releases）

打 tag 触发 `.github/workflows/release.yml` 自动构建产物并创建 Release：

```bash
git tag v0.1.4 && git push origin v0.1.4
```

## 架构

```
kernel-contracts / kernel-session / kernel-storage / kernel-supervisor  ← kernel/ submodule（纯内核库）
bm/ports          产品级契约层（Compactor / ToolRegistryPort / ToolGatePort）
plugins/*         产品插件（只依赖契约层，插件间零依赖）
bm/assembly       组合根（唯一装配点）
bm/web-server / bm/headless / bm/quickjs-bridge   ← L0 最终程序
frontend/         React 19 + Vite 8 + Tailwind v4 + daisyUI 5（纯 Web，RPC + 双 WS 下行流）
```

分层纪律「依赖只许向下」由 `bm/assembly/tests/crate_boundaries.rs` 硬守卫（`cargo test --workspace`）。

## 验证

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cd frontend && npx tsc -b && npm run build
```

前端规格与交接见 `docs/FRONTEND-REQUIREMENTS.md`（需求）→ `docs/FRONTEND-GUIDE.md`（实现规格）→
`docs/FRONTEND-HANDOFF.md`（交接快照）；项目级交接与审计台账见 `docs/HANDOFF.md` 与根目录 `QUESTIONS.md`。
