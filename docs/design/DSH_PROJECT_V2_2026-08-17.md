# BoenMind 计划 v2：Rust 微内核自研 + 前端借 DSH 生态（2026-08-17）

> 状态：**定稿方向**（微内核一步到位已拍板；§八 拍板点 1–10 全部拍板）
> 修订史：v1（同日夜）＝新建项目、dsh 全家桶为唯一真身、逐单元升级 → **v2（本稿）＝Rust 微内核自研后端 + 前端全套借 dsh 生态 + 插件/APP 全 Rust**。v1 的"全家桶 bootstrap / 毛玻璃皮肤 / DSH_HOME 启动器"成果保留（前端生态基础），后台路线按微内核重排。**v2.1（本稿，同日夜）＝三架构师 SKILL 交叉评审修正**（code-architecture / codebase-reviewer / ln-24，报告见 `docs/review-dsh-v2/`）：契约面 6→9、mux 方向修正、三层事件澄清、插件进程模型拍板、LLM 端口补入、存储模型统一、双栅栏 trust fence。**对外契约逐字对齐为用户拍板铁律（§3.4）。**
> 事实基线：只依据 2026-08-17 核查（npm registry + 本机 dsh 浅克隆 + 官方 docs/source），不引用旧结论。
> 关联：`docs/design/DSH_PROJECT_2026-08-17.md`（v1 全文，已归档为 `docs/archive/`）、`frontend/public/docs/expert-team.md`（专家团队载体）、`backend/vendor/UPSTREAM_TRACKING.md`（上游台账）。

---

## 〇、一句话结论

**后端 = 全 Rust 微内核**（agent loop / session log / 工具注册 / 存储 / MCP 全自研 Rust，核心做小）；**插件与 APP = 全 Rust**（编译产物分发，进程级隔离 + 状态外置，支持 Linux 服务器长期运行时的无感重起）；**前端 = 全套借 dsh 生态**（官方 web-app UI + 皮肤 + ui-slots，不自己打磨前端）；两者之间由 **Rust 实现的 web-server 协议兼容层**接通——照着 dsh 前端的接口合同逐条实现，让 dsh 的 React 界面直接消费我们的 Rust 后端。

---

## 一、用户四条定调（2026-08-17 原话要点 + 推论）

| # | 用户定调 | 推论 |
|---|---|---|
| 1 | 基本自用，开源，也提供给同事；**部分 APP 可能不开源** | 插件/APP 分发 = **编译产物（二进制）**，闭源 APP 不给源码；Rust 插件天然二进制，契合 |
| 2 | **后端全 Rust；前端兼容 dsh 生态**；前端是短板、不愿花时间打磨 | 前端借成熟生态（官方 web-app + 皮肤），不自己造；**工程核心 = Rust web-server 兼容层**（§三） |
| 3 | **所有插件、APP 以后都用 Rust 写** | 插件模型 = cdylib 或独立进程 + 状态外置纪律；弃 TS 插件路线（现有 bm-compat QuickJS 桥退役） |
| 4 | 热插拔意义不大，**但 Linux 服务器长期运行时有意义** | 插件进程化 + supervisor（崩溃自动拉起/蓝绿替换）统一本地与服务器；优先级后置但架构现在留口 |

---

## 二、新项目形态（微内核）

```
boenmind-dsh/
├── kernel/                 # Rust 微内核（核心做小）
│   ├── loop/               #   回合循环（turn/step，waterfall 事件）
│   ├── session/            #   append-only SessionEvent 日志（唯一事实源）
│   ├── tools/              #   工具注册表 + 门控（enabled 名单 + fail-closed）
│   ├── llm/                #   LLM 适配端口（provider trait，门禁 1 前置）【v2.1 补】
│   ├── storage/            #   sqlite 持久化后端（事件日志唯一事实源；sessions/messages 为其投影）
│   ├── mcp/                #   MCP client/server（bm-mcp 语义迁入）
│   └── supervisor/         #   插件进程宿主（拉起/健康检查/崩溃重启/蓝绿切换）
├── plugins/                # Rust 插件（**独立进程**，编译产物，状态外置）【v2.1 拍板】
│   ├── team/               # 专家团队（对齐 expert-team.md）
│   ├── steward/            # 管家（治理区间 + wake）
│   ├── memory/             # 记忆分层/项目隔离
│   ├── audit/              # 审计/工具调用显示
│   ├── browser/            # 浏览器自动化（自研，T4 续）
│   └── skins/              # 皮肤适配（映射到 dsh --dsw-* 令牌）
├── web-server/             # Rust 协议兼容层（§三：dsh 前端接口合同 6 面）
├── frontend/               # dsh web-app 产物 + 皮肤（借来的，build 时打包）
├── shell/                  # Tauri 2 桌面壳（复用现有配置，后置）
├── apps/                   # 我们的 Rust APP（编译产物分发，闭源可选，无授权密钥）
└── docs/
```

