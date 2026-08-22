# Code Review Questions — BoenMind 全仓审计（2026-08-22）

> **状态：Phase 2 已执行完毕（2026-08-22）。** 用户授权「按最优、最合理、最精简自行决策」。
> 决策原则：Critical/High 全修；小而安全的 Medium 顺手修；大重构类挂账（标记 ⏳）。
> ✔ = 已修复并验证；⏳ = 挂账（含理由）；📝 = 记录在案不改。
> 验证：`cargo test --workspace` 全绿（唯一间歇失败为既有 flaky，见 ARCH-001 附注）；
> `tsc -b` / `npm run build` 全绿。

---

## 一、Bug 与潜在问题

### ✔ B-BUG-001: scheduler 定时任务调度循环谓词写反，功能整体失效
**File(s):** `bm/web-server/src/scheduler.rs`
**Severity:** Critical
**Answer:** 已修。取出到期逻辑提取为独立函数 `collect_due`（保留未到期、移出已到期），并新增回归测试 `collect_due_removes_expired_and_keeps_future`。

---

### ✔ F-BUG-001: 前端丢弃 approval/question 帧，触发工具审批时会话永久挂起
**File(s):** `frontend/src/store.tsx`、新增 `frontend/src/components/ApprovalDialog.tsx`
**Severity:** Critical
**Answer:** 已修。mux 流处理 `approval/requested`（按 rpcId 幂等去重，兼容重连重放）与 `approval/resolved`；新增 `ApprovalDialog` 审批卡（工具名/理由/允许一次/拒绝）；应答走 `respondApproval` → POST `/api/respond`（client-response 信封），未送达保留卡片可重试。`question/requested` 后端当前无生产登记点，显式 console.warn 不静默丢弃（UI 留待后端启用时实现）。

---

### ✔ B-BUG-002: goal 自动续跑排他门泄漏
**File(s):** `bm/web-server/src/goal_driver.rs`
**Severity:** High
**Answer:** 已修。无目标/非 active/额度耗尽路径统一走 `admitted=None` → 既有统一释放门；测试补「无目标经过后门必须释放 + 重新种目标可再次续跑」回归。

---

### ✔ B-BUG-003: 宣称 100MiB 上传上限，实际 axum 默认 2MB 先生效
**File(s):** `bm/web-server/src/lib.rs`
**Severity:** Medium
**Answer:** 已修。router 挂 `DefaultBodyLimit::max(host_fs::MAX_UPLOAD_BYTES)`（两个返回分支都挂）。

---

### ✔ B-BUG-004: settings.mutate 的 unset 多段路径语义错误
**File(s):** `bm/web-server/src/api/settings.rs`
**Severity:** Medium
**Answer:** 已修。unset 与 set 对称下钻，中间段缺失/非对象静默结束（幂等）。

---

### ✔ B-BUG-005: WS broadcast Lagged 静默丢事件
**File(s):** `bm/web-server/src/ws.rs`
**Severity:** Medium
**Answer:** 已修。Lagged → warn 日志 + 主动断开（客户端重连走全量基线）；Closed → 退出循环（顺带修掉原 `continue` 在通道关闭时的忙循环）。

---

### ✔ B-BUG-006: goal_create 双锁窗口 panic
**File(s):** `bm/web-server/src/rpc_m3.rs`、`bm/web-server/src/goal.rs`
**Severity:** Medium
**Answer:** 已修（两处同型）。投影先算后插，消除二次取锁窗口。

---

### ✔ B-BUG-007: CLI 参数 argv 越界 panic
**File(s):** `bm/web-server/src/main.rs`
**Severity:** Medium
**Answer:** 已修。统一 `arg_value` 取值助手：缺值友好报错 exit(2)；`--port`/`--max-steps` 解析失败同样友好报错。

---

### ⏳ K-BUG-001: Session::append 的 seq 分配与日志写入竞态
**File(s):** `kernel/kernel-session/src/lib.rs:106-112`
**Severity:** Medium
**Answer:** 挂账。kernel 是 submodule（上游仓 dsh-rust-core），改锁序需上游配合；当前上层每会话单 loop 未触发。列入 kernel 上游待办：锁内分配 seq + 并发测试。

---

### ✔ F-BUG-002: session.prompt 失败无回滚，界面永久残留假消息
**File(s):** `frontend/src/store.tsx`
**Severity:** High
**Answer:** 已修。新增 `send-failed` action：撤掉乐观 user+assistant 占位对，文本/附件退回输入框；create/prompt/网络三层失败都触发。

