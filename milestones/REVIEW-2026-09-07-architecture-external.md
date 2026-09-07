# BoenMind 架构回头看评审报告（2026-09-07）

> 评审方式：6 路并行深度代码审查（bm-core 核心 / bm-core 支撑 / bm-persist+contract / bm-providers / surface-http+server+cli / webapp 前端 / 插件+apps+合同+CI），关键发现逐条交叉验证（含 2 条驳回误报）。
> 范围：`runtime/`（9 crate + webapp）、`plugins/`（2 个官方 MCP 插件）、`apps/`（3 个 Python MCP server）、`boenmind-contracts/`、`.github/workflows/`、`shell/tauri/`。
> 基线：BoenMind-CORE-ARCHITECTURE.md（§1-§24）、ADR-0001..0025、AGENTS.md 硬纪律。
> 结论：**架构骨架优秀、纪律意识强，但存在 2 个 P0 级正确性隐患、1 个 P0 级安全面缺口、一批"补丁成熟化"欠账**。以下按 P0/P1/P2 分级，全部带 file:line 证据。

---

## 一、总体评价

BoenMind 是我见过的个人项目中架构纪律最接近工业级的：单写者核心循环、事件日志先行 + 位点 CAS 单调、合同冻结 + 同步测试、Broker 裁决 / Registry 注册 / Watchdog 只产事实的职责切分、MCP 子进程 env 白名单 + kill_on_drop + respawn 熔断、前端 bus/storage/api 统一收口——这些是"设计沉淀"而非"补丁堆叠"。

但演进方式暴露了代价：**大量"回看/留档/应急修复"注释 + 机械拆分（内容零改动）+ 双默认值并存**，说明很多修复是"注释追认"而非"设计收敛"。真正的技术债集中在三处：

1. **安全默认值未收敛**（trust 缺省 trusted、Grant 谓词子集匹配、/admin 免鉴权、system_exec 环境继承）；
2. **工具轮无轮数上限 + settle panic 可崩进程**（核心循环的 P0）；
3. **重复面广**（openai/glm 六成重复、三个 Python server 九成重复、两个 Rust 插件自写同一套 JSON-RPC 外壳、三处 constant_time_eq、三处 tail 读取）。

---

## 二、P0（必须修复）

### P0-1 工具轮无轮数上限，存在无限循环烧钱风险
- `runtime/crates/bm-core/src/runtime/turn/spawn.rs:306` 声明 `tool_rounds`，`:416` 自增，但**从未有边界检查**；`:699` 注释声称「循环失控由 MAX_TOOL_ROUNDS 熔断」，而 `MAX_TOOL_ROUNDS` 根本不存在。
- 实际唯一防线是 `loop_breaker` 只检测「同工具同参」连续重复（默认 5 次），模型每次变换参数/换工具即可无限轮转。
- **修复**：补 `tool_rounds >= MAX_TOOL_ROUNDS`（如 30）即熔断，与注释对齐。

### P0-2 `Operation::settle` 用 `panic!` 处理非法迁移，可被外部输入打崩进程
- `runtime/crates/bm-core/src/state.rs:103-105`：`transitions().find(...).unwrap_or_else(|| panic!("表外迁移…"))`。
- 调用面 `runtime.rs:494` 对来自用户/Wire 的裁决直接调 `settle`，任一组合踩空即打崩整个 Runtime 进程（而非返回错误）。
- **修复**：`settle` 返回 `Result`，非法迁移收敛为 `CoreError::Internal` 错误回传。

### P0-3 `system_exec` / `jobs` 子进程继承父进程全部环境（安全面）
- `runtime/crates/bm-providers/src/system_exec.rs:151-165` 与 `jobs.rs:100-135`：`platform_shell` 直接 `Command::new` spawn，**无 `env_clear` 白名单**。
- 对照：`mcp.rs:462-467` 已按「P0(第四轮评审):子进程默认继承父进程全部环境 = 主密钥/令牌外泄 (INV-5)」修复，白名单仅放行 PATH/SystemRoot/TEMP/HOME 等 11 项。
- 若 `BOEN_SECRET_MASTER_KEY`/`BOEN_MODEL_API_KEY` 在环境中，任何获批命令可将其整体外泄；主密钥 + `secrets.enc` 即完整身份绕过。与代码库自封的 INV-5 纪律直接矛盾。
- **修复**：`system_exec`/`jobs` 的 spawn 复用 `mcp.rs` 的 `child_inherited_env()` 白名单。

### P0-4 持久层手工 BEGIN/COMMIT 无事务守卫 + 错误吞掉
- `runtime/crates/bm-persist/src/materialize.rs:22,221-225`：手工 `execute_batch("BEGIN")`/`("COMMIT")`，无 RAII 守卫；`COMMIT` 失败直接 `?` 上抛，残留未提交事务与悬置锁；`ROLLBACK` 失败被 `let _` 吞掉。
- `runtime/crates/bm-persist/src/sqlite_state/memory.rs:63,70-74` 与 `mod.rs:231`：`correction_of` 墓碑、FTS5 插入、FTS5 缺失静默跳过全部 `let _` 吞错——检索质量/数据正确性降级无人知晓。
- `memory.rs:99-106` 级联墓碑（`source_ref`）误伤同批插入兄弟；`memory_put` 纠正墓碑与 FTS 不在同一事务，DER 与 LIKE 兜底面数据面不一致。
- **修复**：引入 `rusqlite::Transaction` RAII 守卫；memory 写路径并入单事务；吞错改为结构化日志 + 计数。