- **版本**：v0.1.0 起（已拍板）。
- **历史资产**：现有 BoenMind 仓库只读参考（插件逻辑/专家提示词/记忆语义是搬运素材）；dsh 全家桶 bootstrap 保留作前端生态基础与协议参考实现。

---

## 三、web-server 协议兼容层（本计划最大工程点，专节详解）

### 3.1 为什么要它（大白话）

dsh 的界面（React）只跟 dsh 的 Node 后端说话，两边有一套固定的**接口合同**。我们要"遥控器还用 dsh 的、电视换成自己 Rust 造的"，就必须写一个**信号翻译器**（Rust 版 web-server），照着合同逐条发出 dsh 遥控器认得的信号。合同不满足，界面就白屏或断流。

### 3.2 接口合同（v2.1 修正：9 面 + 双栅栏，2026-08-17 源码级实取）

> v2.1 修正（三架构师评审 + 亲验）：原 6 面 → 9 面；**face2 mux 方向修正**（原稿写反：mux 是宿主→浏览器下行，WS 上行一律拒绝，浏览器上行走 HTTP POST envelope）；补 `/api/respond`、`session.export`、SSE 备选、HMR 通道；boot 协议实为 3 槽。

| # | 面 | 形状 | 谁消费 |
|---|---|---|---|
| 1 | **HTTP POST `/api/<channel>/<endpoint>`** | **RPC 信封**（非 REST）：`client-request` + `rpcId` + `method` + `payload`，回 `server-response`（rpcId 回显校验）。55 个 RPC 方法 + 6 宿主概念（workspace/goals/skills/agentPresets/subagent/jobs） | 前端所有请求上行 |
| 2 | **WS `/api/events.mux`** | **宿主→浏览器 下行**多会话聚合流（MuxFrame 9 种）；浏览器上行消息一律 close(1008 'downlink only') 拒绝 | 前端实时投影 |
| 3 | **WS `/api/events.host`** | 宿主→浏览器 下行宿主事件流（HostFrame 9 种） | 前端宿主状态 |
| 4 | **静态 SPA** | index.html + 资产，SPA 兜底 200 / 非 GET 405 / 越界 403 / 未知扩展 octet-stream | 页面加载 |
| 5 | **`/plugins/<id>/client.js`** | 插件前端 bundle（`__ModuleLoader__` 注册） | 前端插件系统 |
| 6 | **boot 3 槽** | `__DSH_BOOT__`（启动图）+ `__ModuleLoader__` + `__DSH_MODULES__`（模块表），注入 index.html | 前端启动 |
| 7 | **`POST /api/respond`** | 审批/提问应答上行（rpc 之外的特殊面） | 权限弹窗/提问 |
| 8 | **`GET /api/session.export`** | 会话日志 ZIP 下载 | 审计/导出 |
| 9 | **SSE 备选 carrier + `/plugins/events` HMR 通道** | 非 WebSocket 环境的 SSE 流；插件前端 HMR 热更新事件 | 特殊宿主/开发 |

**双栅栏 trust fence（v2.1 补）**：除 Host/Origin 栅栏（`api-request-trust.ts`，只信 loopback + `--trusted-host`）外，另有 **16 个特权方法的 loopback-pin**（`PRIVILEGED_METHODS`：settings.*、credentials.*、agentPreset.read/copy/remove/openDocument、host.pickDirectory/openPath、llm.discoverModels 等）——即使 LAN 部署也强制 loopback。Rust 兼容层两栅栏都要复刻。

### 3.3 为什么可行（不是黑盒）