---

### ✔ F-BUG-003: turn/end 正常完成不重置 streaming
**File(s):** `frontend/src/store.tsx`
**Severity:** Medium
**Answer:** 已修。turn/end 一律复位 streaming。

---

### ✔ F-BUG-004: 切会话不重置 streaming
**File(s):** `frontend/src/store.tsx`
**Severity:** Medium
**Answer:** 已修。select-session 按「目标会话 running 标记」设置 streaming（切回正在生成的会话仍正确显示 live）。

---

### ✔ F-BUG-005: loadHistory 竞态覆盖乐观消息
**File(s):** `frontend/src/store.tsx`
**Severity:** Medium
**Answer:** 已修。按会话的拉取代际（generation）丢弃过期响应。

---

### 📝 F-BUG-006: user/message 防重依赖客户端时钟
**File(s):** `frontend/src/store.tsx:497`
**Severity:** Medium
**Answer:** 挂账。本机部署时钟偏差≈0，10s 内连发同文本的重复/吞消息是边缘情况；根治需后端回显客户端消息 id（协议改动），列入前端下一阶段。

---

### 📝 F-BUG-007: loadedRef 永久跳过 + 双通道全量重拉
**File(s):** `frontend/src/store.tsx`
**Severity:** Medium
**Answer:** 挂账。与 lastSeq 增量协议（F-IMP-001）一起做，属前端下一阶段协议工作。

---

### ✔ F-BUG-008: rpc() 无超时、WS 重连无退避
**File(s):** `frontend/src/lib/api.ts`
**Severity:** Medium
**Answer:** 已修。rpc 加 `AbortSignal.timeout(30s)`（流式内容走 WS 不受影响）；WS 重连指数退避 1s→15s，连上复位。

---

### ✔ F-BUG-009: 模型下拉显示与实际发送不一致
**File(s):** `frontend/src/panels/Composer.tsx`
**Severity:** Medium
**Answer:** 已修。当前选中不在选项里时原样补进选项（显示=发送，无隐藏副作用），不再静默回落第一项。

---

### 📝 F-BUG-010: 所有会话 chunk 无上限累积内存
**File(s):** `frontend/src/store.tsx`
**Severity:** Medium
**Answer:** 挂账。与 F-PERF-001（context 拆分）同批做。

---

### ✔ F-BUG-011: session.cancel 双发
**File(s):** `frontend/src/store.tsx`
**Severity:** Medium
**Answer:** 已修。cancel 收口到 cmd 层 stop 一处。

---

### ✔ F-BUG-012: 菜单点外不关闭 + 流式强制拽底
**File(s):** `frontend/src/panels/SessionPanel.tsx`、`frontend/src/panels/MessageList.tsx`
**Severity:** Medium
**Answer:** 已修。pop-menu 容器 stopPropagation + window click 关闭；滚动改「吸附底部」策略（距底 <40px 才跟随，上翻阅读不被拽回）。

---

### B-BUG-008 ~ B-BUG-013（低危批）
**Severity:** Low
**Answer:** 挂账。atomic_write Windows 覆盖窗口、workspace 排序无持久化、block_on 潜在死锁、create_session 孤儿、goal 额度浪费、cancel 不复位 running——均为边界路径，随对应模块的下轮迭代处理。

---

## 二、安全

### ✔ B-SEC-001: session.export 与两条 WS 流绕过 --auth
**File(s):** `bm/web-server/src/lib.rs`
**Severity:** High
**Answer:** 已修。export 补齐「栅栏 B loopback-pin + auth 门控」（与 download 同款）；WS upgrade 补 auth 门控（HttpOnly cookie 自动携带；API 客户端可用 `?token=` 查询参数，因 WS 无法自定义头）。未启 --auth 时行为不变（本地单用户形态）。

---

### ✔ F-SEC-001: Provider api_key 明文存 localStorage
**File(s):** `frontend/src/lib/storage.ts`、`settings/ModelSection.tsx`、`settings/ProviderFormDialog.tsx`
**Severity:** High
**Answer:** 已修（走后端通道）。保存提供商时 Key 经 `credentials.set {ref:"{KIND}_API_KEY"}` 存后端（后端自动同步进 provider 适配器，值永不出域）；本地与 localStorage 恒存脱敏版（api_key 空，saveSettings 防御性剥离）。编辑时经 `credentials.describe` 显示「已配置，留空保持不变」。已知限制：用已保存 Key 直接「测试连接」不支持（表单里 Key 为空），需重新输入才能测试——记录在案。