### P0-5 崩溃后误杀半部日志（恢复边界自相矛盾）
- `runtime/crates/bm-persist/src/store.rs:51-57`：`PersistStore::open` 判 `applied > log_last` 即 `Corrupt` 拒开。
- 但崩溃窗口是「日志落盘已 fsync、状态未及物化」，此时 `applied < log_last` 是常态、`repair_tail` 正是为此存在；而 `log_last < applied` 仅可能由「状态侧先写」造成。`recover()` 的修复语义（修位点后重放）只在 `applied < log_last` 时才有意义，而 open 只拒 `applied > log_last`——**正常崩溃窗口实为 `applied == log_last` 或 `applied < log_last`，此处对「>」拒开、对「<」的修复却依赖 repair_tail 在 open 之后被显式调用**，恢复边界在启动路径上自相矛盾。
- **修复**：open 时对 `applied < log_last` 自动触发 repair_tail，或把「拒开」降级为「标记需修复」。

---

## 三、P1（应尽快修复）

### 3.1 安全与鉴权

| # | 问题 | 证据 |
|---|---|---|
| P1-1 | **Bearer 令牌形同虚设**：中间件只包 `/rpc/{method}`、`/events/{session_id}`、`/shutdown`，`/v1/*`、`/admin/*`、静态回落全部只靠最外层门户墙；门户墙在「未配置密码」时按 `!public_bind \|\| exempt` 放行——默认回环单机安装下整个 Web 数据面「全裸」 | `lib.rs:100-139`、`auth.rs:11-29`、`portal.rs:268-272` |
| P1-2 | **`/admin/approvals/{id}/respond` 免鉴权**（代码自认「W1 同款已登记欠账」），等于把「批准执行任意命令」的权限对任何能访问壳子的浏览器开放；前端 `runtime.tsx:487` 裁决失败还静默吞错、审批单从抽屉消失无回滚 | `webadmin/approvals.rs:12`、`runtime.tsx:227-238,487` |
| P1-3 | **门户口令接口无 CSRF/无创建限速**：`/api/portal/bootstrap`、`/api/portal/password` 是状态变更却无 CSRF token；未配置密码时任意站点可浏览器侧抢注密码封死用户 | `portal.rs:315-353` |
| P1-4 | **门户会话永不过期**：`authed` 只查 `HashSet::contains`，无服务端剔除路径；Cookie Max-Age 30 天只是客户端约定，泄露的 Cookie 无限期有效 | `portal.rs:248-250,441` |
| P1-5 | **Grant 谓词子集匹配**：`resource_matches` 只检查已列出的键是否相等，不检查 Grant 未列的额外参数——scope 含 `{path:"a"}` 的 Grant 可放行 `{path:"a","rm":true}` | `broker/predicate.rs:5-13` |
| P1-6 | **Grant 预扣不退还**：`prepare()` 决策后 `grants.consume()` 预扣，后续 `issue_credential/verify_credential` 任一失败，`Once`/`Count` Grant 被静默消耗 | `broker/mod.rs:246-247`、`types.rs:137` |
| P1-7 | **memory trust 缺省 trusted**：`args["source_trust"].unwrap_or("trusted")` 与模块「无特权通道/来源链」声明相悖，安全默认应为 untrusted | `memory.rs:76-79` |
| P1-8 | **MCP 远程传输残缺**：`HttpMcpTransport` 未覆写 `cancel_by_token`（走默认空实现）、`subscribe_progress` 恒为 dead rx；`"streamable-http"`/`"sse"` 一律映射到纯 POST+JSON，真正的 SSE/会话管理未实现——用户选 streamable-http 会静默失去服务端推送与取消能力 | `mcp.rs:137,163,727-783` |
| P1-9 | **MCP 服务器被"读失败即清空"**：`read_mcp_servers` 对 `read_to_string` 的任意 `Err` 返回 `Ok(vec![])`，瞬时 IO 故障会静默卸载全部 MCP 能力 | `supervisor.rs:38-47` |
| P1-10 | **skill_wasm 路径未沙箱**：`skill_root.join(&sc.path)` 直接拼接入参路径，`../` 可越出技能目录读取任意文件作为 wasm 编译 | `skill_wasm.rs:64-66` |

### 3.2 正确性