- **合同是组合的、公开的**：每一面由独立包注册（api-gateway / frontend-static / client-modules / client-connection），源码在 `packages/` 下可逐行读；协议就是"REST + 两条 WebSocket + 静态文件"，形状清晰。
- **我们有参考实现**：本机 dsh 克隆就是活的参照物，逐条对表实现 + 用真实前端验收。
- **前端不用动**：dsh web-app 产物原样打包，皮肤/ui-slots 生态原样可用。

### 3.4 铁律：对外契约逐字对齐（用户提醒，2026-08-17）

**内部实现两种语言有差别，对外的样子必须一模一样。** 兼容层不是"能用就行"的转译器，而是 dsh 前端契约的 Rust 镜像：

- **路径/帧/字段/错误码逐字一致**：`/api/events.mux`、`/api/events.host`、REST 路由、WS 帧、JSON 字段、HTTP 错误码——不自创、不改名、不改形状。
- **事件名/负载形状逐字一致**：`agent/pre-step`、`turn/stopping`、`tool/execute`…事件名与字段与 dsh 源码一致（语义随内部实现走 Rust，对外事件语义不变）。
- **行为细节逐字一致**：SPA 兜底 200、非 GET/HEAD 405、路径越界 403、未知扩展 octet-stream、trust fence（只信 loopback + 显式 trusted-host）——插件依赖的正是这些边角行为。
- **挂点集合一致（按三层重述，v2.1）**：dsh 事件分**三层**，复刻范围不同——
  1. **wire 层**（前端看到的，必须逐字复刻）：MuxFrame 9 种 + HostFrame 9 种 + SessionEvent 信封（wide-data + ignorable 守卫）；持久化 SessionEvent 46 种（core 14 + 插件扩展 32）。
  2. **进程内 cordis 事件**（`agent/pre-step`、`tool/execute`、`turn/stopping`…）：**不上 wire**，前端不直接消费——Rust 宿主内部可自由组织，无需逐字复刻（语义对齐即可）。
  3. **扩展槽**：`session/projection`、`host/remote-event` 是开放扩展点，插件可挂。

**契约台账（产物）**：M2 开工先产出 `CONTRACT_LEDGER_DSH.md`——从 dsh `packages/` 源码逐条提取的路由/事件/负载/语义清单，双用途：实现清单 + 验收标准。
**验收方法**：同一份 dsh 官方前端，分别连 dsh Node 后端与我们的 Rust 兼容层，行为逐项对比；对上一项才勾销一项。前端升级 = 对台账核对。

### 3.5 落地顺序（即使架构一步到位，实现仍分阶段验收）

1. 静态 SPA + `__DSH_BOOT__`（页面能加载，白屏问题最先解决）
2. HTTP `/api/*` 最小子集（会话列表/建会话/发消息/取回复——聊天闭环）
3. WS 下行事件流（消息流式、工具调用实时投影）
4. WS 上行 mux + 完整 API 面
5. `/plugins/*/client.js`（前端插件系统可用，皮肤/第三方 UI 生效）

> **风险**：这是全计划最大不确定工程（dsh 前端内部契约多、文档薄，主要靠源码对表）。**缓解**：与 dsh 官方保持同版本锁死（升级前端 = 同步核对合同）；先做出聊天闭环再谈其他。

---

## 四、后台实现顺序（微内核）

> 里程碑按微内核重排；前端 dsh 生态全程并行（M0 已跑通，作 UI 宿主与协议参考）。

### M0 前端生态基础（已完成）
dsh 全家桶 bootstrap 跑通（3080 服务、__DSH_BOOT__、插件资产 200）+ 毛玻璃皮肤接入 + DSH_HOME 统一启动器（`scripts/dsh.cjs`）。**此后 dsh web 角色 = 前端宿主 + 协议参考实现。**

### M1 微内核骨架（Rust）
- kernel/loop + session + tools + **llm（provider trait，先接 mock LLM）** + storage（sqlite）+ mcp：把 bm 内核四件套语义 + dsh 的 harness 语义（append-only 事件流、turn/step waterfall、model-visible-means-logged）合并为 Rust 微内核；**状态外置纪律**（进程只持可重建状态）。
- 存储模型统一（v2.1）：**事件日志（append-only SessionEvent）= 唯一事实源**；sessions/messages/tool_calls 是它的**投影**；崩溃恢复语义对齐 dsh（fsync + 原子发布 + interrupted-turn 修复）。
- supervisor 雏形：插件进程宿主骨架（拉起/健康检查/崩溃重启）。
- **门禁 1**：headless 回合全链路（消息→工具→回复）在 Rust 微内核上跑通；**mock LLM 下 kill -9 恢复测试**（中断回合可续跑，事件日志无 torn-tail）；crate 边界守卫（依赖只许向下，借鉴 bobleer check-crate-boundaries 思路）。

