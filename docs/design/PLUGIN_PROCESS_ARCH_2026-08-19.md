# 插件 = 独立进程、配置驱动、Host 不重编译 — 架构设计（一步到位雏形）

> 来源：Grok 架构评估（2026-08-19），基于三仓结构 + 已拍板"插件 = Rust 独立进程 + 外置"。
> 状态：**设计基线待用户细看后决定开工范围**。配套评估：`docs/review-dsh-rust-core-2026-08-18/grok-arch-review-3repo-2026-08-19.md`（三仓评估）、`.tmp/grok-cdylib-review.md`（cdylib 评估）。

基于已核实三仓结构、已拍板「插件 = Rust 独立进程 + 外置」以及当前进程内 trait 装配耦合。不引入未给定的实现细节。

---

## 一、总体架构（分层图）

原则：**core 只编译契约与运行时骨架；能力发现只来自配置 + 进程握手；业务二进制永远不进 host 的 Cargo 图。**

```
┌─────────────────────────────────────────────────────────────────┐
│ BoenMind（主仓：前端快照 / 文档 / 工具链 / 发布清单）              │
│  不链接任何插件 crate；只分发 exe + yaml + 版本清单                 │
└─────────────────────────────────────────────────────────────────┘
                              │ 发布物：host.exe + plugin-*.exe + plugins.yaml
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ HOST 进程（web-server / headless 二选一入口）                      │
│  ┌─────────────┐  ┌──────────────┐  ┌─────────────────────────┐ │
│  │ 协议兼容层   │  │ PluginHost   │  │ Supervisor 客户端/嵌入   │ │
│  │ HTTP/SSE    │──│ 配置→拉起    │──│ 拉起/健康/重启/蓝绿      │ │
│  └─────────────┘  │ IPC 路由     │  └─────────────────────────┘ │
│                   │ 热插拔编排   │                               │
│                   └──────────────┘                               │
│  依赖：仅 dsh-rust-core 七 crate（无 plugins path）               │
└─────────────────────────────────────────────────────────────────┘
         │ stdio/pipe 或 named pipe（Windows）+ 可选 UDS（后阶段）
         │ 每插件一子进程；热路径可复用长连接
         ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌─────────────┐
│ plugin-llm   │ │ plugin-loop  │ │ plugin-tools │ │ 业务插件…   │
│ 独立 exe     │ │ 独立 exe     │ │ 独立 exe     │ │ team/steward│
│ LlmPort 适配 │ │ Agent 适配   │ │ Tool 适配    │ │ /memory     │
└──────────────┘ └──────────────┘ └──────────────┘ └─────────────┘
         │                    状态一律外置
         ▼
┌─────────────────────────────────────────────────────────────────┐
│ dsh-rust-core（库，不「认识」任何插件实现）                         │
│  kernel-contracts：Port / 事件 / 能力描述（CapabilityDescriptor） │
│  kernel-session：事件日志 + AgentPort 抽象                        │
│  kernel-storage：会话/工件外置                                    │
│  kernel-supervisor：进程生命周期雏形（M3 已拍板）                   │
│  kernel-assembly：组合根只装配「Port 的 IPC 代理」，不装配具体实现 │
└─────────────────────────────────────────────────────────────────┘
         ▲
         │ 配置与注册表（文件，非编译期）
┌────────┴────────────────────────────────────────────────────────┐
│ plugins.yaml + 能力注册表（运行时，内存+落盘镜像）                 │
│  发现：读配置 → spawn → Hello/Describe → 登记 capabilities        │
└─────────────────────────────────────────────────────────────────┘
```

**分层职责（一次定死，后续只加实现不改边界）**

| 层 | 产物 | 知道什么 | 不知道什么 |
|---|---|---|---|
| contracts | 库 | Port 形状、能力 ID、IPC 消息 schema | 谁实现 |
| assembly | 库 | 把「已登记的 IPC 代理」塞进 Runtime | exe 路径、厂商 |
| host | 可执行文件 | 读配置、交给 supervisor、路由 RPC/流 | 插件内部算法 |
| 插件进程 | 独立 exe | 自己的 Port 实现 + 适配器 | host 业务、其他插件源码 |
| 配置/注册表 | yaml + 运行时表 | id、exe、env、cap 声明 | Rust crate 名 |