| # | 问题 | 证据 |
|---|---|---|
| P1-11 | **流式 SSE 超时伪造完成**：硬顶超时时仍发 `finish_reason:stop` + `[DONE]`，客户端误认为回合已完成；应发 `finish_reason:null` + 错误码 | `openai_compat.rs:360-361,420-427` |
| P1-12 | **`context_search` 整文件读入内存无钳制**：`read_to_string` 对 `context-log.jsonl` 无上限整体载入，长会话可达 GB 级；`logs.rs` 至少做了 512KB 回读钳制，此处不一致 | `context.rs:47` |
| P1-13 | **会话消息分页限额借用错误**：`sessions/{id}/messages` 条数上限复用了 `context_search_max_limit`（检索条数），语义错位 | `context.rs:105-109` |
| P1-14 | **`v1_sessions` 只增不删**：会话寻址表长期运行内存增长无上限 | `openai_compat.rs:147-149,226-230` |
| P1-15 | **`config_store` 无锁 RMW**：`set()`/`delete_field` 无锁读-改-写，`atomic_write` 固定 `{path}.tmp`，两并发写互踩同一 tmp → 丢失更新 | `config_store.rs:215-245`、`util.rs:17,21` |
| P1-16 | **`fs_download` 整文件读内存**：下载上限默认可能 256MB，一次大文件下载内存同时存在 bytes+zip buffer | `fs.rs:256-264` |
| P1-17 | **`stop` 排空无超时 + 排空后 autorun 可当场再 spawn 新回合**：`while !w.in_flight.is_empty()` 死等，`autorun_pump` 无 `draining` 检查，自主循环不停产出回合则 `Stop` 永不退出 | `handlers.rs:950`、`autorun.rs:161` |
| P1-18 | **取消竞态**：`Cancelled` 可覆盖 `Completed`，两条发送路径之间无「回合边界已落定」互斥 | `spawn.rs:808`、`handlers.rs:918-920` |
| P1-19 | **状态机迁移直接断言**：`events.rs:76-83,186`、`handlers.rs:525` 用 `transition` 直接断言成功，无 `can_transition` 守卫，边界顺序无强制 | `events.rs:76-83` |
| P1-20 | **`openai_http` 流式 tool_call 聚合 `idx.unwrap_or(0)`**：同块内多个 `index` 缺失的 tool_calls 互相覆盖写进 `tc_parts[0]` | `openai_http.rs:595-606` |
| P1-21 | **`skill_wasm` 所谓"10s 看门狗"不是硬杀**：`timeout` 包裹 `spawn_blocking`，超时只放弃 await，阻塞线程继续跑；`run_wasi` 内 `let _ = timeout_ms` 参数实际未用 | `skill_wasm.rs:128-137,185` |
| P1-22 | **`glm_http` 状态映射语义不一致**：非 2xx 一律 `Unavailable`、401/403 不归 `PermissionDenied`、429 不可重试，与 `openai_http` 两套口径，熔断/降级行为分叉 | `glm_http.rs:155-161` vs `openai_http.rs:323-332` |
| P1-23 | **`approval.rs` 过期检查双写路径**：`respond` 内联过期检查直接置 `Expired` 终态，与 `expire_if_due` 的扫描处置职责重叠 | `approval.rs:106-110` vs `task.rs:148` |
| P1-24 | **`budget.rs` `remaining_tokens` 返回 `i64` 无上界钳制**：`max_tokens` 为 u64 时 `as i64` 可能溢出为负 | `budget.rs:52-54` |
| P1-25 | **`sha256_hex(to_string(args).unwrap_or_default())` 吞序列化失败成空串哈希**，父授权哈希对 JSON 键序敏感（未归一） | `approval.rs:74,124-125`、`coordinator.rs:113-116,139-142` |

### 3.3 前端

| # | 问题 | 证据 |
|---|---|---|
| P1-26 | **审批流双写竞态 + 响应竞态**：`approvalHandlerRef` 末尾无条件覆盖，两回合并发时后回合闭包截获前回合迟到标记；`handledApprovalsRef` 只在轮询侧维护，流内到达的批准后仍被轮询重新入抽屉 | `runtime.tsx:135-190,423-429` |
| P1-27 | **SSE 看门狗不区分"空闲等待审批"与"真卡死"**：审批等待期 >60s 会被前端 abort，与轮询通道叠加出现"双通道互相抢" | `runtime.tsx:249-253` |
| P1-28 | **`loadOlder`/切会话回放异步竞态**：在途响应 `setMessages([...older, ...cur])` 会前插到新会话消息之上（无请求代/会话代守卫） | `runtime.tsx:544-560,428-434` |
| P1-29 | **`key={part.text.length}` 作消息 key**：同长度不同文本复用同一 key，编辑/复制状态跨消息串 | `thread.tsx:486` |
| P1-30 | **Composer 三处 `.catch(() => {})` 静默吞错**：模型/角色/工作区下拉空转无差错提示；`fetch("/v1/models")` 绕过统一 `api.req`，401 不跳登录 | `thread.tsx:606-625` |
| P1-31 | **`context.tsx` 8s 自动刷新无并发节流**：慢响应重入、`setSteps` 乱序覆盖；与 thread 2.5s 审批轮询叠加共 4 个轮询源 | `context.tsx:110-113` |
| P1-32 | **渲染期读 localStorage**：`context.tsx:115`、`thread.tsx:198` 渲染期访问可变外部存储（React Compiler 迁移后是禁项） | `context.tsx:115` |
| P1-33 | **`WorkspaceFiles` 路径字符串拼接**：`${root}/${rel}` 在 Windows 盘符 `D:\` 下双反斜杠/混用分隔符，与 `chainOf` 规范化不一致 | `WorkspaceFiles.tsx:140`、`WorkspacePickerDialog.tsx:30-50` |
| P1-34 | **`PluginsPage` 死参数链**：`onConsumedEditTarget` 由 SettingsPage 传入但 `_goPluginWithFilter` 只设 filter 不设 editTarget，永远不触发 | `PluginsPage.tsx:84-100`、`SettingsPage.tsx:32-35` |
| P1-35 | **`storage.ts` `PINS` 键全仓库零使用**（死键）；`surfaces.tsx` 导出 `paper`/`field` 未被引用（死导出） | `storage.ts:8`、`surfaces.tsx` |
| P1-36 | **`LimitsPage` `fmtNum` 两个分支完全相同**（死逻辑）；`resetAll` 传 `{}` 依赖服务端"空 values=默认"隐式约定 | `LimitsPage.tsx:16-18,86-96` |

### 3.4 插件 / Apps / CI

| # | 问题 | 证据 |
|---|---|---|
| P1-37 | **web-multisearch 版本号三处不一致**：`Cargo.toml:3` = 0.2.0，`main.rs:35` `SERVER_VERSION` = 0.3.0；README 与 main.rs 自称「内置 12 家」而实际 13 家（`cascade.rs:631` 测试断言 13） | `Cargo.toml:3`、`main.rs:35,49,96,543`、`cascade.rs:631` |
| P1-38 | **Python apps 零测试、零 CI 门禁**：三个 server 是真实交付物（进 release 打包）却无任何语法/单测/协议冒烟；`ci.yml` 无 Python job | `apps/`、`ci.yml` |
| P1-39 | **Python apps 异常静默吞掉**：`except Exception: continue` 空转；坏 JSON 应回 `-32700` 而非静默 continue；未知工具应回 `-32602` 而非 `-32601` | `music_server.py:211-213`、`wiki_server.py:132-135,152-158` |
| P1-40 | **CI 插件只跑 ubuntu 单平台**：两插件引用 `native-tls`/`#[cfg(target_os="windows")]`，Windows/macOS 特有代码路径未被验证 | `ci.yml:67-87` vs `ci.yml:40-42` |
| P1-41 | **release 手动触发路径产物名错误**：`workflow_dispatch` 时 `GITHUB_REF_NAME=main`，会打包成 `boenmind-main-linux-x86_64` 并以 `main` 为 tag | `release.yml:5,59-61` |
| P1-42 | **validate.py 未真正校验 mcp-server/capability/task schema**：golden-trace 只覆盖 envelope/agent/logs，其余 schema 仅做 JSON 可解析 | `validate.py:26-33,158-221` |
| P1-43 | **validate.py `format: date-time` 断言过严**：硬正则只接受 `Z` 结尾 UTC，`+00:00` 或含时区偏移即被误报 | `validate.py:121-125` |

