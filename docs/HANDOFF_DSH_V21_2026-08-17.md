# HANDOFF：BoenMind × dsh v2.1 —— 下一轮开工 M1（2026-08-17）

> 状态：**交接完成，下一轮直接动手 M1**。本文档是下一轮执行的唯一迭代指针（新仓库是执行地，本仓是文档权威）。
> 关联：计划 `docs/design/DSH_PROJECT_V2_2026-08-17.md`（v2.1 定稿，本交接的宪法）；评审 `docs/review-dsh-v2/REVIEW_A/B/C_*.md`；契约台账 `D:/96_CoderWorld/boenmind-dsh/docs/CONTRACT_LEDGER_DSH.md`。

---

## 0. 一句话交接

**方向已定死（v2.1）：Rust 微内核自研后端 + 前端全套借 dsh 生态 + 插件/APP 全 Rust（独立进程、编译产物、状态外置）。** M0（前端生态基础）已完成。**下一轮做 M1：Rust 微内核骨架（kernel/ 首个 workspace），门禁 1 = mock LLM 下 headless 回合全链路 + kill -9 恢复 + crate 边界守卫。**

---

## 1. 现状盘点（已完成）

| 项 | 状态 | 位置 |
|---|---|---|
| 新仓库 | 已建 | `github.com/SadBoen/boenmind-dsh`，本地 `D:/96_CoderWorld/boenmind-dsh`（main，最新 938b6c2） |
| M0 前端生态基础 | 完成 | dsh 全家桶 `@deepseek-ai/dsh@0.1.0-rc.6` 跑通（web 3080、`__DSH_BOOT__`、插件资产 200）；毛玻璃皮肤 `dsh-frosted-window` 接入（Settings→Frosted 可切换）；`scripts/dsh.cjs` 统一 DSH_HOME |
| v2 计划 | 定稿 | 旧仓 `docs/design/DSH_PROJECT_V2_2026-08-17.md`（含 v2.1 修正） |
| 三架构师评审 | 完成 | 旧仓 `docs/review-dsh-v2/REVIEW_A_code-architecture.md` / `REVIEW_B_codebase-reviewer.md` / `REVIEW_C_ln24-architecture-auditor.md` |
| 契约台账 | 骨架 | 新仓 `docs/CONTRACT_LEDGER_DSH.md`（M2 前置产物，开工 M2 前填满） |
| 参照物 | 就绪 | dsh 源码 `D:/96_CoderWorld/deepseek-harness`（master=47f9438）；bobleer Rust 版 `D:/96_CoderWorld/bobleer-dsh-rust`（★1，20 crate 分层） |

---

## 2. 下一轮任务：M1 Rust 微内核骨架（动手清单）

### 目标与门禁
- **门禁 1（验收）**：headless 回合全链路（消息→工具→回复）在 Rust 微内核上跑通；**mock LLM 下 kill -9 恢复测试**（中断回合可续跑，事件日志无 torn-tail）；**crate 边界守卫**（依赖只许向下）。
- 核心做小：只做 loop / session / tools / llm(mock) / storage / supervisor(雏形)；mcp、真实 provider、web-server 兼容层**都不在 M1**（M1 后逐步）。

### 动手步骤（顺序即依赖）
1. **Cargo workspace**：`boenmind-dsh/kernel/` 下建 workspace，crate 划分（借鉴 bobleer 分层但按我们的微内核命名）：
   - `kernel-contracts/`：trait 与类型（LlmPort / FsPort / ShellPort / SessionPort / EventBus / 工具 schema）——**先定义端口，bobleer `PluginRuntimePort` 的 fail-loud 形状可借鉴**
   - `kernel-session/`：**append-only SessionEvent 日志（唯一事实源）** + 投影（sessions/messages/tool_calls）
   - `kernel-loop/`：回合循环（turn/step，waterfall 事件语义，对齐 dsh harness：model-visible-means-logged）
   - `kernel-tools/`：工具注册表 + 门控（enabled 名单 + fail-closed）
   - `kernel-llm/`：provider trait + **mock 实现**（门禁 1 用，不接真实 API）
   - `kernel-storage/`：sqlite 持久化后端——**fsync + 原子发布 + interrupted-turn 修复**（对齐 dsh persistence 语义；**勿照搬 bobleer 的"任务结束才整写 JSONL"**，见 §5 坑）
   - `kernel-supervisor/`：插件进程宿主**雏形**（拉起/健康检查/崩溃重启的最小实现，M3 才完整）
   - `kernel-assembly/`：组合根 + **边界守卫脚本**（依赖只许向下，仿 bobleer `check-crate-boundaries`）
2. **事件词汇**：从 dsh wire 层语义吸收（turn/step waterfall、事件序与 seq）；进程内事件先最小集（startup/step/end），够门禁 1 即可，不追求对齐（三层事件见 §4）。
3. **门禁 1 测试**：`headless` 二进制（mock LLM 应答固定工具调用序列）→ 一次完整回合；`kill -9` 后重启 → 从日志恢复续跑。
4. **提交**：旧仓提交设计决策（如有）、新仓提交 M1 代码；pre-push 质量门在旧仓（Rust 测试+clippy+前端构建），新仓暂以本地 cargo test/clippy 为准。