**加载/卸载主路径**

1. Host 启动：读 `plugins.yaml` → 对 `enabled: true` 的项请求 supervisor **拉起**。
2. 子进程启动后走 **握手**：`Hello`（协议版本）→ `Describe`（capabilities）→ 写入注册表。
3. `kernel-assembly` 按 **角色槽**（`llm` / `loop` / `tools` / `ext:<id>`）绑定 **IPC 代理**（实现原 trait，内部走进程）。
4. 运行时卸载：停新请求 → 排空流 → `Shutdown` → supervisor 杀进程 → 注册表删项；**不重编译、不重链。**
5. 加载新插件：配置加一项 + 放置 exe → `reload` 或 API → 同上 1–3。

Windows 上进程边界用 **匿名 pipe / named pipe**；stdio 作为雏形默认（与 MCP 生态一致）。

---

## 二、IPC 选型对比与推荐

| 维度 | ① 自有二进制（bincode + stdio/pipe） | ② JSON-RPC over stdio | ③ MCP（官方有 dsh-mcp-client） |
|---|---|---|---|
| 与「零知识发现」 | 需自研 Describe | 可自研 methods | **能力列表是协议一等公民** |
| 流式（LLM SSE/增量） | 自定义 framed stream，延迟最低 | JSON 分片，解析贵 | 官方有 streaming/progress 一类通道（以当时 MCP 版本为准） |
| 调试/跨语言 | 差 | 好 | 好，生态插件可非 Rust |
| 闭源分发 | 好 | 好 | 好 |
| 与现有三插件 | 要写一整套编解码 | 中等 | 适配一层 client；host 侧可复用官方 client 思路 |
| 协议稳定成本 | 全自负 | 中 | 跟官方 MCP 版本；需钉版本 |
| Windows | pipe 自管 | 同左 | 同左 |

**流式过进程边界（三种都适用的形状，一次定死）**

- **控制面**：请求/响应 RPC（非热路径）：`complete` 元数据、工具列表、加载卸载。
- **数据面**：同一连接上的 **单向 chunk 流**（或第二条 pipe）：`StreamOpen(id)` → `Delta(id, bytes|token)` → `StreamEnd(id, stop_reason)`。Host 的 web-server **只做 chunk 转发**，不在 host 内拼完整 LLM 响应。
- **背压**：窗口或 `Ack`；插件阻塞写 pipe 即自然背压。
- **延迟**：本机 pipe 通常亚毫秒～数毫秒/帧，相对网络 LLM **可忽略**；JSON 每 token 序列化会放大 CPU，二进制或「批量 token 一帧」更稳。
- **吞吐**：热路径避免每 token 一次 JSON-RPC 完整对象；用 **framed 流** 或 **MCP 的 stream 原语**（若钉的版本提供）。

**推荐（一步到位、避免中途换协议）**

- **控制面：MCP 风格的 JSON-RPC + `tools/list` 式 Describe**（可直接走官方 MCP 子集，便于以后接非 Rust 插件与 dsh-mcp-client）。
- **数据面（LLM 热路径）：并列 `stream` 通道**（同一进程、第二条 fd 或 MCP 流），帧格式 **长度前缀 + 极简二进制或 newline JSON 二选一，雏形用 newline JSON，schema 冻结为 `Delta` 三字段**。
- **不选纯自有 bincode 作为对外唯一协议**：调试与跨语言会逼你第二套协议，属于隐性重构。
- **对内**：contracts 只暴露 **Rust trait + 流式 iterator/channel**；IPC 是适配器，换传输不换 Port。

协议版本字段放在 `Hello`，不兼容则拒绝拉起——这是稳定面，不是后期补丁。

---

## 三、核心三插件是否进程化

**推荐：三个核心插件与业务插件同一进程模型（全部独立 exe）。角色槽仍在，但实现一律在进程外。**

理由（对齐「一步到位、不中途重构」）：

