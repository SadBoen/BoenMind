# BoenMind 交接文档（2026-08-21 批次）

给下一轮对话/协作的接手者。这是「全面回头看 + 连续多轮架构修复 + 前端展示层补齐 + 基础设施升级」之后的仓库状态快照。

## 一、仓库一句话

BoenMind = Rust 微内核（`kernel/` 只读 submodule `dsh-rust-core`）+ 产品层（`bm/`）+ 功能插件（`plugins/`）+ 自研前端（`frontend/`，React 19 + dockview + antd v6 prefixCls=bm）。分层纪律「依赖只许向下」由 `bm/assembly/tests/crate_boundaries.rs` 硬守卫（`cargo test --workspace` 即门禁）。

核心插件（loop/llm/tools）只依赖 kernel-contracts + kernel-session + bm-ports——**不碰任何兄弟插件/功能插件**。这是多轮架构修复的核心成果。

## 二、最近改动（都已推 origin/main）

| commit | 内容 |
|---|---|
| `42321c5` | docs(handoff)：苹果审批标识/设置页策略可视化划入已交付 |
| `3d8171c` | **feat(approval,compaction)：审批会话级豁免 + 压缩策略重置**——ApprovalModal「本会话信任该工具」（(sessionId,toolName) 豁免表 ref 承载，同名调用自动放行）；SettingsPage「重置为默认」（回写出厂默认值）；`_test.registerApproval` 补广播（仅 BM_TEST_HOOKS=1） |
| `5e24ad0` | **feat(goal)：前端目标编辑**——展示态新增「编辑」入口，objective/自动续跑轮次可改（goal.edit，CAS ref 同 pause/resume/complete）；至少改一项才提交，未修改直接退出，取消还原表单；goal.clear 墓碑/切换会话时同步退出编辑态；已过 tsc/vite/cargo/gate1/真实 UI 全链路 |
| `4826f6a` | **feat(ui)：审批弹窗危险标识 + 设置页工具审批策略可视化**——危险工具弹窗附红「危险」徽章；设置页高级「工具审批」子区块（危险/安全 Tag 列表只读展示） |
| `e59c2ba` | docs(handoff)：危险工具白名单划入已交付 |
| `5cbc11c` | **feat(approval)：危险工具白名单**——审批只拦真危险（host.run_command/code.*/web.fetch/goal.create+update/schedule.create），安全工具自动放行；`ToolRegistryPort.requires_approval` + 各插件 `DANGEROUS_TOOL_NAMES` + assembly mark |
| `76cbff9` | docs(handoff)：goal 创建 UI 划入已交付 |
| `8173def` | **feat(goal)：前端新建目标表单**——GoalCard 无目标态转创建态（objective + 自动续跑轮次 1-64 默认 8），创建走 goal.create → 投影回灌切展示态 |
| `5834ad5` | docs(handoff)：compaction 设置表单划入已交付 |
| `84463fd` | **feat(compaction)：上下文压缩升级为运行时可调**——`Runtime.compactor` 换 RwLock + SettingsBackedCompactor（每回合现读 settings）+ 设置页压缩表单 |
| `7be0004` | docs(handoff)：前端展示层两项划入已交付 |
| `bf326b7` | **feat(ui)：前端审批弹窗 + goal 目标卡片**——useMuxEvents 全局帧总线 + ApprovalModal + GoalCard + client.ts auth 修复 |
| `a32fd87` … `1186329` | 四插件交付（code-runtime/审批回灌/web-tools/schedule）+ goal 自动续跑 M3.5 |

## 三、当前架构分层（最终形态）

```
kernel-contracts (纯契约)              ← kernel/ submodule，只读不可改
kernel-session / kernel-storage
kernel-supervisor                     ← 死代码（无引用；M3 才接）
bm/ports                              ← 产品级契约层：Compactor / ToolRegistryPort / ToolGatePort / ToolApprovalPort / WorkdirPort / SchedulePort / GoalPort
plugins/plugin-llm loop tools auth compactor host-tools code-runtime web-tools goal schedule
bm/assembly                           ← 组合根（唯一装配点）
bm/web-server / bm/headless / bm/quickjs-bridge   ← L0
frontend/                             ← React 19 + dockview + antd v6
```

**核心插件只依赖 kernel-contracts + kernel-session + bm-ports**——编译期不碰兄弟插件/功能插件。

## 四、关键设计决策（勿推翻，除非有强理由）

