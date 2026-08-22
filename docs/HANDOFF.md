# BoenMind 工作交接（2026-08-22 · 审计修复轮 + 万物皆插件①②）

> 新会话从这里接手。本文件是**项目级**交接（架构主线）；前端线交接见
> `docs/FRONTEND-HANDOFF.md`；全量审计台账（44 项：✔已修/⏳挂账/📝记录）见
> 仓库根 `QUESTIONS.md`——**那是唯一权威清单，本文件不重复其内容**。

## 1. 当前状态（一切已提交已推送，工作区干净）

**刚完成的大事（本轮全部落地）：**

1. **全仓审计 + 修复轮**：44 项发现，30 项已修（含三个 Critical：scheduler retain
   反转、goal 排他门泄漏、前端审批帧丢弃；安全 High：export/WS 补 auth 门控、
   api_key 迁后端 credentials）。提交 `061f6c3`（后端）+ 前端修复随 `e5c20ed`。
2. **用户拍板产品方向：「万物皆插件」走回归路线**（判定记录：QUESTIONS.md
   ARCH-T1——此前项目实际形态是"万物皆端口 + 静态组装"，能力后端住在
   web-server，插件退化为工具注册器）。
3. **路线①已落地**（`0a6caa4`）：四个插件删进程级全局静态，源构造注入进
   handler；Runtime 实例隔离；assembly 测试 ~1/6 概率 flaky 根治，串行锁删除。
4. **路线②已落地**（三批：`ebfbe43`/`d804af8`/`2b2b49a`）：约 1600 行能力后端
   从 web-server 下沉进插件（详见 §3）。web-server 瘦回纯协议层。

**验证基线**：`cargo test --workspace` 218 全绿；`tsc -b` / `npm run build` 全绿。

## 2. 下一步任务（按优先级）

### 首选：路线③ AppState 剩余状态收编
AppState 还有 15 个字段，其中领域状态五块该随能力走：`workspaces` +
`archived_session_ids`（工作区）、`projections`（投影）、`attachments`、
`settings/credentials`（已持久化但归 web-server 管）。模式照抄②：状态搬进
插件、宿主经 bm-ports 端口委托、assembly 构造。完成后 AppState 只剩
sessions 目录 + 广播通道 + 端口句柄，B-QUAL-001/002 结案。

### 路线④ PluginRuntimePort 真装卸（远期）
kernel-supervisor 孤岛crate 接线（K-ARCH-001），需 kernel submodule 上游配合。

### 其他挂账（QUESTIONS.md ⏳ 项，按需取用）
- **kernel 上游仓**（dsh-rust-core submodule）：Session::append seq 竞态
  （K-BUG-001）、同步 SQLite 阻塞 tokio（K-PERF-001）、supervisor 测试硬编码
  Windows cmd——改 kernel 需在 submodule 里提交。
- **前端下一阶段**：文件/技能/插件面板接真后端（现在是 SEED 假数据，
  后端 host.listWorkdir/readFile/writeFile/skill.list 都在位）、lastSeq 增量
  协议、context 拆分（F-PERF-001）。
- **性能专项**：session.search FTS（B-PERF-001，当前无调用方不急）。

## 3. 架构现状（②搬家后的格局，接手人必读）

```
kernel/（submodule，纯内核）← bm/ports（产品契约层）← plugins/*（能力实现）
                                    ↑ 宿主能力端口（web-server 实现，插件消费）
                                    · SessionDrivePort  会话目录/原子占用/spawn 回合
                                    · BroadcastPort     host/mux 广播 + 投影写入
                                    ↓ 领域端口（插件实现，web-server wire 面委托）
                                    · GoalEnginePort    goal 完整状态机（含续跑）
                                    · ApprovalFacePort  respond 路由/重放/测试钩子
                                    · SchedulePort      定时任务（引擎+工具同源）
bm/assembly = 唯一组合根（install_* 系列构造一切；L0 禁依赖 plugin-*）
bm/web-server = 纯协议层（RPC 翻译 + host_face.rs 端口实现）
plugin-approval = 新 crate（审批中心：pending 表+等待表+respond 全在里面）
```

**关键机制**：
- 工具 handler 构造注入源（`register_all(registry, src)`）；install_* 是
  「先注销本组再注册」的替换语义（ToolRegistry::register 重名报错，不能覆盖）。
- goal 双面语义**故意保留差异**：工具面 resume 有额度检查 / wire 面相位直置
  无；wire 建目标缺省 1 轮 / 工具面 8 轮（调用方显式传参）。改前先读
  bm/ports/src/goal.rs 的 trait 文档。
- 审批中心**无条件装配**（respond/重放/测试钩子不依赖 `--approval`）；
  `--approval` 只门控 loop 消费面接线（`connect_approval_loop`）。
- goal 引擎同样无条件装配（wire 面 goal.* 常开）；`--goal` 门控工具注册 +
  续跑启用（`set_driver_enabled`）。
- 功能插件 manifest 进 `plugin_manifest()`：经 assembly 的
  `push_core_plugin_manifest`（防重复）；core_plugins 是 RwLock（&self 可装）。

## 4. 本轮踩坑记录（新会话别再踩）

1. **`cargo test ... | grep` 管道掩盖真实 exit code**——判定必须看
   `${PIPESTATUS[0]}`，否则编译失败会被当绿。
2. **`if let` scrutinee 里的 `Mutex.lock()` guard 活到块尾**——块内再锁同表
   = 自死锁，测试表现为无输出挂起。先 `let x = map.lock()...cloned();` 取快照
   再进 if（plugin-approval respond 踩过，已修）。
3. **Windows 仓库多 CRLF 文件**——node 脚本做多行字符串替换必须按文件实际
   行尾（`\r\n`）匹配，否则**静默不命中**（crate_boundaries 登记踩过三处漏两处）。
4. bash 双引号里写 `node -e "...`...`..."`——反引号被 shell 当命令替换吃掉，
   中文注释里的 markdown 反引号也会中招。复杂改写用临时 .js 文件执行后删。
5. assembly host-tools 测试 flaky **已根治**（①构造注入），若再出现间歇失败
   先怀疑新代码，不要归因到老问题。
6. 前端 IAB 自动化点击常失灵，用 curl/node 脚本验证后端行为更可靠（历史经验）。

## 5. 验证命令（交付门）

```bash
cargo check --workspace --all-targets   # 看 PIPESTATUS
cargo test --workspace                  # 当前 218 全绿
cd frontend && npx tsc -b && npm run build
# 稳定性抽查：cargo test -p bm-assembly --lib 连跑 3 次
```

## 6. 本轮提交索引（git log 可查详情）

```
2b2b49a refactor(万物皆插件②c): Approval 中心下沉新 crate plugin-approval
d804af8 refactor(万物皆插件②b): Goal 引擎下沉 plugin-goal
ebfbe43 refactor(万物皆插件②a): Scheduler 下沉 plugin-schedule + 宿主能力端口铺设
6fd87b6 docs(审计): 补录专题——「万物皆插件」语义漂移判定（ARCH-T1）
0a6caa4 refactor(万物皆插件①): 插件源构造注入，删 4 处进程级全局静态
f82d0d5 chore(工程卫生): README 对齐纯 Web 形态 + ignore 补齐
061f6c3 fix(审计): 后端 Critical/High 修复 + QUESTIONS.md 台账
e5c20ed feat(前端重写): v3 纯 Web 前端 + 前后端打通 + session.delete
```

签名：现场实现（审计轮 + 万物皆插件①②）