### 3.5 架构与分层

| # | 问题 | 证据 |
|---|---|---|
| P1-44 | **`mcp.rs` 1399 行上帝模块**：同时承载传输层（stdio/http/inproc 三种 `McpTransport`）、`McpHub`、配置装载+校验、测试替身与四组测试 | `mcp.rs:1-1399` |
| P1-45 | **`openai_http` 与 `glm_http` 重复约六成**：`WireMessage/WireRequest/WireResponse/WireUsage`、`failed()`、deadline→send→cancel-select→choices 解析骨架几乎逐行拷贝 | `openai_http.rs:313-320` vs `glm_http.rs:94-101` |
| P1-46 | **`handle.rs` 1119 行上帝模块**：含启动恢复(270 行)、中断清点(120 行)、butler bootstrap(40 行)，与「运行句柄 M1 方法集合」定位不符；应拆 `handle.rs`(纯样板) + `bootstrap.rs`(恢复/清点) | `handle.rs` |
| P1-47 | **`registry.rs` 职责错位**：`direct_tools`(252-266) 与 `chat_tools`(276-294) 把「对话工具闭环枚举」塞进 Registry，与模块声明「只回答谁提供什么、不持有任何策略」冲突；`direct_tools` 零调用（死代码），Approval 判定逻辑在 registry 与 broker 重复实现 | `registry.rs:14-15,252-294` |
| P1-48 | **`store.rs` 纯转发电梯**：60 个方法大页面，`fn x() { self.state.x() }` 机械转发；「写穿/恢复/快照」与「CRUD 表」应分成两个类型 | `store.rs:419-562` |
| P1-49 | **`boenmind-server.rs` 300+ 行 main 不可单测**：参数解析、limits、token、双开预防、连接器、能力装载、路由表、Router 组装、优雅停机全内联；与 `load_skill_scripts` 的"抽函数可测"风格形成反差 | `boenmind-server.rs:27-402` |
| P1-50 | **`runtime.tsx` 582 行单体**：流式 + 审批双通道 + 会话分页 + 回放全塞一个文件，审批通道是前端最大技术债集中地 | `runtime.tsx` |

---

## 四、P2（改进建议，按性价比排序）

### 4.1 重复代码收敛（三体重复）

| 重复面 | 位置 | 建议 |
|---|---|---|
| `constant_time_eq` 逐字重复 | `auth.rs:32-39`、`portal.rs:226-233` | 抽公共工具 |
| tail 读取+去半行逻辑重复 | `logs.rs:11-31`、`context.rs:160-186` | 抽 `read_tail` 公共函数 |
| CRLF 替换三处写法各不同 | `config_store.rs:135-137`、`providers.rs:54`、`mcp.rs:37` | 统一为公共函数 |
| 默认模型魔法串 `"zhipu.glm-4-flash"` 三处硬编码 | `boenmind-server.rs:151,358`、`bm-cli/src/main.rs:75` | 收敛为常量 |
| 轮询游标样板三处 | `rpc.rs`、`openai_compat.rs:282-333,376-418`、`sse.rs` | 抽 `EventCursor` 迭代器 |
| 三个 Python server 的 stdio 循环 + JSON-RPC 分发九成重复 | `apps/*.py` | 提炼共享 MCP 框架基类 |
| 两个 Rust 插件自写同一套 JSON-RPC 外壳 | `web-multisearch/src/main.rs:107-171`、`context-inspector/src/main.rs:231-262` | 提取公共 crate |
| `ENV_MAP` 与 `apply_legacy_key` 手工两份 key 映射 | `web-multisearch/src/config.rs:14-25`、`cascade.rs:454-470` | 单点约束 |

### 4.2 死代码 / 残留

- `registry.rs:252-266` `direct_tools` 零调用；`broker/types.rs:129` `DispatchedAsync`、`:125` `ProviderUnavailable` 无生产者；`approval.rs:36-37` `TaskScopeUnavailable` 无构造/消费路径；`watchdog.rs:253-255` `task_ref` 零调用；`team.rs:52-53,59,86` `child_verbs` 收集后 `let _` 丢弃。
- `mcp.rs:795-797` `Route.server` 被 `#[allow(dead_code)]` 标注（真死字段）。
- `glm_http.rs` 全程无生产接线（仅 feature 门控的"存在性证明"适配器）。
- `runtime.rs:46-52` `turn_timeout_from_env()` + `DEFAULT_TURN_TIMEOUT_SECS` 死代码（唯一调用方是 demo）；`BOEN_TURN_TIMEOUT_SECS` 在 `limits.rs:618-636`、`bm-judge/src/lib.rs:210` 又各读一份——同 env 三处解析。
- `spawn.rs:68` 注释「`turn_timeout_secs` 保留为兼容字段不再读」但 `spawn.rs:69` 已改读 limits——注释与代码脱节。
- `thread.tsx:463-465` 注释自认「历史双份实现，于 2026-09 审计清理」但函数残留。
- `McpDialog.tsx:1-16` 死导入（空导入块、未用 `DialogDescription`）；`PluginsPage.tsx:13-14` import 块内空行+尾随空行。
- `storage.ts:8` `PINS` 死键；`surfaces.tsx` `paper`/`field` 死导出。
- `persist/src/error.rs`、`rows.rs` 只有一行 re-export 的空壳文件。

