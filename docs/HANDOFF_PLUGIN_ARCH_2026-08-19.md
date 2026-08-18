# HANDOFF：插件架构探索 + QuickJS 路线实测通过（2026-08-19）

> 状态：**插件"真形态"探索收敛 + QuickJS 实测通过**。用户核心诉求 = Rust 核心（高效省内存）+
> 插件免重编译、配置驱动、运行时加载/卸载。历经编译期装配（回退）→ cdylib（不可行）→
> 进程化设计（存档，密度被质疑）→ Bun 跑 dsh（不兼容）→ **QuickJS 路线实测验证：体积 1.5MB exe、
> 内存 4.2MB RSS（Node 的 1/7）——路线成立**。下一步：设计"Rust 核心 + QuickJS 插件桥"边界
> （三核心插件是否 JS 化，一次定死不中途重构）。

---

## 1. 一句话交接

用户核心诉求 = **Rust 核心（高效省内存）+ 插件免重编译、配置驱动、运行时加载/卸载**
（"装个插件 core 要编译、boenMind 也要重新编译，那还叫插件吗"）。历经编译期装配（回退）→
cdylib（grok 判不可行）→ 插件进程化设计（已存档，但一插件一进程被质疑）→ Bun 跑 dsh（实测
不兼容）→ **收敛到 QuickJS 插件桥路线**（Rust 核心内嵌 QuickJS 引擎跑 JS 插件）。当前：
**QuickJS 体积/开销待实测，通过则重新评估该路线，不通过则回到进程化设计。**

---

## 2. 当前三仓状态（稳定基线，全绿）

| 仓 | 提交 | 内容 | 验证 |
|---|---|---|---|
| **BoenMind** | `51c0e7a` | 主仓：前端快照（官方 38 bundle，皮肤已删）+ docs + 工具链；kernel 是 submodule | 起服实测正常 |
| **dsh-rust-core** | `cd19cf6` | 内核 7 crate：contracts/session/storage/supervisor/assembly/headless/web-server；kernel-assembly **依赖 plugins 仓**（装配层）；契约含 AgentPort（kernel-session/agent.rs） | 56 测试 + clippy 全绿 |
| **dsh-rust-plugins** | `a906f8c` | 插件 3 crate：plugin-llm/loop/tools；依赖 core 契约 | 52 测试 + clippy 全绿 |

- 三仓均无未提交改动；跨仓 path 依赖要求两仓并行存在于 `D:/96_CoderWorld/`。
- BoenMind kernel submodule 指向 cd19cf6。

---

## 3. 完整探索历程（2026-08-19，按时间）

### 3.1 出发点：grok 三仓评估
- grok 指出"assembly 依赖插件仓 = 核心依赖插件"违规，给 P0 建议（契约收回 contracts、装配移到最外层）。
- 我据此过度设计（SessionPort/ToolRegistryPort/LoopRuntime 契约化 + from_parts 8 参数），
  **用户喊停回退**（教训：物理隔离=仓库边界已够；assembly 依赖插件是正当组合根装配；
  外部评估批评先判断适用性）。记忆：`avoid-over-abstraction-revert-2026-08-19`。

### 3.2 用户定调插件"真形态"
- "每个插件加入 BoenMind 只要做好配置，不用每次编译"；"插件运行时加载/卸载"；
  "装个插件 core 编译、boenMind 重新编译，那还叫插件吗"；提到微服务。
- 要求"一步到位雏形符合不重编译、可慢慢完善、不中途重构"。

### 3.3 cdylib(dll) 方案 → grok 判不可行
- 用户提"插件编成 dll"。grok 评估：Rust **无稳定跨 dll trait ABI**（vtable/async/panic 均 unstable），
  `extern "C"` 只稳定 C ABI；async trait 无法直通；要上 C ABI 插件 SDK + async 桥，工作量大。
  报告：`.tmp/grok-cdylib-review.md`。**结论：dll 不可行，进程化或内嵌运行时才可行。**

### 3.4 插件进程化架构设计 → 已存档
- grok 出完整设计：三核心插件全进程化（独立 exe）+ plugins.yaml 配置驱动 + MCP 风格 IPC +
  分阶段 A/B/C 接口冻结。**已存档** `docs/design/PLUGIN_PROCESS_ARCH_2026-08-19.md`。
- 用户质疑"10 插件 10 进程？100 插件 100 进程？是 Rust 限制？JS 就没这问题？"

### 3.5 官方 dsh 插件模型实锤（源码级）
- 官方 dsh **所有插件同进程**（`ctx.plugin()` 注册，Cordis DI），非一插件一进程。
- 唯一进程边界 = 用户代码沙箱（sandbox-local worker、tool-pwsh shell），不是插件框架。
- 官方"免重编译"靠 **JS 语言运行时动态加载**（require/import 任意 .js），不是进程。