1. **所有产品级策略端口统一放 bm-ports**（Compactor/ToolRegistryPort/ToolGatePort/ToolApprovalPort/WorkdirPort/SchedulePort/GoalPort）。kernel/ 只读塞不进契约。核心插件新增端口一律放这里（只依赖 kernel-contracts + 无实现）。
2. **bm-assembly re-export（MockTurn/DEFAULT_PASSWORD/DefaultCompactor/scripted_llm）是守卫强制的 L0 出口**，L0 只依赖 bm-assembly，装配参数必须经组合根 re-export。**别砍**。
3. **auth 双持久化是有意的 bounded context**（auth.json/sessions.jsonl 自管，不经 kernel-storage）。**不要**迁进 sqlite 事件表。
4. **事件日志 = 唯一事实源**；上下文压缩是运行态视图变换（不改日志）。model-visible-means-logged / logged-means-persisted 是 loop 铁律。
5. **守卫规则 3**：核心插件禁依赖功能插件；功能插件互不依赖。新增插件先登记 crate_boundaries.rs。
6. **危险工具白名单**：审批只拦 `DANGEROUS_TOOL_NAMES` 声明的工具（插件常量 + assembly mark_dangerous），安全工具自动放行。危险度**不塞** kernel submodule 的 ToolSchema（规避改外部仓+升 gitlink）——以插件并行名单 + `ToolRegistryPort.requires_approval` 查询方法承载，模型侧 schema 输出不受影响。
7. **settings-backed 端口模式**（三件套）：L0（web-server）提供 settings-backed 端口实现（内部 Arc<AppState> 现场锁读 settings）→ 经 bm-assembly 组合根装配 → 核心插件零改动。`SettingsWorkdir`（host.workdir）/`SettingsBackedCompactor`（compaction）已验证此模式。**web-server 装配 settings 类策略端口一律走此三件套，守卫规则 3 兼容**。
8. **compaction 语义升级**：config.toml `[compaction]` 段降为**启动种子**（`--compact` 装配时种进 settings.compaction），settings.compaction 记录优先（重启保留）。`enabled=false` 否决权保留。设置页高级分区可调，下一回合生效。

## 五、交付记录（2026-08-21 批次全清单）

### 2026-08-21 四插件交付批（已推 main）
- ✅ **plugin-code-runtime**：code.compile/python/shell，workdir 作用域 + 30s 超时 kill + 输出钱包 512KB/流。`--code-runtime`
- ✅ **工具审批回灌**：bm-ports ToolApprovalPort + plugin-loop 执行前暂停点（Rejected → is_error 回写）+ web-server ApprovalRouter（PendingRegistry + oneshot 等待表 + respond 回拨）。`--approval`
- ✅ **plugin-web-tools**：web.fetch/web.search（SSRF 防线 + 输出钱包）。`--web-tools`
- ✅ **plugin-schedule**：schedule.create/list/cancel；SchedulePort + Scheduler（1s tick 后台驱动）。`--schedule`
- ✅ **goal 自动续跑 M3.5**：plugin-goal（goal.get/create/update）+ GoalRouter + GoalDriver（回合完成点续跑）。`--goal`

### 前端展示层批
- ✅ **useMuxEvents 全局帧总线**（frontend/src/hooks/useMuxEvents.ts）：单例 WS，聚合 approval/requested、approval/resolved、session/projection。**handler 收完整帧（含外层 rpcId）**——审批应答必须回显帧 rpcId（曾因只传 payload 丢 rpcId 导致 respond 恒 bad-response，见坑 10）
- ✅ **审批弹窗 ApprovalModal**：POST /api/respond（allowed-once/rejected），多帧排队、resolved 自动关闭、应答必达
- ✅ **goal 卡片 GoalCard**：快照（session.history projections）+ 增量（session/projection，higher-seq-wins）合并展示；pause/resume/complete 走 goal RPC（CAS ref）；刷新后快照恢复
- ✅ **goal 新建表单**：GoalCard 无目标态「🎯 新建目标」→ objective + 轮次 → goal.create → 投影回灌切展示态
- ✅ **goal 编辑表单**：GoalCard 展示态「编辑」→ objective + 轮次预填 → goal.edit（CAS ref 同 pause/resume/complete，至少改一项才提交；未修改直接退出，取消还原表单）→ 投影回灌切展示态；goal.clear 墓碑/切换会话时同步退出编辑态
- ✅ **client.ts auth 修复**：auth-not-available 直接抛 code（原来被 err.message 吞，未开 --auth 时前端永久卡「载入中…」）

### 基础设施/策略批
- ✅ **compaction 运行时可调**：见决策 8。设置页高级分区表单（启用/水线/尾部比例/下限/中部下限）
- ✅ **危险工具白名单**：见决策 6。默认危险 = host.run_command / code.* 全 / web.fetch / goal.create+update / schedule.create；安全放行 = list_dir/read_file/write_file/goal.get/web.search/schedule.list+cancel
- ✅ **审批弹窗危险徽章 + 设置页工具审批子区块**：前端 DANGEROUS_TOOLS 集合与后端名单对齐，纯展示
- ✅ **审批会话级豁免**：ApprovalModal「本会话信任该工具」→ allowed-once 应答当前调用 + 记入 (sessionId, toolName) 豁免表（ref 承载）；同会话同名工具后续请求自动放行不弹窗；弹窗底部显示「本会话已信任 N 个工具」；纯前端豁免层（后端契约零改动），页面刷新豁免失效（内存态）
- ✅ **compaction 重置为默认**：SettingsPage 压缩表单「重置为默认」→ 回写 settings.compaction 出厂默认值（enabled/watermark/keepRecentRatio/keepRecentFloor/minMiddleTokens）+ 表单同步；`_test.registerApproval` 钩子补 broadcast（仅 BM_TEST_HOOKS=1，豁免全链路验收用）