### 4.3 打补丁痕迹 / 硬编码

- **日期注释泛滥**：`team.rs:76,163`、`task.rs:161`、`budget.rs:71`、`coordinator.rs:12,23` 等「2026-09-05 回看收紧/修复」实为已合入逻辑的变更记录，应移入 commit message/ADR，不留源码。
- **magic number 散落**：`spawn.rs:605` sleep 400ms、`capability.rs:30` `Count(5)`/`Ttl(3600000)`、`fs_tools/mod.rs:39,60,78,96` 内联 `30_000/10_000`、`builtin.rs:34,164,229` 内联 `1000/2000/120000`、`jobs.rs:160,189` 轮询 200ms/钳 60s、`portal.rs:96` 阈值 1024、`mcp.rs:310` 探针 10s。
- **文案与配置脱节**：`mcp.rs:563-567` respawn 熔断文案写死"60 秒"、`mcp.rs:745` 写死"超时(60s)"，而实际读 `limits.mcp_respawn_window_ms/mcp_remote_timeout_ms`（可热改）。
- **`Arc::into_inner(self).expect("装配期独占")`**：`mcp.rs:421,719`、`portal.rs:48` 被 clone 引用共享时直接 panic，宜返回 `Result`。
- **`expect("锁未中毒")` 高频**：`portal.rs` 7 处、`jobs.rs:122` 等，把 Mutex 中毒直接变 panic。
- **日志风格混用**：`sse.rs:59` 走 `tracing::warn`，而 `boenmind-server.rs:97-98,300,360` 及 webadmin 全用 `eprintln!`/`println!`，两套并存。
- **调试 eprintln 留在生产路径**：`mcp.rs:477,489,494`（"MCP 子进程已拉起 pid=…"）、`guard.rs:54`（无效根告警）。

### 4.4 测试与文档

- **bm-core 是唯一 src 内嵌测试的 crate**：`runtime/tests.rs` 205 行、`broker/tests/` 4 文件，其余 crate 均为独立 `tests/` 目录——风格不统一，建议迁移。
- **`mock_model.rs` 未 `cfg(test)` 门控**：`boenmind-server.rs:145` 生产默认路径直接装配 `MockConnector::repeating`（默认模式静默返回"mock 模型回答"）；`latency_ms: 1873` 魔法默认值却作为"性能定标口径"。`builtin.rs:211-214` 已有 `production_builtin_capability_set()` 把测试桩挡在生产外，mock 模型是唯一外泄点。
- **`tests.rs:53-174` 一个测试跑 5 张表 roundtrip**：任何一张失败只给表名；v2/v3 测试同文件重复建 schema——建库样板可抽 helper。
- **`key_meta_covers_every_serialized_key` 只断言字段名与 meta 键一一对应**，不校验 min/max 区间与 default 一致性，钳制表与默认值易漂移。
- **web-multisearch README 过时**：`README.md:43` 接线示例仍指向 `D:\96_CoderWorld\boenmind-mcp-servers\...` 外部路径（已移入主仓）；README 称「9 家 API Key」与 13 内置不符。
- **`usage.rs:17-68` 自实现日历**（手写 1970 起逐年限推），冗长且易在新月边界出错。
- **`context-inspector` `est_tokens` 估算偏差无档**：`(chars+2)/3` 对 CJK 全按 3 tokens/字，文档未降格为估算。

---

## 五、测试代码与发布源代码分离情况

**总体良好，两处例外**：

1. **`mock_model.rs` 在 src/ 且生产默认路径装配**（P1-20 同款）——测试替身进入发布面，应 `cfg(test)` 门控或移入 bm-testkit。
2. **`bm-core` src 内嵌 `#[cfg(test)]` 模块**（`runtime/tests.rs`、`broker/tests/`）——与其余 crate 的 `tests/` 目录惯例不一致，属风格债而非功能问题。

其余分离良好：`bm-surface-http/tests/` 8 个集成测试文件、`bm-testkit` 独立 crate（含 chaos_kill/机器 fuzz/GT 回放）、`webapp/e2e/` 与 `scripts/*.mjs` 分离、插件 `tests/` 目录独立。**但 Python apps 零测试**（P1-38）是发布面最大的测试缺口。

---

## 六、技术文档梳理建议

1. **`AGENTS.md` 当前状态行已严重超载**：单行塞入 v0.0.10→v0.0.13 全部交付史，新会话读起来是"意识流"。建议只留当前版本 + 指向 `milestones/HISTORY.md`。
2. **`PLAYBOOK.md` §3 浏览器自动化怪癖**已膨胀为"排障回忆录"，与 §1 启动/§4 Rust 测试的"工具书"定位不符；建议把排障叙事移到 `docs/` 独立排障手册。
3. **`docs/` 目录已有 5 份一次性报告**（overnight-LEDGER、limits-inventory、agent-tools-comparison 等），无索引；建议建 `docs/README.md` 索引或按主题归档。
4. **`BoenMind-CORE-ARCHITECTURE.md` 82KB 单文件**：§1-§24 编号被大量引用（硬锚点），重排风险高；建议保持现状，但把"阶段二演进模型"（§2.1 五层运行时）与"阶段一实现"的边界在文中更醒目地标注——当前 §2.1 开头已有说明，但正文多处仍混用 L0-L5 术语。
5. **web-multisearch README 需与实现对齐**（版本号、内置家数、接线路径），见 P1-37/P2-4.4。
6. **ADR 编号跳空**（0012 随 M10 dsh 线归档）已在 AGENTS.md 说明，建议在 `adr/README.md` 补一张"编号→状态"总表，避免新读者误以为缺档。

