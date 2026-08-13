# 上游补丁台账（UPSTREAM PATCHES REGISTRY）

本文件是 **BoenMind 对 vendored 上游代码全部改动的唯一权威记录**。
升级上游（`backend/vendor/pi_agent_rust/`）时，先读本文件，按「升级流程」逐条复现补丁。

> 政策依据：改动最小化、统一标记 `BoenMind 补丁`、能不改就不改。
> 上游问题一律优先提 issue（补丁是临时方案，上游合入后删除对应补丁）。

---

## 基线信息（Baseline）

| 项 | 值 |
|---|---|
| 上游仓库 | https://github.com/Dicklesworthstone/pi_agent_rust |
| 基线 commit | `44ddf80ff1fccbeb08501c1e8eaa69f2b5dd5d92` |
| 基线版本 | v0.2.0（Cargo.toml `version`） |
| 引入方式 | 2026-08-11 去 submodule 化，源码直接入库（commit `f0aa969`） |
| 官方发布通道 | `curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/pi_agent_rust/main/install.sh` |

**权威差异核验命令**（对比上游基线，应只得到下方 6 个文件）：

```bash
git clone --filter=blob:none --no-checkout https://github.com/Dicklesworthstone/pi_agent_rust /tmp/pi_upstream
cd /tmp/pi_upstream && git checkout 44ddf80ff1fccbeb08501c1e8eaa69f2b5dd5d92
diff -rq /tmp/pi_upstream /path/to/BoenMind/backend/vendor/pi_agent_rust \
  | grep -v -E "\.git/|legacy_pi_mono_code|^Files .* are identical$"
```

> 注：上游独有 `.beads/ .claude/ .github/ tests/` 属上游元数据，vendored 版本刻意不带。
> `.cargo/config.toml` 与上游一致（非补丁）。

---

## 补丁清单（共 6 文件 / 8 处）

