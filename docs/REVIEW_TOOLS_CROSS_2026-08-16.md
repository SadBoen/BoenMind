# 三工具交叉审查报告 2026-08-16

> 方法：三个独立审查工具各全库审查一次 → 问题交叉校验（聚类去重、共识度标记、关键点实证复核）。
> 独立报告：`docs/review-tools-2026-08-16/TOOL_A_code-architecture.md`（29 条）、`TOOL_B_codebase-reviewer.md`（26 条）、`TOOL_C_ln24-architecture-auditor.md`（14 条）。
> 审查范围：backend/crates（9 crate）+ backend/plugins（6 插件）+ backend/tests/event_log + frontend/src；排除 vendor/target/node_modules/docs。

## 〇、一句话结论

三工具一致认为：**骨架质量上乘（依赖方向机器守卫/事件日志事务/压缩协议/权限 fail-closed 都是真功夫），核心问题集中在"先建后接的抽象前置"与"双写过渡未收口"两个结构主题；另有一条已实证的测试断裂（事件日志验证根基）和一处双工具判 High 的权限链执行态失效**——架构/精简/复用维度最该动手的是"砍无消费者扩展点 + 抽统一回合运行器"。

## 一、三工具概况与分工差异

| 工具 | 来源 | 工作流 | 发现 | 强项 |
|---|---|---|---|---|
| A code-architecture | 本地标准 skill | 3A 架构审查：现状映射→亮点→风险分级→结论 | 29（架构15/精简8/复用3/其他7） | 复用维度最敏锐（编排重复/协议四侧重复/死代码） |
| B codebase-reviewer | npm（cidoliveira） | 全库六维穷举 ARCH/SEC/PERF/QUAL/BUG/IMP | 26（High 3） | 实证纪律（跑 cargo check 抓到编译断裂）；安全六维最全 |
| C ln-24 architecture-auditor | GitHub（levnikolaevich 537★） | 清单式 DoD 审计（44/44）+ 证据规则过滤 | 14（Verdict FAIL） | 契约/所有权分析最深（downcast 可换失效、TS 轨隔离） |

三工具交叉后去重，独立问题约 38 个；**多工具共识度 = 可信度信号**，单工具发现按实证强度对待。

## 二、共识问题（按共识度与优先级）

### ★★★ 三工具共识（可信度最高）

**主题 1：先建后接的抽象前置——项目自定 YAGNI 判据（"第一个第二实现出现时"）未被执行**
- A-1/2/12、B-ARCH-002/QUAL-001~004、C-F2/F5/F6/F8/F12 从五个角度打到同一病根
- 具体存量：服务面 13 面中 **memory/notify/scheduler/tools/credentials 5 面零生产消费者**；EventBus（374 行，四种分发模式）生产零调用；`declare_event!` 宏零生产使用；LoopHooks 12 挂点中 8 个无消费者；`enqueue_turn` 队列 API、`StepEnd/SessionEndSeed/HeaderReason::Resume` 契约变体、`pi_name` 24 路映射、fork 分支机制（schema+语义全落地但零发射者）全是死重量
- 定位：**架构+精简双维度第一优先**。每轮"顺手铺面"会让规模随轮次线性膨胀；删/收均无风险（代码可留、撤注册/删调用点即可回滚）
- 建议：一次清理定调——5 个无消费面撤注册；总线/挂点/队列按"第二个真实消费者出现"再恢复；死契约变体删除或补写者；fork 在文档标注"发射者随 M3"

**主题 2：双写事实源未收口——messages 表与 event_log 两存储并行、无对账**
- A-6、B-ARCH-004、C-F4 三方一致
- 现状：前端历史读 messages 表（权威），todo 投影读 event_log；崩溃窗口①（add_message 后、UserMessage 落日志前）日志缺用户消息、窗口②（TurnEnd 后、add_message 前）db 缺助手文本，均无修复/对账路径；dual_write.rs 自述"事件日志是渐进式吸收的新家不是闸门"与文档 §5.1"一切状态的唯一事实源"方向相反
- 定位：架构维度第二优先。修复有界：启动对账任务（以 event_log 为准重建 messages，幂等），或显式把 M3 提为 backlog 冻结双写范围并在 §5.1 如实标注当前真相源

