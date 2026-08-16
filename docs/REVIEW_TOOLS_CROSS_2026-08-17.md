# 三工具交叉审查报告 2026-08-17

> 方法：三个独立审查工具各全库审查一次 → 问题交叉校验（聚类去重、共识度标记、关键点实证复核）。
> 独立报告：`docs/review-tools-2026-08-17/TOOL_A_code-architecture.md`（15 条）、`TOOL_B_codebase-reviewer.md`（42 条）、`TOOL_C_ln24-architecture-auditor.md`（Verdict FAIL，材料性门后 1 条 P1）。
> 审查范围：frontend + backend crates/plugins；**硬排除 WIKI 应用**（`bm-wiki`、`/api/wiki`、wiki GUI、`wiki_*` 工具语义）。允许观察组装层被 wiki 加厚，不把 wiki 功能当问题。
> 对照基线：`docs/REVIEW_TOOLS_CROSS_2026-08-16.md`（P0–P2 九项已修；挂点按定调不删）。

## 〇、一句话结论

上轮「接线未完」已变成「接线已完、运行时一致性未完」。三工具一致认为：**骨架与上轮修复没有回潮**（权限门 / CSRF Origin / `run_agent_turn` / MemoryPort 单例 / Compactor dyn / context_window / 双写冻结标注均仍在；`cargo check` 与 `tsc -b` 全绿），本轮主伤是服务面铺开时把 `AppConfig` clone 成第二权威，再加权限门闩启动快照——设置页改密钥/技能/档位可以和热路径各说各话。

## 一、补全后的审查清单（差提示词补全）

| 维度 | 问什么 | 本轮用法 |
|---|---|---|
| 架构合理性 | 分层、依赖方向、所有权、事实源、插件边界是否与可执行代码一致 | A 3A 主线 + C 清单 DoD |
| 精简 | 死代码、双路径、不可达退化、心智货架；挂点按定调只评估不删 | A 复用/精简 + C 材料性门 |
| 优美 | 命名、上帝文件、注释与实现是否同语 | A + B QUAL |
| 复用 | 重复编排、协议重复、已注册面被旁路 | A 强项 |
| 完善 | 热更新是否生效、测试、契约、文档漂移 | B 实证 + A 完善 |
| 安全 | 鉴权、CSRF、路径、SSRF、密钥、权限门、XSS | B 六维 + A/C 交叉 |
| 性能 / 正确性 | 热路径成本、取消语义、配置是否真改到执行面 | B 必跑编译 |

## 二、三工具概况

| 工具 | 工作流 | 发现 | 维度均分（约） | 强项 |
|---|---|---|---|---|
| A code-architecture | 3A 现状→亮点→担忧 | 15 | 架构 7 / 精简 6 / 优美 6 / 复用 7 / 完善 6 / 安全 7 | 所有权与复用（配置双源、fork 日志、面被旁路） |
| B codebase-reviewer | 六维穷举 + 编译实证 | 42（High 7 簇） | 综合 6.5 | 实证（check 全绿）+ 安全/热更新 bug |
| C ln-24 auditor | 44/44 清单 + 材料性门 | 1 条 P1，Verdict **FAIL** | 所有权 FAIL，依赖 PASS | 只报过门的结构缺陷；把 CSRF 任意端口、workspace root 标为已接受例外 |

## 三、共识问题（按共识度）

### ★★★ 三工具共识 — P1 配置/策略所有权分裂（本轮第一优先）

- A-1 Critical、B ARCH-001/BUG-001/QUAL-007 High、C F1 P1
- 现象：`serve_inner` `config.clone()` → kernel `shared_config`（std RwLock）；`AppState.config` 再包一把 tokio RwLock。`PUT /api/config` 只写后者；`SkillPort::set_enabled` 只写前者并落盘；`BuiltinGate`/`McpGate`/`ExtensionPolicy` 用启动档位固化。
- 后果：改 API key 下一轮仍打旧密钥；技能启停 UI 与聊天注入面不一致；yolo→safe 后 bash/subagent 仍直放。
- 修法（三工具建议同形）：Port 与 AppState **共用同一 `Arc<RwLock<AppConfig>>`**；门闩每次 `check` 读当前 `extension_policy`；`put_config` 后 `invalidate_loop_agents`。

### ★★☆ 双工具共识