## 六、验证闭环（每次提交前过一遍）

```bash
cargo build --workspace                        # 0 error
cargo test --workspace -- --test-threads=1     # 串行（已知坑：并行测试竞态）
cargo clippy --workspace --all-targets         # 零警告（除 rquickjs future-incompat）
cd frontend && npx tsc --noEmit && npx vite build
bash scripts/verify-gate1.sh                   # headless 全链路 + kill-9 恢复
```

## 七、剩余待办/候选

### 待办（非 repair，产品演进）
- **unwrap/expect 全面清理**（unwrap_used/expect_used lints 留 allow）：TODO: unwrap-polish。**评估结论（2026-08-21）：建议维持现状**——550+ unwrap 绝大多数是 pthread 锁惯例、必成功构造、测试断言；全量改造噪音大、无功能收益、有回归风险。若做：先启用 lints 再用 `cargo fix` 分批，锁定非测试代码。
- **compaction 深度**：config 段种子已实现「重启保留」；设置页可加「重置为默认」按钮（settings.clear 语义）。✅ 2026-08-21 已交付

### 候选（下轮）
- **审批体验深化**：~~「本会话信任此工具」/「记住上次选择」类记忆~~ ✅ 2026-08-21 已交付（会话级豁免）；可继续做 **持久化豁免**（重启保留）、**豁免管理面**（设置页查看/清除豁免表）
- **goal 编辑**：~~改 objective/额度（现只有 pause/resume/complete，无 edit UI；RPC goal.edit 已存在）~~ ✅ 2026-08-21 前端 GoalCard 已实现（见交付记录）
- **前端新视图/打磨**：设置页无缝体验、文件管理器深度集成、CodingApp 落地（当前占位）
- **settings 页 app文档**：设置项 applies 恒 "restart" 元字段未真正区分；可将 compaction/workdir 类的"下一请求生效"语义显式化

### 已知坑（别重踩）
1. **并行测试竞态**：`WORKDIR_SOURCE`/`SCHEDULE_SOURCE` 全局源跨 test 文件共享，**串行全绿、并行 flaky**。CI 用 `--test-threads=1`。
2. **api.rs 手工按行号切块极易错位**。固定套路：grep -n 精确函数边界 → sed 按函数名提取 → 加 pub(super) → 删原块 → 补 mod/use。删除前确认函数完整闭合。
3. **子模块 `use super::xxx` 访问主文件私有项**；主文件 `mod xx; use xx::*;` 只能导入 pub(super)/pub。跨模块导出用 pub。
4. **`Arc<具体类型>` 不能隐式给 `Arc<dyn Trait>`**——显式 `as Arc<dyn ...>` 或 let 处标注类型。
5. **trait object 需要 async_trait**（async fn 进 trait 不能做 dyn）；实现端同样要宏。
6. **`pub use` re-export 要求目标本身 pub**（非 pub(super)）。
7. **改 Cargo.toml 后 Cargo.lock 会变**，提交一起带。
8. **web-server.exe 被运行进程锁死**——`cargo build` 前先停 3080/3099 进程（`netstat -ano | grep :3080` 拿 PID → taskkill）。
9. **Git Bash 中文提交信息显示乱码是终端假象**——git 存 UTF-8 正确，`git log` 看正常。
10. **审批 rpcId 丢失劫**：approval/requested 帧的应答 key 是**外层信封 rpcId**（非 payload 内 approvalId）。前端总线只传 payload 会丢 rpcId → respond 恒 bad-response（曾定位半天）。useMuxEvent handler 必须收完整帧。

## 八、环境/备忘

- 工作区根 `D:\96_CoderWorld\BoenMind`，branch `main`，remote `origin`（BoenMind.git）/`dsh-origin`（boenmind-dsh.git 另仓，勿混推）。
- `kernel/` git submodule（heads/main 在 95ab2659），只读不改。
- 前端联调：生产 `frontend/dist` 由 web-server 静态服务（默认 3080）；dev 用 vite（5173，代理 3080）。
- 浏览器验证：会话内浏览器标签持续存活；定位器在 React 重渲染下易超时——危险弹窗等交互用坐标点击（evaluate 读 rect → tab.cua.click）最稳。**看界面必须识图**（Minimax/M3 视觉模型），不许只靠 DOM 文字猜。
- 记忆库 `C:\Users\Boen\.zcode\cli\memories\projects\boenmind-...\memory/`：project-frontend-approval-goal / project-compaction-settings / project-dangerous-tool-whitelist / project-handoff 等。
- Grok 独立评审全文存档 `.review/grok/grok-review.md`（已 gitignore）。