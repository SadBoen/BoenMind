# 上游补丁台账（UPSTREAM PATCHES REGISTRY）

本文件是 **BoenMind 对 vendored 上游代码全部改动的唯一权威记录**。
升级上游（`backend/vendor/asupersync/`）时，先读本文件，按「升级流程」逐条复现补丁。

> 政策依据：改动最小化、统一标记 `BoenMind 补丁`、能不改就不改。
> 上游问题一律优先提 issue（补丁是临时方案，上游合入后删除对应补丁）。

---

## 现状：vendored 范围（2026-08-15 收窄）

- **asupersync**（`backend/vendor/asupersync/`）：唯一存活的 vendored 依赖
  （bm-compat 的 http/TLS 底层运行时），A1/A2 补丁见下文。
- **pi_agent_rust**：已于 2026-08-15 随 pi 废除收尾**整体删除**（原
  `backend/legacy/pi_agent_rust/`，commit 见 git 历史）。其补丁 P1-P12 仅作历史归档
  保留在本文件下文——代码已不存在，补丁不可再应用，`git show` 仍可从历史取回。

---

## 归档区：pi_agent_rust 补丁（2026-08-15 删除，仅存档）

> 上游仓库 https://github.com/Dicklesworthstone/pi_agent_rust，基线
> `44ddf80ff1fccbeb08501c1e8eaa69f2b5dd5d92`（v0.2.0），2026-08-11 去 submodule 化
> 源码直接入库（`f0aa969`）。引擎能力已由自研 bm-loop 接替（HANDOFF_KERNEL_PHASE1.md），
> 插件兼容层由 bm-compat（拷入子集 + 自研 shim）接替——本区仅留档，供溯源。