---

## 七、修复优先级建议（按性价比）

| 批次 | 内容 | 理由 |
|---|---|---|
| **第一批（P0，安全+正确性）** | P0-1 工具轮上限、P0-2 settle 返回 Result、P0-3 system_exec/jobs env 白名单、P0-4 事务守卫+吞错、P0-5 恢复边界 | 全部是"一行到几十行"的修复，收益最高 |
| **第二批（P1 安全）** | P1-1~P1-10 鉴权收敛、Grant 谓词/预扣、memory trust 缺省、MCP 远程传输补全、skill_wasm 沙箱 | 安全默认值收敛为核心不变量 |
| **第三批（P1 正确性）** | P1-11~P1-25 流式超时、context_search 钳制、stop 排空、取消竞态、状态机守卫 | 单写者循环的边界加固 |
| **第四批（架构重构）** | P1-44~P1-50 拆 mcp.rs/handle.rs/runtime.tsx、抽 openai/glm 公共客户端、抽 Python MCP 框架、抽插件 JSON-RPC 公共 crate | 重复面收敛，需配回归 |
| **第五批（卫生）** | P2 全部：死代码清理、日期注释迁移、magic number 收敛、日志统一、测试迁移 | 低风险，可随批次顺手做 |

---

## 八、驳回项（交叉验证后判定为误报）

| 原报告 | 驳回理由 |
|---|---|
| 前端 `console.warn` 是无效 API | **误报**。`console.warn` 是标准 Web API，`runtime.tsx:89` 用法正确（代理把"warn 不存在"的表述绕晕了） |
| `context-inspector` 手动实现 HTTP 认证与 web-multisearch 不一致 | **误报**。两插件都是标准 stdio JSON-RPC 握手，`initialize` 返回 `serverInfo` 是 MCP 规范要求，非缺陷 |

---

*评审范围：commit 3890422（v0.0.13 复盘复核批）为当前 HEAD。本报告为只读审查，未修改任何文件。*

---

## 九、复核处理结果(2026-09-07 主代理交叉复核 + 修复批,基于原报告逐条验证)

> 复核方式:5 路并行只读核查(P1 安全面/正确性/前端/插件CI架构/P2)+ P0 五条主代理亲核;
> 每条以 file:line 实证为准。裁决口径:属实即修、设计取舍留档驳回、误报存档勿重查。
> 总裁决:属实约 45 / 部分属实约 18 / 不实约 7。修复面 = 4 条 P0 + 26 条 P1 + 约 20 条 P2。

### 9.1 已修复(本批,全量回归绿:cargo test --workspace 66 二进制全过 + fmt/clippy -D warnings 三 workspace 绿 + validate.py 全绿 + webapp lint/build 绿 + apps 冒烟绿)

**P0(原 §二)**
- **P0-1** 工具轮无上限:新增 `limits.tool_rounds_max`(默认 64,0=关,设置页可热改)+ spawn.rs 接线(超限收束并告知用户);校准 :159「不设上限」与 :699「MAX_TOOL_ROUNDS」两处互相矛盾的注释(后者引用了不存在的符号)。复核注记:v0.0.10 曾刻意取消 30 轮上限,本修以「宽松安全网 + 0=关」承接,正常链式调用达不到 64。
- **P0-2** settle panic:`Operation::settle` 改返回 `Result<_, IllegalTransition>`;唯一入口 `settle_operation` 对表外迁移记 exec_log + tracing::error 后原样返回(不再打崩进程)。
- **P0-3** exec/jobs 环境继承:`platform_shell`(前台+后台作业共用出口)`env_clear` 后仅回灌剥离 `BOEN_*` 的环境。复核注记:**未采纳** 11 项白名单原修法——exec 是审批闸后的任意命令执行(子进程本就有完整文件系统访问权),白名单严重破坏常规用法且不构成真实边界;剥离内部密钥命名空间(BOEN_SECRET_MASTER_KEY 等)才是对症下药,mcp.rs 白名单(MCP 长驻半信任插件)维持不变。附单元测试。
- **P0-4** 持久层事务:materialize.rs 手工 BEGIN/COMMIT 改 `unchecked_transaction` 守卫(Drop 兜底回滚);memory_put 写入/纠正墓碑/FTS 并入单事务;FTS 吞错与迁移降级改 `tracing::warn` 可观测。「级联墓碑误伤同批兄弟」子项未能实证(source_ref 级联语义即设计),按不成立处理。
- **P0-5 恢复边界:误报驳回**。open=校验(`applied > log_last` 拒开 = T-12「宁可拒开不可双写」,有专门测试)、recover=修复(repair_tail),且启动路径 handle.rs:87 **无条件**调用 recover()——「自相矛盾」「repair_tail 依赖显式调用」与事实不符。

