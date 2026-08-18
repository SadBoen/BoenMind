# BoenMind（dsh 内核版）

以 Rust 微内核自研后端 + 借 DeepSeek Harness 前端生态的 BoenMind 新一代。

> 方向 v2.3 定稿（2026-08-19）：**core 独立 = dsh-rust-core 纯内核库（submodule）+ 组合根外移到
> BoenMind 产品层 + Rust 插件集中 plugins/（一插件一子文件夹）+ QuickJS 桥（rquickjs）兼容官方生态**。
> 四工件：`kernel`（纯内核库）→ `bm/assembly`（唯一装配点）→ `bm/web-server`/`bm/headless`
> （最终程序）→ `plugins/*` + `bm/quickjs-bridge`（实现）。
> 计划：`docs/design/PLUGIN_ARCH_2026-08-19.md` ｜ 评审：`docs/review-dsh-rust-core-2026-08-18/grok-arch-review-3repo-2026-08-19.md`

## 当前状态

- **核心 = dsh-rust-core（git submodule）**：Rust 纯内核库（4 crate：contracts/session/storage/
  supervisor）在 `SadBoen/dsh-rust-core` 独立仓演进，本仓库 `kernel/` 是其 submodule 指针。
  更新核心：`cd kernel && git pull && cd .. && git add kernel && git commit`。**首次 clone 本仓须 `--recursive`**。
- **产品层 = BoenMind 主仓 workspace**：组合根（`bm/assembly`）+ 最终程序（`bm/web-server` /
  `bm/headless`）+ Rust 插件（`plugins/`）+ QuickJS 桥（`bm/quickjs-bridge`）。
- **M0 前端生态基础（已完成）**：dsh 全家桶 bootstrap 跑通（web 3080）+ DSH_HOME 统一启动器 `scripts/dsh.cjs`。

## 快速开始

```bash
git clone --recursive git@github.com:SadBoen/BoenMind.git   # 或 clone 后 git submodule update --init
pnpm install          # dsh 前端生态（M0）
pnpm web              # 启动 dsh web（前端宿主，http://127.0.0.1:3080）

# Rust 产品层
cargo build --workspace        # 编译全部 crate（含 kernel 4 库）
cargo test --workspace         # 全量测试（含 crate 边界守卫）
bash scripts/verify-gate1.sh   # 门禁 1：roundtrip + kill-9 恢复 + 尾部完整性
```

## 目录

```
kernel/      Rust 纯内核库 —— git submodule → SadBoen/dsh-rust-core（4 crate）
bm/          产品层 workspace：assembly（组合根）/ web-server / headless / quickjs-bridge
plugins/     Rust 插件（一插件一子文件夹：plugin-llm/plugin-loop/plugin-tools）
dsh-home/    dsh 全家桶 profile（前端生态基座，快照重抓源）
docs/        设计/交接/审查文档
scripts/     门禁/热升级验证脚本
.tmp/        验证工具（conformance/gate25/仿真/vision，不入库）
```

## 分支

- `main`：dsh 内核版（本仓库当前真身，v0.1.0 起）
- `legacy/pi-kernel`：换核心之前的 pi 内核时代全部代码与文档（废弃保留，只读）