### 可参考的既有资产（搬语义，不搬代码）
- 旧 BoenMind `backend/crates/bm-*`：bm-kernel/bm-loop/bm-storage-turso/bm-mcp 的语义（事件日志、四件套）——**只读参考**
- `D:/96_CoderWorld/bobleer-dsh-rust`：分层与端口形状参考
- `D:/96_CoderWorld/deepseek-harness`：harness 语义与 wire 契约参考

---

## 3. 关键决策与铁律（已拍板，勿推翻）

- **10 拍板点**（v2 §七）：新仓库 / v0.1.0 起 / 不内置 Node（Rust 单二进制便携包）/ 浏览器先行 Tauri 后置 / 插件信任两档 / 浏览器自动化后置 / 历史数据只读归档 / **Rust 微内核一步到位** / **web-server 兼容层一步到位（9 面+双栅栏）** / **插件 APP 全 Rust 编译产物、闭源可选、无授权密钥**。
- **铁律（用户拍板）**：对外契约逐字对齐——路径/帧/字段/错误码、wire 事件名/负载、行为细节、挂点集合与 dsh 一致；内部实现自由。
- **进程模型（评审拍板）**：插件 = **独立进程**（弃 cdylib 摇摆）。
- **存储模型**：事件日志 = 唯一事实源；sessions/messages 为投影。
- **进程隔离 ≠ 沙箱**：第三方 worker 需降权/能力裁剪（M3）。

---

## 4. 三架构师评审要点（M1/M2 直接引用）

- **契约面 9 面 + 双栅栏**（B/C 实取）：POST RPC 信封（非 REST）/ mux 是**宿主→浏览器下行**（上行 close1008 拒绝）/ host 下行 / 静态 SPA / client.js / boot 3 槽（`__DSH_BOOT__`+`__ModuleLoader__`+`__DSH_MODULES__`）/ respond / session.export / SSE+HMR；Host-Origin 栅栏 + **16 特权方法 loopback-pin**。→ M2 填台账用。
- **三层事件**：wire 层（必复刻）/ 进程内 cordis 事件（**不上 wire，内部自由**）/ 扩展槽。→ M1 内部事件无需逐字对齐。
- **bobleer**：分层真实落地（20 crate）可借鉴；**无 web-server 实现**、缺 scope、缺 parallel、持久化弱——只借鉴分层与端口形状。
- **门禁增补**：门禁 1 加 mock LLM + kill -9 + 边界守卫（评审 A）；门禁 2 改 conformance harness wire 轨迹 diff（评审 A/B 一致）。

---

## 5. 必须避免的坑（本轮实测，勿重踩）

1. **安装坑**：dsh-base/dsh-web-app 的 `latest` 标签是旧版（0.0.1-rc.1，依赖未发布的 `dsh-bash-env`→404）；**只装主包 `@deepseek-ai/dsh` 自动带 rc.6 全家桶**。npm 10 报 EBADDEVENGINES，必须用 pnpm。
2. **DSH_HOME 坑**：dsh 默认落 `~/.dsh`，插件 add 会装错地方——**所有 dsh 操作走 `scripts/dsh.cjs`**（注入项目 DSH_HOME）。
3. **pnpm approve-builds 是交互式**，不可脚本化；native（node-pty/koffi）构建待用 onlyBuiltDependencies 批准（终端功能前处理）。
4. **settings.yaml 属运行时配置（未来含 API key），不入库**（已 gitignore + 出库）。
5. **mux 方向**：`/api/events.mux` 是下行不是上行（原稿写反，已修正）——写兼容层时别再看错。
6. **bobleer 持久化勿照搬**：任务结束才整写 JSONL，无运行中落盘、无 torn-tail 恢复——违反"Linux 长期运行"目标；M1 按 dsh 原子性语义实现。
7. **pre-push 质量门在旧仓**（Rust 测试+clippy+前端构建）；新仓仓库当前无钩子，提交前自查 cargo test/clippy。

---

## 6. 材料索引

| 材料 | 路径 |
|---|---|
| 计划（v2.1 宪法） | `BoenMind/docs/design/DSH_PROJECT_V2_2026-08-17.md` |
| 评审 A/B/C | `BoenMind/docs/review-dsh-v2/REVIEW_A_code-architecture.md` 等 |
| 契约台账（M2 填） | `boenmind-dsh/docs/CONTRACT_LEDGER_DSH.md` |
| dsh 源码 | `D:/96_CoderWorld/deepseek-harness` |
| bobleer Rust | `D:/96_CoderWorld/bobleer-dsh-rust` |
| 旧 BoenMind 语义参考 | `BoenMind/backend/crates/bm-kernel|bm-loop|bm-storage-turso|bm-mcp|bm-core` |
| 前端生态运行态 | `boenmind-dsh`（`pnpm web` 起 dsh web 3080） |

---

## 7. 遗留与待拍板

- 无阻塞项。细节执行中定夺（v2 §八 注："其余细节执行中定夺，不再逐项回审"）。
- M1 完成后：M2 web-server 兼容层（先填满契约台账 → conformance harness → 聊天闭环）。
