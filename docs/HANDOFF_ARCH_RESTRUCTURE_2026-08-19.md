# HANDOFF：四工件架构重构（组合根外移 + 插件集中 + QuickJS 桥占位）（2026-08-19）

> 状态：**已落地并全绿**。BoenMind 主仓从"无 Cargo 工程"变为**产品层 workspace**；
> kernel 从"10 crate + 最终程序"精简为**纯内核库 4 crate**；组合根外移到 `bm/assembly`；
> Rust 插件集中 `plugins/`（一插件一子文件夹）；QuickJS 桥 `bm/quickjs-bridge` 占位。

---

## 1. 一句话交接

按 grok 评审定稿的四工件架构完成重构：**kernel（纯内核库）→ bm-assembly（唯一装配点）
→ bm-web-server / bm-headless（最终程序）→ plugins/* + quickjs-bridge（实现）**。
用户四点拍板全部落地：① core 独立（submodule，对齐上游）② 组合根外移 + 插件集中
BoenMind 一个插件一个子文件夹 ③ QuickJS 桥放 BoenMind（rquickjs）兼容官方生态
④ 好用插件吸收进 BoenMind 做 Rust 版。**验证全绿：108 测试 + clippy + gate1 + 起服。**

---

## 2. 最终结构

```
BoenMind/                          ← 产品层 workspace（组合根 + 程序 + 插件 + 桥）
  Cargo.toml                       ← workspace：bm/* + plugins/*
  bm/assembly/                     ← 组合根（唯一装配点，原 kernel-assembly 改名 bm-assembly）
  bm/web-server/                   ← 产品运行时（原 kernel/web-server，含 frontend 快照）
  bm/headless/                     ← 门禁工具（原 kernel/headless）
  bm/quickjs-bridge/               ← QuickJS 桥占位（rquickjs，宿主 API 面见设计文档）
  plugins/plugin-llm/              ← Rust 插件（吸收自 dsh-rust-plugins，一插件一子文件夹）
  plugins/plugin-loop/
  plugins/plugin-tools/
  scripts/verify-gate1.sh          ← 门禁 1（从 kernel 迁来）
kernel/                            ← git submodule → SadBoen/dsh-rust-core（纯内核库 4 crate）
  kernel-contracts / kernel-session / kernel-storage / kernel-supervisor
dsh-rust-plugins/                  ← 独立插件源仓（保留；吸收进 BoenMind 锁 commit，不双向同步）
```

**分层（依赖只许向下，边界守卫 `bm/assembly/tests/crate_boundaries.rs` 门禁）：**
layer 5 contracts → layer 4 session/storage → layer 3 supervisor → layer 2 插件实现 →
layer 1 组合根 → layer 0 最终程序/桥。

## 3. 做了什么

| 动作 | 内容 |
|---|---|
| BoenMind 建 workspace | 根 Cargo.toml（bm/* + plugins/* 成员，workspace.dependencies 对齐 kernel） |
| 组合根外移 | `kernel-assembly` → `bm/assembly`，**改名 `bm-assembly`**（`use kernel_assembly` → `use bm_assembly`） |
| 程序外移 | `kernel/web-server` → `bm/web-server`（含 12MB frontend 快照）、`kernel/headless` → `bm/headless`；`--dist` 默认路径改 `bm/web-server/frontend` |
| 插件集中 | `dsh-rust-plugins` 三 crate 复制进 `plugins/`（path 依赖改指 `kernel/`） |
| kernel 精简 | 删 assembly/headless/web-server；Cargo.toml 只留 4 库；README 重写为纯内核定位；gate1 脚本迁主仓 |
| QuickJS 桥 | `bm/quickjs-bridge` 占位 + 设计文档 `docs/design/QUICKJS_BRIDGE_DESIGN_2026-08-19.md` |
| 边界守卫重写 | 适配 bm/ plugins/ kernel/ 三目录 + 包名从 Cargo.toml 读（目录名≠包名）+ 新分层表 |
| 主仓文档 | README 目录结构更新；.gitattributes 前端 linguist 标记；frontend README 改"官方 38 无皮肤" |

## 4. 验证矩阵（全绿）

| 项 | 结果 |
|---|---|
| `cargo test --workspace`（BoenMind） | 108 测试全过（含边界守卫 1 + minimal_three_plugins 6 + web-server 22 + 插件 52） |
| `cargo clippy --workspace --all-targets -D warnings` | 零警告 |
| `bash scripts/verify-gate1.sh` | GATE1: ALL PASS（roundtrip + kill-9 恢复 + 尾部完整性 11 步） |
| `cargo check`（kernel 独立） | 4 库纯内核构建通过 |
| web-server 起服 | release 起服：index.html 200 + __DSH_BOOT__ 注入 + boot.json 200，dist 指向新路径 |

## 5. 待办（下轮）

1. **quickjs-bridge 实现**（按设计文档 §5 顺序）：host 面契约测试 → rquickjs 桥接 → manifest 驱动 → 接真 LLM。
2. **dsh-rust-plugins 源仓打 tag**：三插件已复制进 BoenMind/plugins，源仓应打 tag 锁 commit，
   之后吸收机制 = 源仓冻结 → 产品 crate 吸收（不双向同步）。
3. **web-server 的 conformance / gate25 / m3-r3 / hot-replace 脚本**：路径要跟到 BoenMind
   （.tmp/ 下脚本引用 `kernel/web-server` 旧路径的需更新）。
4. kernel `docs/harness-surface-comparison.md` 是历史对照（旧布局描述），保留；新布局见本交接。

## 5.5 P0 装配点收口（2026-08-19 同轮完成，grok 评审驱动）

grok 评审指出"唯一组合根被 web-server 直接 new 具体 provider 挖空"。已收口：

- **`bm-assembly` 新增 `config` / `provider` 模块**：`provider_config.rs` 从 web-server 迁入
  （单源），新增 `assemble_providers`（配置→OpenAICompatLlm→MultiProviderLlm 聚合）+
  `LlmAdapter` trait（L0 消费适配器的端口，隐藏具体实现）。
- **`Runtime::apply_llm`**：唯一装配出口（`&mut self`，swap 聚合 LLM + 写默认 provider/model）。
- **web-server main.rs**：删 plugin-llm 依赖，只调 `bm_assembly::apply_llm`；删 `provider_config.rs`。
- **headless**：删 plugin-llm 依赖，用 `bm_assembly::scripted_llm` / `MockTurn`。
- **边界守卫改严**：L0（web-server/headless/quickjs-bridge）**禁止依赖 plugin-\***，
  具体插件只能在 bm-assembly（L1）装配——防第二组合根，测试钉死。
- 验证：108 测试 + clippy + GATE1 + 起服（host.describe mock provider 正常）全绿。

## 6. 环境纪律（沿用）

- 每轮先杀 web-server：`taskkill //F //IM web-server.exe`
- 跑服务用 release（debug exe 2GB 超 PE 限制）
- 跨仓 path：kernel 子模块 + plugins 都在 BoenMind 内（不再依赖 D:/96_CoderWorld 兄弟仓位置）
- 验证三件套：cargo test / clippy / gate1（BoenMind 主仓）
