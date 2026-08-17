# BoenMind（dsh 内核版）

以 Rust 微内核自研后端 + 借 DeepSeek Harness 前端生态的 BoenMind 新一代。

> 方向 v2.1 定稿（2026-08-17）：**后端全 Rust 微内核（loop/session/tools/llm/storage/mcp/supervisor）+ 插件/APP 全 Rust（编译产物分发、进程隔离、状态外置）+ 前端全套借 dsh 生态（官方 web-app UI/皮肤/ui-slots）**，两者由 Rust web-server 协议兼容层接通（对外契约逐字对齐）。
> 计划：`docs/design/DSH_PROJECT_V2_2026-08-17.md` ｜ 交接：`docs/HANDOFF_DSH_V21_2026-08-17.md` ｜ 评审：`docs/review-dsh-v2/` ｜ 契约台账：`docs/CONTRACT_LEDGER_DSH.md`

## 当前状态

- **M0 前端生态基础（已完成）**：dsh 全家桶 bootstrap 跑通（web 3080）+ 毛玻璃皮肤接入 + DSH_HOME 统一启动器 `scripts/dsh.cjs`。
- **M1 起**：Rust 微内核骨架（loop/session/tools/llm/storage/supervisor）。

## 快速开始

```bash
pnpm install          # dsh 前端生态（M0）
pnpm web              # 启动 dsh web（前端宿主，http://127.0.0.1:3080）
```

## 目录

```
kernel/      Rust 微内核（loop/session/tools/llm/storage/mcp/supervisor）
plugins/     Rust 插件（team/steward/memory/audit/browser/skins）
web-server/  Rust 协议兼容层（dsh 前端接口合同 9 面 + 双栅栏）
frontend/    dsh web-app 产物 + 皮肤（借来的）
shell/       Tauri 2 桌面壳（后置）
apps/        Rust APP（编译产物分发，闭源可选）
```

## 分支

- `main`：dsh 内核版（本仓库当前真身，v0.1.0 起）
- `legacy/pi-kernel`：换核心之前的 pi 内核时代全部代码与文档（废弃保留，只读）