---

### ✔ F-SEC-002: 代码高亮输出未经消毒注入 DOM
**File(s):** `frontend/src/lib/markdown.tsx`
**Severity:** Medium
**Answer:** 已修。hljs 输出经 `DOMPurify.sanitize({ALLOWED_TAGS:["span"], ALLOWED_ATTR:["class"]})` 白名单后再注入。

---

### ✔ F-SEC-003: FileEditor 拼接 HTML 字符串
**File(s):** `frontend/src/panels/FileEditor.tsx`
**Severity:** Medium
**Answer:** 已修。改 React 元素渲染（src/alt 自动转义）；顺带删除只剩这一个调用点的 `lib/sanitize.ts`。

---

### ✔ B-SEC-002: 非 loopback 绑定 + 未启 --auth 无警告
**File(s):** `bm/web-server/src/main.rs`
**Severity:** Medium
**Answer:** 已修。启动时显著 warn（保持默认 127.0.0.1 安全缺省不强制，服务器形态需要 0.0.0.0）。

---

### ✔ B-SEC-003: 默认密码明文进启动日志
**File(s):** `bm/web-server/src/main.rs`
**Severity:** Medium
**Answer:** 已修（不再打印密码值）。默认密码本身保留（本地单用户形态的产物；改为随机生成会破坏「开箱即用」，挂账到 --auth 形态的产品决策）。

---

### 📝 B-SEC-004: credentials 明文落盘，Windows 无 ACL
**File(s):** `bm/web-server/src/api.rs`
**Severity:** Medium
**Answer:** 挂账。Unix 已 0600；Windows DPAPI/ACL 是专项工作，先在威胁模型上接受（本机单用户文件系统权限兜底）。

---

### 📝 B-SEC-005: auth.login 无失败锁定 + 30 天 cookie
**Severity:** Medium
**Answer:** 挂账。--auth 形态当前非主路线（前端未做登录流）；启用时一并接 plugin-auth 的 per-IP 限速并缩短 cookie。

---

### ✔ B-SEC-006: mux 重放 pending 审批无门禁
**Answer:** 已随 B-SEC-001 修复（WS auth 门控覆盖重放面）。

---

### ✔ P-SEC-001: web.fetch 重定向绕过 SSRF 防线
**File(s):** `plugins/plugin-web-tools/src/lib.rs`
**Severity:** Medium
**Answer:** 已修。reqwest `redirect::Policy::custom` 逐跳复用 `validate_host_target` 校验，不可信跳转目标直接中止报错。深层 DNS-rebinding TOCTOU（校验与连接两次解析）仍在，挂账到 web-tools 专项（需 resolver 层固定 IP 直连）。

---

### 低危安全批（B-SEC-007~010 / K-SEC / F-SEC-004~006）
**Severity:** Low
**Answer:** 📝 记录在案。host.openPath 黑名单、trust 宽松端口、host.listDirectory 任意绝对路径（设计使然有双护）、JS 插件注入面、kernel 层路径责任外推、bgUrl CSS 拼接（纯样式影响）、dev 代理硬编码端口、无 CSP meta——本地单用户形态下优先级低，随对应模块迭代。

---

## 三、性能

### ⏳ B-PERF-001: session.search 全会话逐个全量扫描
**Severity:** High
**Answer:** 挂账。需要 FTS5 索引/扫描范围限制的设计决策，且当前无调用方（前端未接搜索），先不做。

---

### ⏳ K-PERF-001: 同步 SQLite 阻塞 tokio worker
**Severity:** High
**Answer:** 挂账（kernel submodule 上游）。spawn_blocking 化涉及全部 8 个端口方法与 busy_timeout 语义，需上游仓专项；单用户本地负载下影响有限。

---

### ⏳ F-PERF-001: 单一 Context 全树重渲染
**Severity:** High
**Answer:** 挂账。context 拆分是前端下一阶段的重构项（与 F-BUG-010、F-PERF-002~008 同批）。

---

### ⏳ K-PERF-002~004 / B-PERF-003~010
**Severity:** Medium/Low
**Answer:** 挂账。内存无界、WS 全量基线、fork N+1 写、quickjs 每 runtime、broadcast 容量等——单用户本地形态下非瓶颈，列入性能专项（顺序：K-PERF-001 → F-PERF-001 → 其余）。