**P1 安全(§3.1)**
- P1-2(前端半边):respondApproval 检查 `res.ok`,失败回滚重新入抽屉+从去重集摘除;YOLO 流内批准失败同样兜底重试。后端 /admin 免鉴权 = W1/W2 规格已登记欠账,本次修复「登记断链」(见 9.3 第 1 条)。
- P1-3:bootstrap 补登录同款 login_gate 限速(按对端 IP);CSRF 子项**部分驳回**——axum Json 提取器强制 Content-Type + Cookie SameSite=Lax 已挡跨站表单/fetch,真实风险面是本机/网络内直连,限速即对症。
- P1-5:尝试「Grant 谓词精确键集匹配」后**被测试否决回退**——审批抽屉刻意只授「抽屉谓词」(t131_132 证实:批准授 scope、调用带全量参数是主路径);精确匹配会打断审批流。残留风险(委派 args_eq 全参快照场景的额外键)登记 BACKLOG 待设计裁决(见 9.3 第 2 条)。
- P1-7:memory.write 缺省 trust "trusted"→"untrusted"(合同枚举含 untrusted,安全默认)。
- P1-9:read_mcp_servers 仅 NotFound=空清单,其他 IO 错误上抛(热重载保持现状不卸载;启动如实报「装载失败已跳过」;webadmin 整表回写不再可能拿空表覆盖丢配置)。
- P1-10:skill_wasm 注册路径 canonicalize + starts_with(技能根) 钉死,`../`/绝对路径/符号链接越界拒绝;附越界拒绝测试。复核注记:WASI 零 preopen,越界只影响「读哪个文件当 wasm」,报告的威胁描述已按此收窄。

**P1 正确性(§3.2)**
- P1-11:SSE 收尾按真实结局分路——正常完成才发 finish stop+[DONE];失败/取消/中断/硬顶超时发 OpenAI 兼容错误帧(`{"error":{...}}`)后断流;前端解析器上屏 `[流式错误: …]`。
- P1-12:context_search 改 BufReader 流式+滑动窗口(最新 limit 条命中,新→旧序不变),不再整文件载入(context-log 无轮转)。
- P1-13:新增 `limits.session_messages_max_limit`(默认 200)独立旋钮,分页页大小不再借用检索上限。
- P1-14:v1_sessions 有界化(容量 1024,插入序逐出最旧;被逐出会话走 session_resume 回源,无用户可见损失)。
- P1-15:config_store set/delete_field 进程级写序化锁;atomic_write 与 filter_lines_atomic 临时名带序号唯一化(防并发写互踩同一 tmp)。
- P1-19:events.rs ×2 + handlers.rs ×1 迁移点加 `can_transition` 边守卫(handle.rs 先例同款),迟到事件记日志不崩进程。
- P1-20:流式 tool_call 聚合缺 index 时按 id 归槽(同块多调用不再全挤 0 号槽拼接成畸形调用);顺带修复非 [DONE] 出口兜底聚合丢工具名。
- P1-22:glm_http 非 2xx 改用 openai_http::map_status(pub(crate) 收口):401/403→PermissionDenied、其余 4xx→ValidationFailed(不烧熔断),429/5xx 才可重试。
- P1-24:remaining_tokens 饱和减法(u64::MAX 无限预算不再 as i64 溢出为 -1);附测试。

**P1 前端(§3.3)**
- P1-26:流内审批标记到达即写入 handledApprovalsRef(轮询不再重复入队);审批裁决 POST 收口单一实现。
- P1-28:会话视图代(sessionEpochRef)——切会话/新建对话抬代,loadOlder/切会话回放/刷新回放三路在途响应代数不符即丢弃。
- P1-29:内容 key 由 `part.text.length` 改常量(流式期间每个 delta 不再整子树重挂载,ThinkingBlock 展开态不被清零)。
- P1-30:Composer 三处下拉加载失败 console.warn;`/v1/models` 401 正向跳登录。
- P1-31:context 8s 刷新加在途守卫(慢响应不再重入乱序覆盖)。
- P1-32:渲染期 localStorage 直读清除——thread 状态栏改 state+事件订阅(新增 `bm-active-model-changed` 事件)、context 的 sid 惰性 useState+事件同步。
- P1-33:WorkspaceFiles absPath 统一正斜杠(Windows 反斜杠 display 与 rel 混拼消除)。
- P1-34:editTarget 死参数链整链拆除(SettingsPage `_goPluginWithFilter` 零调用+editTarget 恒 null)。

**P1 插件/Apps/CI/合同(§3.4)**
- P1-37:web-multisearch Cargo.toml 0.2.0→0.3.0(与 SERVER_VERSION 对齐);main.rs/README「12 家」→「13 家」;README 接线示例外部路径改随包口径;ENV_MAP 补 parallel_api_key(与 apply_legacy_key 对齐)。
- P1-38:新增 `apps/smoke_test.py`(真实 stdio 管道:握手/tools_list/未知工具 isError/未知 method -32601/坏 JSON -32700)+ CI `apps-smoke` job(py_compile+冒烟)。
- P1-39:music_server 未知 method 从「无响应悬挂」改回 -32601;三 app 坏 JSON 静默吞改回 -32700。未知工具 isError=true 为 MCP 规范口径,**不采纳**原报告 -32602 建议(三 app 本就未用 -32601 表达未知工具,报告前提不成立)。
- P1-40:两插件 CI 从单 ubuntu 扩为三平台矩阵(native-tls 与 cfg(windows) 分支入门禁)。
- P1-41:release.yml build-linux 加 `if: startsWith(github.ref, 'refs/tags/v')` 护栏(workflow_dispatch 不再产出 boenmind-main 包)。
- P1-42:validate.py 新增 R1b schema 自检(20 份 schema:关键字子集白名单/类型合法/required⊆properties/pattern 可编译/枚举非空等;注解键 x-*/default/const_note 与文档锚点键豁免)。
- P1-43:date-time 按 RFC3339 接受 `Z` 或 `±HH:MM` 偏移。