| # | 文件 | 位置 | 内容摘要 | 原因 | 上游 issue | 引入 commit | 源码标记 |
|---|---|---|---|---|---|---|---|
| P1 | `src/auth.rs` | L52-66 | 4 个 Google OAuth 凭据（Gemini/Antigravity 的 client-id/secret）替换为 `boenmind-placeholder-*` 占位符 | 安全：不把上游作者的私有凭据带入产品发布 | — | `f0aa969` | ✅（2026-08-13 补齐） |
| P2 | `src/providers/openai.rs` | `reasoning_style()` / 请求构造 / `OpenAIThinking` | 新增 `ReasoningStyle::MiniMax` 方言：`thinking:{type:adaptive, budget_tokens:N}` 控制推理深度；`OpenAIThinking` 增 `budget_tokens` 字段 | MiniMax 国内端点思考参数真实生效（DeepSeek 方言照搬不适用于 MiniMax） | — | `04f5191` | ✅（2026-08-13 补齐） |
| P3 | `src/session_store_v2.rs` | `artifact_file_identity()` Windows 分支 | `volume_serial_number()`/`file_index()`（unstable API #63010）改为稳定 API：`creation_time() ^ file_size()` 近似指纹 | Windows 下 stable 工具链无法编译 | [#159](https://github.com/Dicklesworthstone/pi_agent_rust/issues/159) | `b79a87d`（注释补链接 `5d003b3`） | ✅ |
| P4 | `src/sdk.rs` | `SessionOptions` + `create_agent_session()` | 新增 `compaction_settings: Option<ResolvedCompactionSettings>` 透传，覆盖会话级压缩水线/尾部预算/上下文窗口；`None` 保持原行为 | bm-core 按模型 50% 水线注入压缩 | [#160](https://github.com/Dicklesworthstone/pi_agent_rust/issues/160) | `e01c498` | ✅ |
| P5 | `src/sdk.rs` | `SessionOptions` + `create_agent_session()` | 新增 `ui_handler: Option<Arc<dyn ExtensionUiHandler>>` 透传 + mpsc 桥接（tokio::spawn 后台任务）；无响应 fail-closed | 插件权限询问链路（前端弹窗） | [#161](https://github.com/Dicklesworthstone/pi_agent_rust/issues/161) | `42e29b2` | ✅ |
| P6 | `Cargo.toml` | dev-dependencies | 新增 `tokio = { workspace = true }`（P5 桥接任务用，避免新增独立版本） | P5 的配套依赖 | — | `42e29b2` | ✅ |
| P7 | `src/tools.rs` | L4213 / L4310 | `from_mode(mode)` → `from_mode(u32::from(mode))`；`dev()` 比较包 `i128::from` | macOS/平台类型差异编译修复 | — | `f0aa969` | ✅（2026-08-13 补齐） |
| P8 | `src/tools.rs` | L11224 / L11416 / L11501 | 测试 fixture 中 `sk-redactionfixture*` → `sk_redactionfixture*`（3 处） | 避免 secret 脱敏逻辑把测试 fixture 当真实 key 误扫 | — | `f0aa969` | ✅（2026-08-13 补齐） |

| P9 | `src/subagents.rs` | `execute()` ToolOutput 构建 + 新增 `structured_result_block`/`truncate_owned` | content 文本末尾追加 `<subagent-structured-result>` 紧凑 JSON 块（output/stderr 截断 2000 字符、总块 16KB）；上游 `details` 在 providers/openai.rs 序列化时被丢弃，模型只读 content 文本 | 子代理结构化返回：队长代理像读函数返回值一样直接取用结果 | [#163](https://github.com/Dicklesworthstone/pi_agent_rust/issues/163)（以默认关闭配置项形式建议，预期长期自持） | 本轮 | ✅ |
| P10 | `src/extensions.rs` | `discover_sibling_index_entries()` | bundle 探测加"cluster_root 名为 extensions 时跳过"保护（与 `discover_sibling_extension_entries` 一致） | 上游缺陷：多个 entrypoint=index.ts 的独立插件共存于扩展根时被 bundle 探测互相认领，触发 "Ambiguous JS extension ownership"（实测 3 插件共存必现） | [#164](https://github.com/Dicklesworthstone/pi_agent_rust/issues/164)（建议按 sibling 函数同样保护合入） | 本轮 | ✅ |

**复现命令**（每个补丁的完整 diff，按 commit 精确取回）：

```bash
git show f0aa969   -- backend/vendor/pi_agent_rust/src/auth.rs backend/vendor/pi_agent_rust/src/tools.rs   # P1 P7 P8
git show 04f5191   -- backend/vendor/pi_agent_rust/src/providers/openai.rs                                  # P2
git show b79a87d 5d003b3 -- backend/vendor/pi_agent_rust/src/session_store_v2.rs                             # P3
git show e01c498   -- backend/vendor/pi_agent_rust/src/sdk.rs                                                # P4
git show 42e29b2   -- backend/vendor/pi_agent_rust/src/sdk.rs backend/vendor/pi_agent_rust/Cargo.toml        # P5 P6
```

---

## asupersync 补丁（vendor 化依赖，非 pi_agent_rust）

> 自 2026-08-14 起，`asupersync`（pi 引擎的底层异步运行时）由 crates.io 依赖改为
> **vendor 化本地依赖**：`backend/vendor/asupersync/` + workspace `[patch.crates-io]`。
> 原因：上游 crates.io 0.3.10 在 Windows 上存在连接误判 bug（见下），需要本地补丁且
> 补丁必须可复现（cargo 缓存不持久）。asupersync 与 pi_agent_rust 同作者
> （Dicklesworthstone），修复路径共用。

### 基线信息

| 项 | 值 |
|---|---|
| 上游仓库 | https://github.com/Dicklesworthstone/asupersync |
| 基线版本 | 0.3.10（crates.io） |
| vendor 引入 commit | 2026-08-14（本轮） |
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

1. **锁定新基线**：上游新 commit 打 tag（如 v0.2.1）。
2. **全量替换**：用新版本覆盖 `backend/vendor/pi_agent_rust/`（保留本台账，它在 `vendor/` 同级、不被覆盖）。
3. **核验差异**：跑「权威差异核验命令」，对照上表 6 文件逐一确认。
   - 上游已合入的 issue（#159/#160/#161）→ **删除对应补丁**（P3/P4/P5），代码回到上游实现；
   - 上游未合入的 → 按本表逐条 `git show` 重新应用。
4. **编译验证**：`cargo check`（bm-server），`cargo test -p bm-core -p bm-server`。
5. **回归**：聊天全链路 + 插件权限询问 + 压缩水线实测。
6. **更新台账**：修改基线信息、标记已删除/已变更的补丁行。

---

## 关联文档

- 补丁设计背景：
  - 压缩补丁（P4）：`docs/context-compression-plan.md`、上游 issue #160
  - 权限询问补丁（P5/P6）：上游 issue #161（询问语义建议 #162 待上游拍板）
  - 思考方言补丁（P2）：`docs/thinking-levels.md`（思考档位动态出档）
- 政策记忆：pi 修补政策（改动最小化/打标记/能不改就不改）、上游问题提 issue（补丁临时方案）
