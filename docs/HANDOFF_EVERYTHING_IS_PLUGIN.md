# HANDOFF —— 万物皆插件架构（BoenMind 2.0）

> 2026-08-14 交接。**状态：用户已拍板开工；阶段 0 最小内核（T0-T13）已于当日夜间完成并推送（2 commit）。**
> 交接原因：对话上下文将满，新开对话续接。

## 〇、最新进展（2026-08-14 夜：内核落地）

用户睡前授权"就交给你了"，拍板点 1/3 视为已拍板，从 T0 开工。**阶段 0 全部完成**：
- commit `255b2ac`：最小内核四件套——bm-protocol（契约层，零运行时依赖）/ bm-kernel（Ctx/Registry/EventBus 四分发/Loader/EventLog 内存实现/校验器/消息面投影）/ bm-storage-turso（EventStorePort 落库 + checkpoint + DualWriter）+ 分支 fork（T12 随 T9 完成）；80 测试全绿，clippy 零警告
- commit `ae72750`：T11 双写——bm-server chat 路由在现有落库同时写事件日志（UserMessage/TurnStart/工具/AssistantMessage/TurnEnd，失败不阻断主链路）；投影合并修正（占位 assistant 填充）；30 轮模拟双写验收测试（重放两次字节一致 + 消息面 60 条）
- **实现期修正**：Schema 的 AUTOINCREMENT 改为应用层分配 seq（全局计数与"分支内连续"矛盾 + 事务回滚留空洞），UNIQUE(session_id,branch_id,seq) 兜底
- **partial 标注**：ToolResult.output 暂不落日志（agent-loop 移植时补）；subscribe 事件流（阶段 1）；删除会话时事件日志不联动清理

**下一步（阶段 1）**：agent-loop 移植（bm-core 的 agent.rs 事件 → 事件日志完整化，补 ToolResult output）、pi-compat、分支 UI（二期 A1）。

## 一、一句话现状

用户已拍板战略方向：**BoenMind 2.0 = "万物皆插件" + Agent OS 框架**（一切皆插件，含记忆/网络/RPC/UI；长远 Agent OS；编程应用第一优先）。四层文档全部完成并推送，**唯一待办 = 用户拍板后开始实现（T0 起）**。

## 二、四层文档索引（全部已推送，main 分支）

| 层 | 文档 | 内容 |
|---|---|---|
| 战略 | `docs/boenmind-strategic-review.md` | 命名即愿景（心智平台非软件集合）、战略五问、三护城河（可审计心智/软件形态革命/Steward）、风险全景、五年路径、**时间哲学（实现速度∝Token）**、项目第一性动机（§七 用户原话） |
| 战术 | `docs/everything-is-plugin-architecture.md`（v0.11，~900 行） | 三条铁律（〇·二）、概念映射表（〇·一）、四家+赛道借鉴（D1-D10/P1-P6/Z1-Z6/H1-H12/A1-A12）、平台驱动层（四·A）、前端=DE（四·B）、应用=软件安装（四·C）、核心机制（§五）、记忆/网络/RPC/应用插件/权限/会话生命周期（§6.1-6.5）/Steward（6.7）/编程应用（6.8）、渐进路线 7 阶段、Simplicity Check S1-S12、挑战假设、迭代清单 |
| 实现 | `docs/kernel-implementation-plan.md` | 最小内核实现方案：bm-protocol（纯契约 crate，零依赖）/bm-kernel/bm-storage-turso；接口签名；event_log+branch_heads Schema；**任务清单 T0-T13**（带依赖图）；10 条约束；测试策略 |
| 研读 | `docs/deepseek-harness-evaluation.md`、`docs/ai-os-landscape.md`、`docs/hanaagent-evaluation.md` | dsh 研读（2026-08-14）；AI OS 赛道四项目源码级（AIOS/MemGPT/Life Agent OS/kernel.chat）；HanaAgent（记忆/沙箱参照） |

## 三、用户定调清单（新对话必须遵守，勿再讨论）