### 3.6 Bun 跑 dsh → 实测失败
- 装 Bun 1.3.14，实测 `bun dsh.../bin.js web`：**启动即挂**——
  `Export named 'stripTypeScriptTypes' not found in module 'node:module'`
  （dsh 的 `dsh-code-runtime-worker-thread` 用 Node 22.6+ 实验 API，Bun 未实现）。
- **结论：官方 dsh 当前不能换 Bun 跑**（需 patch 该插件或等 Bun 实现）。

### 3.7 QuickJS 路线提出 + 体积传言澄清
- 用户忆"QuickJS 插件桥 6G"。实测 npm：`quickjs-emscripten` 2.4MB、全部 wasm 变体 ~10MB。
  **6G 系误判**（当时算了仓库/构建缓存）。
- 论证：QuickJS = Fabrice Bellard 的独立嵌入式 JS 引擎（纯 C，几 MB）；Rust 核心 + QuickJS =
  "Rust 当引擎高效省内存 + JS 插件动态加载免重编译"；内存账 = Rust 核心近零开销 + QuickJS 几 MB
  vs Node/V8 100~300MB 固定成本。

---

## 4. 已验证事实（可信结论）

1. **dll 动态加载 Rust trait 不可行**（无稳定 ABI + async 无法直通）——grok 源码级论证。
2. **官方 dsh 插件 = 同进程 JS 模块**（Cordis DI），非进程化；官方免重编译靠 JS 运行时动态加载。
3. **Bun（Rust 内核 JS 运行时）跑官方 dsh 当前不可行**（stripTypeScriptTypes API 缺口，实测）。
4. **QuickJS npm 包体积几 MB 级，非 6G**（npm registry 实测）。
5. **Rust 无 JS 式"免费"动态加载**——要动态插件，路径只有：内嵌脚本运行时（QuickJS/Lua）
   或进程边界（IPC）。

---

## 5. 下一步待办（高优先）

### 5.1 QuickJS 实测验证（✅ 已做，2026-08-19 真机数据）

**测试方法**：`rquickjs`（Rust 绑定）最小程序，内嵌 QuickJS 跑斐波那契循环（fib(20)×5），
release 构建（opt-level=3 + strip + lto）。脚本/项目在 `.tmp/quickjs-bench/`。

| 指标 | QuickJS（Rust 内嵌） | Node.js（对照） | 结论 |
|---|---|---|---|
| **进程体积** | release exe **1.5MB**（含 QuickJS 引擎+Rust 运行时） | node.exe 几十 MB / bun.exe 94MB | **体积实锤小**，6G 传言彻底证伪 |
| **执行耗时** | fib(20)×5 = **5.1ms** | 未测耗时（内存对照为主） | 毫秒级，脚本插件场景足够 |
| **RSS 内存** | **4.2MB**（WorkingSet，含引擎） | **32.1MB**（WorkingSet） | **内存省 7.6 倍** |

**判定**：✅ QuickJS **体积小（1.5MB exe）+ 运行开销小（4.2MB RSS，Node 的 1/7）**——
"Rust 核心 + QuickJS 插件桥"路线**成立**，重新评估值得做（见 5.2）。

### 5.2 若 QuickJS 路线通过（✅ 已成立）
- 设计"Rust 核心 + QuickJS 插件桥"：三核心插件（llm/loop/tools）改为 JS 插件？还是保留 Rust
  核心三件 + 业务插件走 QuickJS？——边界一次定死（吸取"不中途重构"教训）。
- 内存/性能分层：重逻辑留 Rust、轻胶水走 JS；插件主要做配置/编排/调核心 API。

### 5.3 若 QuickJS 路线不通过（❌ 未触发）
- 回到 `PLUGIN_PROCESS_ARCH_2026-08-19.md` 进程化设计，但**进程密度按需分组**（非 1 插件 1 进程，
  回应"100 插件 100 进程"质疑——核心 1 进程 + 业务插件按类分组）。

---

## 6. 环境纪律（沿用）

- 每轮先杀 web-server：`taskkill //F //IM web-server.exe`；Node 后端杀 `node.exe`；测试后杀 `bun.exe`。
- 跑服务用 release（debug exe 2GB 超 PE 限制）；QuickJS 测试项目同样 release。
- 跨仓 path 依赖：core 与 plugins 并行存在于 D:/96_CoderWorld/。
- 验证三件套：cargo test/clippy/gate1（在 kernel/ 子模块内）。
- 视觉验证用 `.tmp/vision-check.mjs`（MiniMax-M3 识图）；Bun 已装（~/.bun/bin）。
