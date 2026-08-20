# BoenMind 交接文档（2026-08-20）

给下一轮对话/协作的接手者。这是「全面回头看 + 连续五轮架构修复」之后的仓库状态快照。

## 一、仓库一句话

BoenMind = Rust 微内核（`kernel/` 只读 submodule `dsh-rust-core`）+ 产品层（`bm/`）+ 功能插件（`plugins/`）+ 自研前端（`frontend/`，React 19 + dockview）。分层纪律「依赖只许向下」由 `bm/assembly/tests/crate_boundaries.rs` 硬守卫（`cargo test --workspace` 即门禁）。

## 二、最近改动（都已推 origin/main）

| commit | 内容 |
|---|---|
| `c2df2cf` | 回头看修复：Compactor trait 上提 `bm/ports`（产品契约层）、crate 边界守卫补规则 3（核心禁依赖功能插件）、`[compaction]` config 段收口 |
| `8e070c7` | 大手术：loop→tools 端口化（ToolRegistryPort/ToolGatePort 进 bm-ports）、plugin-loop 拆 assemble.rs、api.rs 拆 credentials 起范、supervisor 去死依赖、jsonschema feature-gate |
| `c1dd392` | api.rs 2751→1636 行领域拆分（auth/credentials/llm/workspace/host/settings 六子模块）、assembly 门面收敛、auth 存储边界文档化 |
| `e8489e9` | 交接文档入库（docs/HANDOFF.md） |
| `fc4e797` | **session 域拆完 api.rs→1102 行 + workspace lints 收口 + 待办评估定案**（见第五节状态） |

## 三、当前架构分层（最终形态）

```
kernel-contracts (纯契约)              ← kernel/ submodule，只读不可改
kernel-session / kernel-storage
kernel-supervisor                     ← 死代码（无引用；只有大小 326 行雏形，M3 才接）
bm/ports                              ← 产品级契约层：Compactor / ToolRegistryPort / ToolGatePort
plugins/plugin-llm loop tools auth compactor
bm/assembly                           ← 组合根（唯一装配点）
bm/web-server / bm/headless / bm/quickjs-bridge   ← L0，只依赖 bm-assembly
frontend/                             ← React 19 + dockview + theme 四档
```

**核心插件（loop/llm/tools）只依赖 kernel-contracts + kernel-session + bm-ports**——不再编译期碰任何兄弟插件/功能插件。这是五轮修复的核心成果。

## 四、关键设计决策（勿推翻，除非有强理由）

1. **Compactor trait 在 bm-ports，不在 plugin-compactor**。kernel/ 只读塞不进 kernel-contracts，产品级策略端口统一放 bm-ports。新增核心插件需要的端口一律放这里（也只依赖 kernel-contracts + 无实现）。
2. **bm-assembly 的 re-export（MockTurn/DEFAULT_PASSWORD/DefaultCompactor/scripted_llm）是守卫强制的 L0 出口，不是门面污染**。L0 只依赖 bm-assembly，装配参数必须经组合根 re-export。**别砍**（曾评估过，砍了 L0 就满足不了「只依赖 bm-assembly」）。
3. **auth 双持久化是有意的 bounded context**。`auth.json`/`sessions.jsonl` 是认证域自管，不经 kernel-storage（sqlite 只承载事件日志=唯一事实源）。**不要**把认证存储迁进 sqlite 事件表（plugin-auth/src/lib.rs 模块 doc 有定稿说明）。
4. **事件日志 = 唯一事实源**；上下文压缩是运行态视图变换（不改日志、前端无感）；model-visible-means-logged / logged-means-persisted 是 loop 铁律。
5. **守卫规则 3**：核心插件（llm/loop/tools）禁依赖功能插件（auth/compactor）；功能插件互不依赖。新增插件先登记进 crate_boundaries.rs。

## 五、待办/候选（2026-08-20 终版）