---

## 四、架构与结构

### ✔ B-ARCH-001: 进程级全局静态注入（workdir/schedule/goal source）
**File(s):** plugins/plugin-{host-tools,code-runtime,schedule,goal}、bm/assembly/src/lib.rs
**Severity:** Medium
**Answer:** 已修（2026-08-22 万物皆插件①）。四个插件的 `set_*_source` 全局静态删除，`register_all(registry, src)` 把源**构造注入**到每个工具 handler；`Runtime::headless` 构造函数零全局副作用；install_* 改「先注销本组再注册」的替换语义（构造期 NoWorkdir → install 真源覆盖）。**多 Runtime 实例天然隔离，HOST_TOOLS_TEST_SERIAL 串行锁删除**——assembly 测试 6 连跑全绿（改造前 ~1/6 概率间歇失败，干净基线同样复现）。web-server/headless/quickjs-bridge/plugin-loop/plugin-tools/kernel 全部零改动。

---

### ⏳ B-ARCH-002/004: 三种热换装语义不一致（JS 快照/每请求现读/每会话快照）
**Severity:** Medium
**Answer:** 挂账。统一为现读需改 JS 桥的生命周期，随 B-ARCH-001 专项。

---

### ⏳ K-ARCH-001: kernel-supervisor 孤岛 crate
**Severity:** Medium
**Answer:** 挂账。M3 占位、无调用方；其测试硬编码 Windows cmd。上游仓标注 experimental 或移除。

---

### ⏳ K-ARCH-002: contracts 死依赖 + tokio 耦合
**Severity:** Medium
**Answer:** 挂账（上游仓）。删 uuid/async-stream 死依赖是小事，但 submodule 提交需上游流程。

---

### ✔ F-ARCH-001: render 期写模块级可变对象
**File(s):** `frontend/src/store.tsx`
**Severity:** High
**Answer:** 已修（精简版）。改为 `useRef` + effect 提交后写入（`latestRef`），loadHistory 经状态读取器穿透最新值。完整 useSyncExternalStore 化随 F-PERF-001 专项。

---

### ✔ F-ARCH-002: useSendMock 名不副实
**Answer:** 已修。改名 `useChatActions`。

---

### ⏳ F-ARCH-003: 文件/技能/插件/用量面板全是 SEED 假数据
**Severity:** Medium
**Answer:** 挂账（产品工作，非缺陷修复）。后端能力已在位（host.listWorkdir/readFile/writeFile、skill.list、plugin.js.list）；文件面板接真后端是前端下一阶段第一项。

---

### ⏳ F-ARCH-004: 前端无 --auth 登录流
**Severity:** High
**Answer:** 挂账（产品决策）。本地单用户形态下 --auth 非主路线；若启用服务器形态再做登录页。后端 auth 门控面本轮已补齐（B-SEC-001）。

---

### 低危架构批
**Severity:** Low
**Answer:** 📝 记录在案。goal 双路径、as_any 逃生舱（文档化取舍）、双 Cargo.lock、@ alias 闲置等随迭代。

---

## 五、代码质量

### ✔ F-QUAL-001: 前端 RPC 全部静默吞错
**File(s):** `frontend/src/store.tsx`
**Severity:** Medium
**Answer:** 已修。命令层统一 `rpcToast` 出口：信封错误与网络异常都 toast 具体原因。

---

### ⏳ F-QUAL-002: wire 数据裸断言无运行时校验
**Severity:** Medium
**Answer:** 挂账。zod/守卫 + 事件名字面量联合随 F-ARCH-003 阶段做（与后端共享 schema 一起设计）。

---

### ⏳ B-QUAL-001/002: AppState god struct + 内存态重启全丢
**Severity:** Medium
**Answer:** 挂账。拆分与持久化是后端下一轮迭代；「内存态即产品态」若成为产品预期需先做决策。

---

### ✔ 低危质量批（前端部分 + 卫生）
**Answer:** 已顺手清理：MODELS 编造死代码、Unit.tsx 死组件、rpcErr、registerSection 死设计、@types/dompurify 废弃依赖、空目录 `frontend/frontend/`、delete-session 双 case 合并、空态文案语义、错位测试注释（api.rs）。