### ★★☆ 双工具共识

**主题 3：权限链执行态失效（B+C 均判 High）——安全维度最重**
- B-SEC-002 + C-F1：内置工具（bash/read/write/subagent/todo）完全绕过权限门——`on_tool_pre` 生产恒 Allow（bm_engine.rs:238-249），`QuickJsToolExecutor` 四分支零检查；而沙箱 QuickJS 插件反被严格设闸（host.rs:131-160）。**威胁方向倒置**：不可信插件层层设闸，能执行任意进程的内置 bash 无闸——提示注入（模型读到网页/文件）即可触发任意命令执行
- B-ARCH-001：前端安全设置页四档权限（safe/balanced/yolo）UI+持久化完整，后端从不读取 `extension_policy`——**死配置**，用户选 yolo 不放开 exec/env，选 safe 不收紧
- 建议：内置工具经同一 Gate（bash/subagent 至少一次询问/决策记忆，复用 permission_store，改动有界），或显式文档化"内置手脚=可信边界"+ taint 落地；档位配置接入 `ExtensionPolicy` 映射

**主题 4：记忆双实例（A-7 + C-F11）**
- kernel 注册的全局 memory 面零消费者；每会话又 open 同一 facts.md——两实例互不感知，同一事实可能双写/互不可见。建议：随主题 1 一起撤销 memory 面注册，或让会话实例改经全局单例（这正是"服务面=承诺 API"设计的首个用例）

**主题 5：context_window 死配置（A-10 + C-F7）**
- 硬编码 128_000，压缩水线/窗口预算/工具裁剪全由它驱动；`CompactionOverride.context_window` 字段序列化在案但从不被 `effective()` 读取。换小窗口模型时硬触发可能失灵（超窗不触发压缩即回合失败）。建议：`effective()` 消费该字段注入 LoopConfig，或删除字段

### ★☆☆ 单工具发现（按实证强度/价值筛选）

**主题 6：event_log 集成测试套件整体编译断裂（B 发现，已独立复核确认）——最紧急**
- `cargo check -p bm-event-log-tests --tests` 实测失败：checkpoint_recovery.rs:8 引用已删除的 `CheckpointState/CheckpointStore`。**全部 7 个集成测试（replay 确定性/双写/fork/ignorable/orphan/proptest）当前无法运行**——"事件日志是唯一事实源"的验证根基断裂，CI 若只跑单 crate 测试会被静默放过
- 修法二选一：(a) 恢复 Checkpoint 实现；(b) 删除该测试文件 + 更新 Cargo.toml 描述（仍残留"+ checkpoint 策略"）。建议先修这个

**主题 7：回合编排重复（A 发现，复用维度第一发现）**
- chat_bm（552-714）与 run_steward_turn（909-1039）~150 行平行重复：超时 task/心跳/watchdog/attach/detach/收尾，仅参数不同；第三个回合源（Goal）将再复制一份。建议抽 `SessionRunner::run_prompt(state, session, source, 是否接SSE)` 统一封装——这正是架构 §14.1"共用同一套循环内核"承诺的落地

**主题 8：压缩插件"可换"契约失效（C 发现）**
- 消费者按具体类型 `kernel.service::<bm_compactor::DefaultCompactor>` downcast（bm_engine.rs:385-396），注册第二实现会 downcast 失败并**静默回落默认参数**——文档 §6.9"策略层已插件化可换"运行期不成立。建议改经 `dyn Compactor` port 或构造注入，并在文档去掉"可换"承诺直至真实替换路径验证

**主题 9：本地 API 无 CSRF 防护（B 发现，root 行为已复核）**
- `BOENMIND_TOKEN` 默认不设；CORS 只挡读取不挡发送；`/api/workspace/file` 的 `root` 参数接受任意绝对路径（resolve_root 测试自证，safe_join 只护 rel）。任意网页可 POST /api/chat 驱动本地 Agent 或读写任意文件。建议：非 GET 请求校验 Origin/自定义头，至少覆盖 workspace 写/chat/updates/plugins 四组端点