### 已清空（fc4e797 起）
- ✅ api.rs 2751→**1102 行**：七领域全拆（auth/credentials/llm/workspace/host/settings/session），session 域 (542 行) 最后拆完；model 三 helper 作共享件留主文件
- ✅ workspace lints：`[workspace.lints]` `unused=deny` 全仓强制（10 crate 继承），clippy 零自有警告
- ✅ kernel-supervisor：**保留隔离态**（已脱离 BoenMind 依赖图；kernel submodule 只读不夺，M3 再接）
- ✅ cargo-deny：**不引入**（自研 + 依赖面小，license/bans 低风险）
- ✅ api.rs 测试区：**不拆**（测试贴 AppState/dispatch 是合理的，拆了徒增风险）
- ✅ sharedConfig 漏传：**已过时**（M3 已用 llm.providers + settings.update 实现 provider 表单）
- ✅ headless --compact：**不加**（会破坏 verify-gate1.sh 的位置参数签名）

### 剩余（需产品决策/功能演进，非 repair）
- **compaction 设置表单**：现在是启动参数语义（`--compact` + config.toml `[compaction]`），设置页无对应段。要做 = 升级为运行时可调（后端新增 settings ns + loop 每回合读），属**功能演进**，需产品确认。
- **unwrap/expect 全面清理**（unwrap_used/expect_used lints 留 allow 待做）：测试与合法断言大量使用，清理是独立任务（TODO: unwrap-polish）。

## 六、经典陷阱（踩过的坑，别重踩）

1. **api.rs 手工按行号切块极易错位**。拆分固定套路：先 `grep -n` 精确定函数边界 → 用 sed 按函数名提取到子模块文件（不是裸行号区间）→ 函数名 sed 加 `pub(super)` → api.rs 删除原块 → 补 `mod xx; use xx::*;`。删除前**先确认函数完整结束**（尤其最后一个函数，容易缺闭合 `}`）。本轮 host 拆分就因行号错位缺了 rel_from_workdir 的尾部 body。
2. **子模块里 `use super::xxx` 才能访问主文件的私有项**；主文件 `mod xx; use xx::*;` 只能导入子模块的 `pub(super)`/`pub` 项。函数要跨模块可见就标 `pub(super)`（不是 private）。
3. **`Arc<具体类型>` 不能隐式给 `Arc<dyn Trait>` 字段**——需要显式 `as Arc<dyn ...>` 或在 let 处标注类型（assembly loop_runtime 里就是这么干的）。
4. **trait object 需要 async_trait 宏**（async fn 直接进 trait 不能做 dyn）；实现端也要 `#[async_trait]` 宏。
5. **`pub use host::host_workdir` re-export 要求该项本身是 `pub`**（不能是 pub(super)）。跨模块导出用 `pub`。
6. **修改 Cargo.toml 后 Cargo.lock 会变**（如去掉 supervisor 依赖），提交时一起带上。
7. **验证闭环**：`cargo build --workspace`（0 error）→ `cargo test --workspace`（查 FAILED）→ `cargo clippy --workspace --all-targets`（零警告，除 rquickjs future-incompat 提示）→ `bash scripts/verify-gate1.sh`（GATE1: ALL PASS）。**每次提交前过一遍**。

## 七、验证命令速查

```bash
cargo build --workspace
cargo test --workspace            # 含 crate_boundaries 守卫；当前 29 组全绿
cargo clippy --workspace --all-targets
bash scripts/verify-gate1.sh      # headless 全链路 + kill-9 恢复
# 端到端（web-server + --compact 消费 [compaction] 配置）：
./target/debug/web-server.exe --db <tmp.db> --config ~/.boenmind/config.toml --compact --port 3099
```

## 八、环境/备忘

- 工作区根 `D:\96_CoderWorld\BoenMind`，branch `main`，remote `origin`（BoenMind.git）/`dsh-origin`（boenmind-dsh.git 另仓，勿混推）。
- `kernel/` 是 git submodule（heads/main 在 95ab2659），内容只读不改。
- 前端联调走 3080 生产服务（`frontend/dist`），开发用 vite dev（代理 web-server 3080）。
- 记忆库见 `C:\Users\Boen\.zcode\cli\memories\projects\boenmind-...\memory/`（project-architecture-fix.md 汇总多轮架构决策）。
- Grok 独立评审全文存档在 `.review/grok/grok-review.md`（已 gitignore，复盘可读）。