### 📝 低危质量批（后端/kernel 部分）
**Answer:** 记录在案：unwrap 全 allow、valid_channel 死代码、SSE 备选 handler 未接线（文档化的面 9 备选，保留）、updatedAt 1970 占位（前端 `0||now` 兜底，两端随 F-BUG-007 增量协议一起治）、is_hidden_path/mime 双份、goal 常量双处、kernel load_events 时间戳静默回退等——随各模块迭代。

---

## 六、改进与建议（含测试缺口、文档、打包）

### ✔ H-DOC-001: README 三处失实
**Answer:** 已修。README 重写：React 19 技术栈、Tauri 桌面壳章节删除（含签名密钥段）、HANDOFF 链接改为 FRONTEND- 三件套。

---

### ⏳ H-GIT-001: 大重写整体未提交
**Answer:** 待用户决策。建议分批提交：①前端重写+文档 ②本轮审计修复 ③卫生 ignore 补齐。本轮未代提交（避免替用户拆分其重写内容）。

---

### ✔ H-DOC-002: 安装脚本/release.yml「无登录认证」文案过时
**Answer:** 已修。两处改为「默认未开启，--auth 可开启」。

---

### ✔ B-TEST-001: 高危 bug 对应的测试缺口
**Answer:** 已补：scheduler 调度循环回归（B-BUG-001）、goal_driver 门泄漏时序回归（B-BUG-002）。HTTP 层集成测试（respond/export/WS 门控）挂账到测试专项。

---

### ⏳ K-TEST-001: kernel 测试缺口（rewrite_events 零测试、零并发测试）
**Answer:** 挂账（上游仓）。

---

### ⏳ F-IMP-001: lastSeq 增量协议被浪费
**Answer:** 挂账。随 F-BUG-007 一起做。

---

### ✔ 工程卫生补齐
**Answer:** 已做：.gitignore 补 `gui-test-screenshots/`、`*.tsbuildinfo`；删除仓库根误装的 package-lock.json + node_modules（根目录本无 package.json）。

---

## 七、专题：万物皆插件（2026-08-22 回头看补录）

### ⏳ ARCH-T1: 「万物皆插件」语义漂移——插件降格为工具注册器，能力后端住在程序里
**File(s):** `bm/web-server/src/{scheduler,goal,goal_driver,approval,pending,rpc_m3}.rs`（约 1700 行领域逻辑）；`plugins/plugin-schedule/src/lib.rs`（仅 schemas+register+全局注入，无实现）；对照 `bm/assembly/tests/crate_boundaries.rs`（结构门禁在位）
**Severity:** Medium（方向债，非缺陷）
**Observation:** 结构面守住（依赖只许向下/L0 禁依赖 plugin-*/插件间零依赖，硬门禁）；语义面失守四点：①调度循环、goal 驱动、审批路由等能力后端实现在 web-server（L0），插件只剩工具面且经全局静态注入反向拿实现；②插件是进程级单例不可多实例（ARCH-001 的 flaky 实证）；③领域状态集中在 AppState；④PluginRuntimePort/supervisor 真装卸从未接线（K-ARCH-001）。当前实际形态 = 「万物皆端口 + 组合根静态装配」，唯一动态插件面是 JS 插件（quickjs-bridge）。
**Answer:** ⏳ 方向决策已定（2026-08-22 用户拍板走回归路线）。**第①步已落地**：ARCH-001 构造注入完成（见上），插件恢复为完整可替换部件、Runtime 实例隔离。后续：② 能力后端（scheduler/goal-driver/approval/pending，~1700 行）从 web-server 下沉为插件侧实现 → ③ AppState 领域状态收编 → ④ 远期接 PluginRuntimePort。关联：ARCH-001（✔）、B-ARCH-002/003、B-QUAL-001/002、K-ARCH-001。

---

## 附：本轮修复清单（commit 建议）

后端（7 文件）：scheduler.rs、goal_driver.rs、lib.rs、ws.rs、api/settings.rs、rpc_m3.rs、goal.rs、main.rs、api.rs（注释）、plugin-web-tools/lib.rs
前端（15 文件）：store.tsx、lib/api.ts、lib/markdown.tsx、lib/storage.ts、types.ts、panels/{Composer,MessageList,SessionPanel,FileEditor}.tsx、settings/{registry,ModelSection,ProviderFormDialog}.tsx、layouts/Shell.tsx、components/ApprovalDialog.tsx（新增）；删除 components/Unit.tsx、lib/sanitize.ts
文档/卫生：README.md、.gitignore、packaging/linux/install.sh、.github/workflows/release.yml；删除根 package-lock.json + node_modules