**其余单工具发现**（完整清单见三份独立报告）：
- A：api_key 经 Port 边界 JSON 明文流转（A-11）；工具结果 content-blocks 协议四侧重复实现（A-18）；权限询问双通道并存（A-15）；死代码三件 clip_tool_output/ChatWindow/ExpertTeamDocs（A-14）；"经 port、退化直调"双路径样板重复 6+ 处（A-13）；装配层上帝函数（A-23）；NotifyPort 静默丢事件（A-20）；http hostcall 每次新建 reqwest Client（A-21）；文档-代码漂移 5 处（A-22）
- B：每步全量重放投影+双序列化 O(n²)（PERF-001）；SSE 解析三处复制（QUAL-005）；权限裁决两套独立实现 check_capability/check_exec_capability（ARCH-003+QUAL-006）；取消→ToolResult 缺失日志永久未闭合（BUG-002）；ctx-compactor 轮转非行边界截断（BUG-003）；权限询问落在 prompt 收尾窗口即静默丢失（BUG-004）；权限记忆 tempdir 泄漏（BUG-005）；插件默认放行 http（SSRF 面）（SEC-003）；子代理 child 模式仅凭 env 判定（SEC-004）；roles.json 每步读盘（PERF-003）；ctx-search 全量读盘（PERF-002）
- C：TS 插件轨与 kernel 注册表轨完全隔离，文档承诺"插件互相看见"对现存 TS 插件不可达（F9，建议文档标注能力边界）；bm-server 组装层承载 ~3400 行应用域逻辑（F13，阶段 4 按模块收编）；memory/role 注入同构算法两份实现（F10，D8 PromptSection 注册表是规划统一家）；轮询订阅无退避（F14）

## 三、工具本身评估（对以后选工具回头看有参考价值）

- **codebase-reviewer（B）最会实证**：唯一跑了 cargo check、唯一系统性扫了安全六维；盲区是复用维度（编排重复/协议重复全漏）
- **ln-24（C）契约分析最深**：唯一挖出 downcast 可换失效、TS 轨隔离、fork 死维度；但只报 14 条、不看性能/前端细节
- **code-architecture（A）复用与装配最敏感**：唯一报出编排重复、双路径样板、api_key 流转；但漏掉编译断裂与 CSRF
- 结论：**三工具互补成立**，交叉校验把"单工具盲区"补成了全谱；建议保留三件套做定期回头看（B 每次必跑编译实证）

## 四、修复优先级建议（待拍板）

| 序 | 项 | 来源 | 类别 | 成本 |
|---|---|---|---|---|
| P0-1 | event_log 集成测试编译断裂（删 checkpoint 测试 or 恢复实现） | B（实证复核✓） | 其他/验证根基 | 小 |
| P0-2 | 内置工具接权限门（或文档化信任边界） | B+C（High） | 架构/安全 | 中（一个裁决点） |
| P0-3 | 本地 API Origin 校验（四组端点） | B（root 行为复核✓） | 安全 | 小 |
| P1-1 | 无消费者扩展点清理：5 面撤注册 + 总线/挂点/队列/死契约变体二选一 | 三工具共识 | 架构+精简 | 小-中（一次定调） |
| P1-2 | 双写对账任务或显式冻结 + §5.1 如实标注 | 三工具共识 | 架构 | 中 |
| P1-3 | 权限档位接入 ExtensionPolicy（UI 设置生效） | B | 架构/死配置 | 小 |
| P2-1 | 抽 SessionRunner 统一回合编排（~150 行重复） | A | 复用 | 中 |
| P2-2 | 记忆双实例合一（随 P1-1） | A+C | 精简 | 小 |
| P2-3 | 压缩插件改 dyn Compactor 取用（可换契约恢复） | C | 架构 | 小 |
| P2-4 | context_window 接入模型注册表 | A+C | 架构/死配置 | 小 |
| P3 | 其余单工具低优先项（双路径 helper/SSE 复用/roles 缓存/死代码三件等） | A/B/C | 精简/复用 | 随轮 |

**建议顺序**：P0-1（10 分钟定掉）→ P1-1（一次清理定调）→ P2-1（复用最大收益）→ P0-2/P0-3（安全轮）→ 其余随轮。

---
*本报告由三份独立工具报告交叉校验产出；单工具发现均已标注来源，P0 项经独立复核。*