1. **当前耦合就是 assembly/headless/web-server path 依赖 plugins。** 若 llm/loop/tools 留在进程内，host 仍要链这三个 crate，用户「加插件不编译」对核心路径不成立，且会形成「双轨装配」（trait object vs IPC），半年后必并轨重构。
2. **流式开销**：热路径是 **LLM 网络 I/O**，本机 IPC 不是主因。用「控制面 RPC + 数据面 framed 流」可把额外开销压到可忽略。loop 是编排、tools 是偶发 RPC，更不敏感。
3. **崩溃隔离与蓝绿**：llm 适配器挂了只重启 llm 进程；若进程内，整 host SSE 全死。与已拍板 supervisor 模型一致。
4. **闭源/分发**：三仓已分离；进程化后 plugins 仓只出 exe，core 永不 path 依赖。
5. **性能逃生口（不重构）**：允许配置 `transport: inproc` **仅限开发**（同一套 Port 代理接口，后面接 in-process stub）。生产配置只允许 `process`。接口是 Port，不是「有的插件是 crate 有的是进程」。

**不推荐**「核心进程内 + 业务进程外」：边界会按「谁算核心」漂移，配置模型分裂，assembly 永远删不掉 plugins 依赖。

角色槽（配置里写死，避免能力乱抢）：

- 恰好一个 `role: llm`、一个 `role: loop`、一个 `role: tools`（tools 可再挂子工具进程，见下）。
- 业务插件只有 `role: extension`，经 tools 或 session 事件交互，不替换三槽。

---

## 四、配置驱动注册表设计

**加插件 = 放 exe + 配置加一项（或 drop-in 文件）。卸 = `enabled: false` 或删项 + 热卸载。Host/core 零 crate 知识。**

### 4.1 配置样例

`D:/…/config/plugins.yaml`（主清单）：

```yaml
apiVersion: boenmind.plugins/v1
protocol: mcp-subset/1
host:
  reloadWatch: true          # 文件变更触发调和
  ipc:
    control: stdio           # 雏形
    stream: stdio-2          # 第二 fd；或 named-pipe
supervisor:
  restart: on-failure
  backoffMs: [200, 1000, 5000]
  healthIntervalMs: 3000

slots:
  llm:   plugin-llm-openai
  loop:  plugin-loop-react
  tools: plugin-tools-core

plugins:
  - id: plugin-llm-openai
    role: llm
    exe: ./plugins/plugin-llm.exe
    args: []
    env:
      OPENAI_BASE_URL: ${OPENAI_BASE_URL}
    enabled: true
    version: "1.0.0"
    replace: blue-green      # 或 restart-in-place

  - id: plugin-loop-react
    role: loop
    exe: ./plugins/plugin-loop.exe
    enabled: true
    version: "1.0.0"

  - id: plugin-tools-core
    role: tools
    exe: ./plugins/plugin-tools.exe
    enabled: true
    version: "1.0.0"

  - id: plugin-memory
    role: extension
    exe: ./plugins/plugin-memory.exe
    enabled: true
    capabilitiesHint: [memory.upsert, memory.search]  # 可选，以 Describe 为准
```

可选：`config/plugins.d/*.yaml` 合并，便于「丢一个文件即安装」。

### 4.2 运行时注册表（host 内存 + 可选落盘镜像）

```text
PluginRecord {
  id, role, pid, protocolRev,
  capabilities: [{ name, kind: rpc|stream, schemaRef }],
  health: up|degraded|down,
  generation: u64          // 蓝绿代数
}
```

**core 零知识发现**：不以 yaml 的 hint 为权威。权威是进程 `Describe` 返回的 capability 列表；yaml 只负责 **找得到 exe、填得上槽**。contracts 定义 capability 名的稳定集合（如 `llm.complete_stream`、`agent.step`、`tools.call`）；未知名对 core **忽略或挂到 extension 总线**，不编译、不写 match 死链。

### 4.3 热加载 / 卸载（进程生命周期）