**P1 架构(§3.5)与 P2**
- P1-47:`direct_tools` 死代码删除;chat_tools 注释锚定 Broker 步 5(判定权威在 Broker,投影随之)。其余拆分项见 9.3 第 5 条。
- P2 卫生:constant_time_eq 收口 auth.rs 单实现;tail 读取收口 webadmin/tail.rs;CRLF 收口 config_store::crlf(providers/mcp 两处并入);DEFAULT_MODEL_ID 常量(server 两处;cli 不依赖 bm-core 保字面量+双写注);死代码删除(Route.server 字段/watchdog task_ref/team child_verbs 空收集/turn_timeout_from_env,demo 内联读 env);mcp.rs respawn/remote 超时文案随 limits 热值;MCP 子进程生命周期 eprintln→tracing;key_meta 测试补 min/max/default 区间一致性;thread.tsx 残留注释删除;McpDialog 死导入块清理;docs/README.md 索引新建;AGENTS.md 状态行瘦身;context-inspector README est_tokens 降格为估算口径。

### 9.2 驳回项(误报/设计取舍,勿重查)

| 原报告 | 驳回理由 |
|---|---|
| P0-5 恢复边界自相矛盾 | **误报**。open=校验/recover=修复职责分离,handle.rs:87 无条件 recover(),`applied>log_last` 拒开是 T-12 有测试的既定口径 |
| P1-18 取消竞态 Cancelled 覆盖 Completed | **误报**。回合任务经同一有序通道恰发一个终态事件;迁移表无终态出边,假想迟到也只是 P0-2 修的可观测错误,不存在静默覆盖 |
| P1-27 SSE 看门狗误杀审批等待 | **误报**。服务端 10s keepalive 注释行 + 前端任意字节 poke 已防护(正是 v0.0.11 修复),900s 硬顶注释明示 |
| P1-5 谓词子集匹配=漏洞 | **部分驳回**。修法与审批抽屉设计冲突(t131_132 实证),已回退;委派场景残留风险登记 BACKLOG 待裁决 |
| P1-6 Grant 预扣不退还 | 属实但系**留档设计取舍**(模块注释明示「授权=一次执行机会,保守面」,回看复核过) |
| P1-23 respond 内联过期与扫描重叠 | 属实但系**有文档的双保险**(测试 :405-419 明示兜底场景) |
| P1-17 Stop 排空无超时 | **部分驳回**。「autorun 致 Stop 永不退出」不成立(send_input 有 draining 守卫,自主环必终止);排空不取消进行中回合=INV-12 语义,模型调用有 HTTP deadline 收敛 |
| P1-25 哈希键序敏感 | **部分驳回**。serde_json 默认 BTreeMap 序列化即按键名规范序,工作区未启用 preserve_order;改哈希输入反而破坏存量授权链,不动 |
| 原报告 §八 两条(console.warn/context-inspector 认证) | 维持原报告自己的误报判定 |
| P2:mock_model 进生产 | **误报**(上轮 v0.0.13 已驳):零配置缺省装配=首启兜底,配齐 model.json/env 必走真实网关 |
| P2:expect("锁未中毒")/Arc::into_inner expect | 属 BACKLOG P4 既定裁决「非测试 panic 均系不变量断言,维持现状」 |
| P2 各计数偏差 | coordinator.rs 无日期注释/mcp.rs:310 无 10s 探针/thread.tsx 函数已删只剩注释/DispatchedAsync、ProviderUnavailable 有生产者/rpc.rs 无轮询游标/「5 张表」实为 4 组/「四组测试」实为 3 组/portal expect 实为 11 处——均按实证更正 |
| 报告建议 6(adr/README 补编号总表) | **已存在**(adr/README 本就有含 0012 跳空注记的编号→状态总表),无需动作 |

### 9.3 登记 BACKLOG(本评审新增/扩口径,未在本批动工)

1. **/admin 面 Bearer 鉴权**(P1-1/P1-2 后端半边):此前只登记在 W1/W2 里程碑规格(AUDIT 引用),BACKLOG 无条目=跟踪断链;公网部署当前靠门户墙(v0.0.5)+portal 密码,VPS 已设密码。修复需前端令牌接线(admin fetch 全量带 Authorization)。
2. **Grant 谓词精确模式设计裁决**(P1-5 残留):区分「审批抽屉子集授权」与「委派 args_eq 全参快照」,后者是否收紧为精确匹配涉 Grant 字段增发(合同 Minor)。
3. **fs_download 流式化**(P1-16):256MB 上限内整读内存,单份缓冲(bytes 即响应体);改流式 zip 属低优。
4. **skill_wasm 超时硬杀边界**(P1-21):阻塞线程无法硬杀,现有 fuel 2e9 指令硬顶 + tokio timeout 双限,长尾占用小;如需更紧可按 timeout 折算 fuel,涉兼容评估。
5. **P3 大文件拆分扩口径**(挂既有条目):mcp.rs 1399 行(传输三实现+hub+装载)、handle.rs 1119 行(start() 545 行)、runtime.tsx 581 行、openai_http/glm_http 62% 重复抽公共客户端、store.rs 转发电梯;与 broker/turn/task_ops/context.tsx 同批。
6. **日期注释迁移**(P2):bm-core 36 处「2026-09-0x 回看」式变更记录注释迁 commit message,低优批量。
7. **日志风格统一**(P2):boenmind-server/webadmin eprintln/println → tracing,低优。
8. **轮询游标样板收口**(P2):openai_compat 两处+sse.rs 抽 EventCursor,低优。
9. **glm_http 单测**(已有条目扩口径):错误分类已对齐 openai 口径,补 feature 门控单测。
10. **Python apps stdio 框架基类**(挂 BACKLOG「MCP 插件杂项」同族):三 server 分发外壳九成同构,抽共享基类(独立交付物随包,非本批)。

### 9.4 本批交付说明

- 交付人:主代理(单写者);批口径=强耦合合批一轮交付、共享全量回归。
- 遗留见 `milestones/BACKLOG.md`(9.3 各条);交付史登记 `milestones/HISTORY.md`。
- 本文件自 docs/ 移入 milestones/ 存档(惯例:外部评审存档 milestones/)。