| 项 | 来源 | 级别 | 处理 |
|---|---|---|---|
| 插件 `pi.http` 无 SSRF / 每次 `Client::new` / 无 body 上限 | A-3 + B SEC-002/PERF-004/005 | High | **修**：复用 `validate_base_url` + 共享 Client + 8MB 上限 |
| 本机任意端口 Origin 过 CSRF | B SEC-001 High；A-15 Low；C **接受为例外** | 分歧 | **修有界**：状态变更要求自定义头（简单 form CSRF 带不上），CLI/无头仍可用 token 或头豁免 |
| 工作区 `root` 任意绝对路径 | A-2 High + B SEC-006 Medium；C **接受为产品语义** | 分歧 | **修有界**：`root` 必须是 `working_dir` 或已登记项目前缀 |
| 双写未闭环 / fork 不拷 event_log | A-5 High + B ARCH-004 Medium；C 冻结接受 | 部分 | **修 fork**：分叉时拷贝/replay 该会话 event_log（不解开 M3 双写冻结） |
| 装配层过重 + 运行期 `register_port` 吞错 | A-4 High；C 标演进可接受 | Medium | **只修吞错**：运行期注册失败 fail-fast；拆文件本轮不做 |
| api_key 经 Port JSON / GET 明文回传 | A-8 Medium + B SEC-005 Medium | Medium | **修**：GET 掩码；`resolve_config` 去掉 JSON 里的 key，走 CredentialsPort |
| 已注册面被生产旁路（set_wake / credentials） | A-6 Medium；C 按定调不立项 | Medium | **修接线不删面**：`set_wake` 经 SchedulerPort；Llm 取 key 经 CredentialsPort |
| 前端无契约/无 vitest | A-10；上轮遗留 | Medium | **本轮不做基建**（成本大，非正确性回归） |
| wiki 字面量进通用分派 | A 观察 + B ARCH-005；C 门面可接受 | Medium | **本轮不做**（排除 WIKI 功能；注册表化是演进） |

### ★☆☆ 单工具、实证够强、纳入本轮

| 项 | 来源 | 理由 |
|---|---|---|
| 内置 read/write/edit 绝对路径不圈禁 | B SEC-003 High | 门注释自称「工作区 safe_join 圈禁内」，实现却放行绝对路径——注释与代码打架，修圈禁 |
| `referer_allowed` 任意 `tauri://` | B BUG-005 | 与 Origin 白名单不对齐，一行收口 |
| Markdown `javascript:` href | B QUAL-006 | 前端 `a` 组件拒非 http(s)/# |
| LoopHooks 模块头「五个扩展点」过期 | A-9 | 改注释，不删挂点 |
| `}impl` 同行 | A 优美 | 格式一行 |

### 明确不修（定调 / 已接受 / 未过材料性门）

- 待接线服务面 / EventBus / `declare_event!` / LoopHooks 空挂点 / `enqueue_turn` / 死契约变体：**不删**
- 双写对账、messages 收口：**冻结至 M3**
- EventBus 换轮询、每步全量投影、roles.json 每步读盘：性能可接受
- 前端 vitest / OpenAPI：遗留
- 子代理 env、Bearer 非恒定时间：Low，本地威胁有限
- 无 Origin 放行 curl：保留；用自定义头补浏览器 CSRF
- 拆 `serve_inner` / `extensions_js.rs`：演进，非本轮

## 四、与 2026-08-16 基线

上轮 P0–P2 九项代码确认仍在修复态。本轮 **新主伤** = 服务面 clone 配置造成的运行时双权威（当时 Port 刚铺开，热路径对打尚未形成）。C 的 FAIL 主因从上轮「权限门未挂」换成「配置三持有」。

## 五、修复状态（本轮已落地）

| 序 | 项 | 状态 |
|---|---|---|
| 1 | P1 配置单锁 + 门闩读实时档位 + `put_config` invalidate + CompatEngine `set_policy` | ✅ |
| 2 | 插件 HTTP：`validate_base_url` + 共享 Client + 8MB | ✅ |
| 3 | CSRF：`X-BoenMind-Client`（浏览器请求必带；curl 无 Origin 仍放行） | ✅ |
| 4 | workspace `root` / 终端 cwd 白名单（working_dir + APP + `trusted_project_roots`） | ✅ |
| 5 | 内置文件工具 `safe_join(cwd)`，绝对路径拒绝 | ✅ |
| 6 | fork 复制 event_log（失败只告警，不回滚 messages） | ✅ |
| 7 | GET config 掩码 api_key；LlmPort JSON 去 key，消费走 CredentialsPort | ✅ |
| 8 | set_wake 优先 SchedulerPort | ✅ |
| 9 | 运行期 register_port fail-fast；Referer 与 Origin 对齐；md 拒 `javascript:`；LoopHooks 模块头；`}impl` | ✅ |

定向验证：`cargo check -p bm-server --tests` 绿；`cargo test -p bm-core --lib workspace config` 10 过；`cargo test -p bm-server --lib live_policy llm_port resolve_root` 3 过；`pnpm exec tsc -b` 绿。

---
*交叉校验产出；单工具发现已标来源。*