### M2 Rust web-server 兼容层
- 先产出 **`CONTRACT_LEDGER_DSH.md` 契约台账**（9 面 + 双栅栏 + 55 RPC 方法 + 46 SessionEvent + MuxFrame/HostFrame 9+9 + boot 3 槽，逐条从 dsh `packages/` 源码提取），再按 §3.5 顺序实现；dsh 前端直连 Rust 后端（不再起 Node 后端）。
- **门禁 2**：**conformance harness**（台账机器化：wire 轨迹 diff，同一 dsh 前端连 Node 后端 vs Rust 兼容层，请求/响应逐帧对比一致）——建会话→发消息→流式回复→工具调用可见全通；皮肤可用；台账全部勾销。

### M3 插件/APP 全面 Rust 化 + 微内核红利
- 插件 = **Rust 独立进程**（编译产物分发、闭源可行）；权限两档（官方/自研宿主 + 第三方 worker 降权）；**进程隔离 ≠ 沙箱**（v2.1 澄清：第三方 worker 需额外降权/能力裁剪，防读全盘）；记忆/审计/团队/管家插件逐个移植。
- supervisor 完整（蓝绿替换、崩溃计数、IPC 协议版本化+鉴权）——**Linux 服务器无感重起就绪**；本地形态延续旧版热升级（单二进制替换 / 壳重启子进程，§五·7）。
- **门禁 3**：禁用插件→蓝绿替换→**流式进行中**实测会话不中断；专家团队全链路（派工→并行→结构化返回→汇总）。

### M4 发布
- v0.1.0：浏览器版首发 + Tauri 壳（后置项）+ CI（Rust 质量门：测试/clippy/VMware runner 复用）+ Docker + 便携包（Rust 单二进制，无需内置 Node）。
- **门禁 4**：全量回归 + 便携包真实启动（沿用"先本地实测再发版"铁律）。

---

## 五、功能亮点处理（不变，全部 Rust 插件形态）

1. **专家团队**：`team` 插件（Rust）= 团队配置 + 子代理进程（隔离天然数据隔离）+ 编排（parallel/chain）+ 结构化返回（JSON 契约）；DAG 可视化参照 dsh-task-dag 映射到前端槽位。
2. **管家 Steward**：`steward` 插件 = 治理区间 + wake + 版本化替换进化（supervisor 蓝绿替换天然承载）。
3. **皮肤/特效**：前端借 dsh 生态原样（官方主题 + 毛玻璃已接入）；我们 skins 语义（参数化滑杆）映射 `--dsw-*` 令牌。
4. **工具调用显示/审计**：前端借官方 trajectory + 我们的摘要键逻辑（存于前端插件）。
5. **记忆分层/项目隔离**：`memory` 插件（Rust，storage-domain 语义 + 桶/项目隔离）。
6. **可审计心智**：Rust 微内核原生 append-only SessionEvent（与 dsh 同构），审计 UI 借前端生态。
7. **热升级/便携包（架构要求，2026-08-18 升格）**：Rust 单二进制便携包（无 Node 依赖）+ supervisor 蓝绿替换 = 热升级天然载体。用户要求**在线热升级延续**（旧版 pi 时代已落地：点一下升级、不重装、不退出，见 docs/HANDOFF_HOT_UPDATE.md）。

   **微内核下的诚实评估**：微内核让热升级"从数据安全难题降级为进程编排工程"——核心风险已由既有能力消除，剩余是 supervisor 编排（工程量清晰，非免费）：

   - **与微内核的衔接（为什么可行）**：
     - 状态外置纪律（M1）：进程只持可重建状态，事件日志 = 唯一事实源 → 换进程不丢数据；
     - kill -9 恢复已实测（M1 门禁 1）：中断回合可续跑、无 torn-tail → **热升级 = 主动 kill -9 + 新版接管，恢复路径已验证**；
     - WS 断连重连语义（台账 §4 + M2.5 subscribed 基线已实现）：前端自动重连 → 后端进程切换前端自愈。
   - **三层热升级**（各层独立，互不阻塞）：
     1. **核心 web-server**（Rust 单二进制）：supervisor 蓝绿替换（新进程起 → 健康检查 → 切流 → 旧进程排空）。流式会话中断点最多丢一个 chunk 尾部（chunk 逐块落盘，M1 abort@2 断点已证），新进程从事件日志恢复接续；
     2. **插件**（Rust 独立进程，M3）：单独替换不动核心；IPC 协议版本化 + 鉴权；
     3. **前端**：dsh HMR（/plugins/events）插件 bundle 热替换；壳层升级走刷新。
   - **本地/桌面形态延续旧版**：浏览器版 = 单二进制替换（旧版 standalone 同构，PID 不变）；Tauri 壳（后置）= 壳重启子进程（旧版 managed 同构）；验签沿用 ed25519 minisign 发布资产 + 按版本号落盘天然回滚。
   - **门禁**：M3 门禁 3（禁用插件→蓝绿替换→**流式进行中实测会话不中断**）即热升级验收；**过渡态（M2.5 现状）已可验证**：kill -9 后前端自动重连 + 会话历史恢复（kill -9 恢复 M1 已过、WS 重连 M2.5 已过，两者合流即为过渡态热升级验证）。