| # | 文件 | 位置 | 内容摘要 | 上游 issue | 引入 commit |
|---|---|---|---|---|---|
| P1 | `src/auth.rs` | L52-66 | 4 个 Google OAuth 凭据替换为 `boenmind-placeholder-*` 占位符 | — | `f0aa969` |
| P2 | `src/providers/openai.rs` | `reasoning_style()` / 请求构造 | 新增 `ReasoningStyle::MiniMax` 方言（thinking adaptive + budget_tokens） | — | `04f5191` |
| P3 | `src/session_store_v2.rs` | `artifact_file_identity()` Windows 分支 | 稳定 API 替代 unstable `volume_serial_number()`/`file_index()` | [#159](https://github.com/Dicklesworthstone/pi_agent_rust/issues/159) | `b79a87d`（`5d003b3` 补链接） |
| P4 | `src/sdk.rs` | `SessionOptions` + `create_agent_session()` | `compaction_settings` 透传（压缩水线/尾部预算/窗口） | [#160](https://github.com/Dicklesworthstone/pi_agent_rust/issues/160) | `e01c498` |
| P5 | `src/sdk.rs` | `SessionOptions` + `create_agent_session()` | `ui_handler` 透传 + mpsc 桥接（fail-closed） | [#161](https://github.com/Dicklesworthstone/pi_agent_rust/issues/161) | `42e29b2` |
| P6 | `Cargo.toml` | dev-dependencies | `tokio = { workspace = true }`（P5 配套） | — | `42e29b2` |
| P7 | `src/tools.rs` | L4213 / L4310 | `from_mode(u32::from(mode))` 平台差异修复 | — | `f0aa969` |
| P8 | `src/tools.rs` | L11224 / L11416 / L11501 | 测试 fixture key 改名防误扫 | — | `f0aa969` |
| P9 | `src/subagents.rs` | `execute()` ToolOutput | `<subagent-structured-result>` 紧凑 JSON 块（截断 2000 字符/16KB） | [#163](https://github.com/Dicklesworthstone/pi_agent_rust/issues/163) | 2026-08-13 |
| P10 | `src/extensions.rs` | `discover_sibling_index_entries()` | bundle 探测跳过名为 extensions 的 cluster_root | [#164](https://github.com/Dicklesworthstone/pi_agent_rust/issues/164) | 2026-08-13 |
| P11 | `tests/common/mod.rs` | 曾补最小桩 `TestHarness` | **已撤销**（2026-08-14 重构：legacy 移出 workspace，test 不再编译） | — | `4b85bb1` 引入 |
| P12 | `Cargo.toml` | `tokio` 显式版本 | legacy 独立编译用（文件已删，失效） | — | 2026-08-14 |

---

## asupersync 补丁（唯一存活 vendored 依赖）

> 自 2026-08-14 起，`asupersync`（pi 引擎的底层异步运行时）由 crates.io 依赖改为
> **vendor 化本地依赖**：`backend/vendor/asupersync/` + workspace `[patch.crates-io]`。
> 原因：上游 crates.io 0.3.10 在 Windows 上存在连接误判 bug（见下），需要本地补丁且
> 补丁必须可复现（cargo 缓存不持久）。asupersync 与 pi_agent_rust 同作者
> （Dicklesworthstone），修复路径共用。10057 问题档案：`backend/vendor/ASUPERSYNC_ISSUE_10057.md`。

### 基线信息

| 项 | 值 |
|---|---|
| 上游仓库 | https://github.com/Dicklesworthstone/asupersync |
| 基线版本 | 0.3.10（crates.io） |
| vendor 引入 commit | 2026-08-14（pi_agent_rust 依存期）；2026-08-15 随 legacy 拆除移入 `backend/vendor/` |
| 上游依赖声明 | pi_agent_rust `Cargo.toml` 声明 `asupersync = "0.3.9"`；上游 `Cargo.lock` 锁 0.3.9（**比本地 0.3.10 更旧，无下文修复**） |

### 补丁清单

| # | 文件 | 位置 | 内容摘要 | 原因 | 上游 issue |
|---|---|---|---|---|---|
| A1 | `src/net/tcp/stream.rs` | `wait_for_connect_fallback()`（Windows 分支） | 连接完成检测改用 `WSAPoll` 内核 WRITABLE 事件，替代 `peer_addr()` 轮询 | 部分 Windows 网络栈（实测 Win10 19044 直连阿里云）`getpeername()` 在 TCP connect 真正完成前就返回成功 → 误判"已连接" → 首笔 send 报 WSAENOTCONN(10057) | [#62](https://github.com/Dicklesworthstone/asupersync/issues/62) |
| A2 | `src/net/tcp/stream.rs` | `poll_write()`（Windows 分支） | WSAENOTCONN 重试由"纯次数上限"改为"100ms 真实时间窗口"（新增 `first_10057_at` 字段） | A1 误判发生后，原 4096 次忙等重试在连接完成（~40ms）前耗尽预算 → TLS 握手必败；时间窗口保证慢连接有机会完成 | [#62](https://github.com/Dicklesworthstone/asupersync/issues/62) |

**验证**：修复前约 50% 请求失败（连接目标 IP 快慢决定）；修复后 API 多轮 + 前端 UI 全链路
0 失败（含最慢连接场景，后端处理耗时 2-3s 正常）。

**升级流程**：上游合入对应修复后，删除 A1/A2 补丁并将 `[patch.crates-io]` 移除、
恢复 crates.io 依赖（`cargo update -p asupersync`）。

---

## 升级流程（上游出新版本时）

1. **锁定新基线**：上游新 commit 打 tag（如 0.3.11）。
2. **全量替换**：用新版本覆盖 `backend/vendor/asupersync/`（保留本台账）。
3. **核验差异**：对照「补丁清单」A 区逐文件确认；上游已合入的（#62）→ 删除对应补丁，
   代码回到上游实现；未合入的 → 按本表重新应用。
4. **编译验证**：`cargo check -p bm-compat`，`cargo test -p bm-compat --test host --test load --test execute --test events --test session`。
5. **回归**：聊天全链路（asupersync 是 http 层，重点验证 TLS/长请求不 10057）。
6. **更新台账**：修改基线信息、标记已删除/已变更的补丁行。

---

## 关联文档

- 10057 问题档案：`backend/vendor/ASUPERSYNC_ISSUE_10057.md`
- bm-compat 拷入来源与同步纪律：`backend/crates/bm-compat/DEPENDENCIES.md`
- 政策记忆：pi 修补政策（改动最小化/打标记/能不改就不改）、上游问题提 issue（补丁临时方案）

---

## bm-compat 同步区（主线 B 拆法 A，2026-08-14 登记）

`backend/crates/bm-compat/` 从 pi_agent_rust（已删，基线 44ddf80）拷贝 6 文件作库
（extensions_js.rs / scheduler.rs / hostcall_queue.rs / hostcall_io_uring_lane.rs /
embedded_assets.rs / error.rs，共 45K 行），目标 = pi.dev 插件兼容层
（详见 crates/bm-compat/DEPENDENCIES.md）。

**同步纪律**（2026-08-15 起上游已不随版本升级——pi_agent_rust 源码已删，本区仅存档）：
1. ~~上游升级（本目录替换新基线）后，对上述 6 文件逐文件 diff，把上游变更同步到 bm-compat/src/；~~
2. ~~本台账 P1-P12 补丁若触及这 6 文件，bm-compat 副本必须同步应用；~~
3. ~~bm-compat 侧的 shim 层随上游符号变化同步更新；~~
4. ~~每次同步后跑 `cargo check -p bm-compat`（加入 workspace 后为 CI 门禁）。~~

**当前状态**：6 文件拷入与 shim 适配已完成（B1-B6 全落地）；bm-compat 现存唯一
vendored 关联 = asupersync（其 A1/A2 补丁如触及 bm-compat 代码须同步应用）。

### bm-compat 本地删减（2026-08-15 登记，非上游补丁）

| 项 | 文件 | 内容 | 理由 |
|---|---|---|---|
| L1 | `legacy_pi_mono_code/`（目录） | pi-mono models.generated.ts 整目录删除 | 嵌入后零运行时消费者（全仓 grep 无调用点，仅 CRC 自测）；随 pi 废除收尾 |
| L2 | `build.rs` | EMBEDDED_TEXT_ASSETS 删 legacy 条目（4→3），头注释更新 | 同 L1 |
| L3 | `src/embedded_assets.rs` | 删 `legacy_models_generated_ts`/`legacy_models_generated_ts_crc32c` 访问器 + LEGACY_MODELS_GENERATED_TS static + 测试断言段 | 同 L1；embedded_assets.rs 自本项起不再逐字节等于上游（上游已删无从对比，仅存档） |