1. **三条铁律**（〇·二）：① Agent OS = 用户空间 OS，永远寄生宿主 OS（Windows/Linux/macOS 是 HAL，平台驱动内部只调宿主 API，不碰硬件）；② 会话即生命周期，边界由 Agent 自主决策（Steward 是决策者，前台小弟专注干活）；③ 渐进式、复用优先、**吸收不进核心**（吸收只到插件/应用/驱动三层，隐形核心膨胀 = 第一大忌）。
2. **成本不谈**：核心架构阶段不谈成本优化（"节省是目标，不能本末倒置"）；文档中成本项一律标注"实现期调优"。
3. **编程应用第一优先**：主线 B = 编程 > Wiki（顺延，xu-wiki-desk 先独立演进后收编）> 相册。编程应用自举里程碑 M1-M5（最后 M5 = 用 BoenMind 做 BoenMind 完整功能）。
4. **应用互操作**：应用 = 服务+工具+事件域；数据互通三种机制（能力调用/事件订阅/数据血缘 source_seqs），统一在事件日志上，全部留痕。
5. **时间观**：实现速度 ∝ Token 非人力——地基期（阶段 0-3）人力密集，爆发期（阶段 4+）100 个小弟并行（token 密集）。自举 M5 = 人力模式切 token 模式的开关。
6. **项目动机**：理想载体非商业载体（战略文档 §七）——不妥协愿景、可持续（自举不依赖外部资源）。
7. **用户偏好**：技术解释大白话；先讨论后实施（拍板点列清单再动手）；自动推送政策（任务完成即 commit+push）；Rust 项目内用 Rust 组件不调系统二进制。

## 四、拍板点（用户拍板后从 T0 开工）

1. **方向立项**：按架构 v0.11 + 实现方案开工？（默认等用户确认）
2. **应用插件前端隔离机制**：A iframe（推荐先落地）/ B WebComponent / C 联邦——阶段 4 才需要，可后拍。
3. **阶段 0 启动时机**：会话事件日志层（T0-T11）零风险可立即先行——建议拍板即开工。

## 五、关键事实（已查证，勿重复调研）

- **pi QuickJS 引擎可拆**（拆法 A）：`PiJsRuntime` 自包含、零 session 耦合；vendor 6 文件（extensions_js.rs+scheduler.rs+hostcall_queue.rs+hostcall_io_uring_lane.rs+embedded_assets.rs+error.rs）+ 拷 ExtensionPolicy 等 5 符号 + 自写 ~300 行 host 线程 = **1-2 周**；pi.dev 200+ 插件当日兼容。
- **xu-wiki-desk 是应用插件实证**：D:/96_CoderWorld/xu-wiki-desk（Rust server+Tauri+React，22 表 38 API 28 测试全过）；LLM 网关已插件化（trait LlmProvider，受控子步骤模式）。
- **四家+赛道研读结论**：机制派无人做全"事件日志+动态插件+应用 UI+Steward"组合；Life Agent OS 最接近（分支日志/Port 契约/Custom 事件验证了我们的设计）；kernel.chat 的 acap 降级/taint/配额/审计链直接可吸收。
- **研读副本**（浅克隆，勿当上游库）：D:/96_CoderWorld/deepseek-harness、hermes-agent、life-agent-os、kernel-agent-os。
- **已装 skill**：code-architecture-skill（CarterIrish，MIT）→ `C:/Users/Boen/.zcode/skills/code-architecture-skill`（9 点一致性审计方法论已用）。
- **现有代码规模**：bm-core 5966 行 + bm-server 5388 行；vendor pi 41.4 万行（P1-P10+A1/A2 补丁台账）。

## 六、新对话续接建议开场

> 继续 BoenMind 2.0 万物皆插件项目。交接见 docs/HANDOFF_EVERYTHING_IS_PLUGIN.md 与 docs/boenmind-strategic-review.md、docs/everything-is-plugin-architecture.md、docs/kernel-implementation-plan.md。待拍板项：方向立项/前端隔离/阶段 0 启动。拍板后从 T0 开工（kernel-implementation-plan.md 任务清单）。

## 七、Git 状态

- main 最新：`41d0cb0`（docs: v0.11 应用互操作 + 战略 v1.1 时间哲学）
- 工作区干净（docs 全部已推送）
- 记忆索引：MEMORY.md 有 everything-is-plugin-architecture 条目（v0.9 版，可更新到 v0.11）