---

## 六、风险与对策

| 风险 | 对策 |
|---|---|
| **web-server 兼容层工程量/未知**（dsh 前端契约多、文档薄） | §3.5 分阶段；与官方同版本锁死；源码对表 + conformance harness（wire 轨迹 diff）；聊天闭环优先 |
| 契约漂移（dsh rc 生态） | 锁版本快照；升级=显式核对台账 |
| 微内核复杂度（进程编排） | 从 supervisor 雏形渐进；IPC 协议化+鉴权；状态外置纪律是硬前提 |
| 进程隔离 ≠ 沙箱（第三方插件可读全盘） | 第三方 worker 降权 + 能力裁剪（v2.1 明确） |
| 插件/APP 全 Rust 的构建速度 | sccache/CI runner 复用；插件增量编译；Rust 热替换靠 supervisor 不是进程内动态加载 |
| 热升级"在线不中断"的切流时序（蓝绿编排） | supervisor 蓝绿 + 健康检查 + WS 重连兜底；**数据安全已由状态外置 + kill -9 恢复消除**（剩余风险=切流时序，非数据丢失） |

---

## 七、拍板点（全部已拍，2026-08-17）

| # | 议题 | 决议 |
|---|---|---|
| 1 | 仓库形态 | 新仓库 `boenmind-dsh`（已建，github.com/SadBoen/boenmind-dsh） |
| 2 | 版本号 | v0.1.0 起 |
| 3 | Node 分发 | **不内置 Node**（Rust 单二进制便携包；v1 的 node-runtime 项作废） |
| 4 | 桌面壳 | 浏览器先行，Tauri 2 壳后置 |
| 5 | 插件信任 | Rust 进程隔离两档（官方/自研 + 第三方 worker 降权） |
| 6 | 浏览器自动化 | M3 后排期（v0.2.x） |
| 7 | 历史数据 | 只读归档（现有 turso 不动） |
| 8 | **后端架构** | **Rust 微内核一步到位**（不先全家桶后迁移；v2 定稿） |
| 9 | **前端协议** | **Rust web-server 兼容层直连 dsh 前端**（一步到位，§三） |
| 10 | **插件/APP 语言与分发** | **全 Rust 编译产物**；闭源可选；**不做授权密钥**（基础 APP 出来后再议） |

---

## 附：与既往决策的连续性（语义层）

- "学 dsh 不抄 dsh" → 前端借生态、后端自研微内核（dsh 语义吸收：harness 事件流/组合思路）。
- 万物皆插件/插件自治边界 → Rust 插件进程化 = 完整实现；supervisor 替换 = 功能原子化替换决策权（管家）。
- 微内核讨论（上轮）→ 本稿 M1/M3 即其落地：核心做小、插件进程化、状态外置、supervisor 守护。
- 三护城河（可审计心智/软件形态革命/Steward 治理）→ 全部在 Rust 微内核语义上保留。