| 操作 | 行为 |
|---|---|
| 启用 | spawn → Hello/Describe → generation++ → 切槽指针到新代理 |
| 禁用/卸载 | 槽切到「空实现或排队失败」→ 排空 stream → Shutdown(grace) → kill → 删 Record |
| 替换（同 id 新 exe） | 蓝绿：拉起 generation+1 → 健康通过 → 切流 → 停旧进程 |
| 崩溃 | supervisor 重启同 generation 或 +1；session 状态在 storage，插件无本地权威状态 |

卸载不是删 dll，是 **停进程 + 改注册表**。

---

## 五、与 supervisor 的关系

已拍板：supervisor 负责拉起 / 健康检查 / 崩溃重启 / 蓝绿（Linux 长期运行；Windows 开发机同等语义、实现用 Job Object 约束子进程）。

**纳入方式**

```
plugins.yaml  ──调和循环──►  Supervisor API
                    │            │
                    │            ├─ start(spec) → pid
                    │            ├─ health(pid) → 探活（进程活着 + 插件 Health RPC）
                    │            ├─ restart(id)
                    │            └─ replace(id, newSpec)  // 蓝绿
                    ▼
              PluginHost 只拿 pid + IPC stdio handles
```

- **Host 不直接 `Command::spawn` 散落各处**；唯一出口是 supervisor（雏形可把 supervisor 链进同一 host 进程，但 **API 先独立**，避免以后拆微服务再撕装配）。
- **健康**：OS 级存活 + 插件 `Health`；连续失败则重启，超过预算则槽位置 `degraded`，web-server 返回明确错误，不拖死整个 Runtime。
- **蓝绿**：新进程 Describe 成功且 `role` 匹配才切槽；切槽是原子替换代理指针（generation）。
- **与「微服务」**：进程模型已是本地微服务；以后把 pipe 换成 localhost TCP **不改** yaml 的 id/role/capability，只改 `ipc.control`。

状态外置：插件崩溃重启后从 kernel-storage / session 日志恢复；禁止插件把权威会话态只放内存。

---

## 六、分阶段落地路径（接口先冻，实现后填）

**冻结面（A 日即锁定，B/C 不得改语义）**

- `plugins.yaml` 的 `apiVersion` / `id` / `role` / `exe` / `enabled`
- `Hello` / `Describe` / `Health` / `Shutdown`
- 三槽绑定规则
- Port trait 与 `Delta` 流形状
- Supervisor 的 start/health/restart/replace

### 阶段 A — 雏形一步到位（最小闭环，已满足「不重编译」）

- 切断 core → plugins 的 Cargo path；plugins 只编三个 **exe**。
- Host 读 yaml，经 supervisor 雏形 spawn，stdio JSON-RPC 控制面 + newline JSON 数据面。
- assembly 只装配三个 **IPC 代理**。
- 手工改 yaml + 发 `reload`：能换 llm exe 而不编 host。
- headless 门禁用「配置里的 exe」跑通一条流式补全。
- 开发可用 `transport: inproc` 测逻辑，**不进发布配置**。

### 阶段 B — 完善（加厚，不改 A 的消息与配置字段）

- 热加载监听 `plugins.d`
- 蓝绿 replace、backoff、Windows Job Object
- 数据面可选第二 pipe / 二进制帧（**新增** `ipc.stream: framed-v1`，旧值仍可用）
- 业务 extension 进程；tools 进程做 tool 路由
- 能力 schema 校验、版本协商失败的人读错误

### 阶段 C — 生产与生态（仍不重构）

- MCP 全子集或对接 dsh-mcp-client；非 Rust 插件
- `ipc.control: tcp` 真微服务部署
- 签名校验 exe、发布清单与 BoenMind 工具链对齐
- 指标：IPC 排队、重启次数、切槽耗时

**禁止的「做到一半再重构」**：A 阶段若保留 assembly 对 plugin-llm 的 path 依赖，或配置只服务 extension，则 B 必然双轨合并——这是明确排除项。

---

## 七、一句话结论

**Host 只编译契约与「配置 → supervisor → IPC 代理 → 三槽」；一切能力来自独立 exe 的 Describe，加插件只加 yaml 和二进制，热路径用独立流通道，从而从第一天就消灭 path 依赖与中途换模型的必要。**