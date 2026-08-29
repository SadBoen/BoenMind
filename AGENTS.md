# BoenMind — AI 工作规程(新会话必读)

## 这是什么

BoenMind:个人生态的 AI Runtime / AI OS,当前为**阶段一(跨平台单软件)**。
设计已定稿,经三模型辩论复核(五裁决修订后成立 + 两条新裁决,见 `adr/`)与三真实系统
对照验证(Erlang/OTP、Kubernetes、VS Code,见 `architecture/deepwiki-validation.md`)。
合同库已冻结 v1.0(字段只增不破)。

**当前进度:M6 已收官(2026-08-30,tag `m6-team-delegate`):
`milestones/M6-review.md`——基线 M6 四条通过条件全部强制点化
(成员权限只减不增/委派四门禁/成员故障不破坏 Task/collect 三要素),
196 测试全绿;ADR-0002 条件 5 余项闭合(reservation 裁定延续不做)。
下一步 = 起草《M7 实现规格》(Provider、MCP 和 App 隔离;基线 §18-M7
五子项),沿用提速方案合批推进。**

**提速方案(2026-08-30 起固化,每轮沿用)**:
1. 强耦合任务合批(如 T4+T5、T6+T7、T8+T9),一轮交付、共享全量回归,
   减少回归次数;依赖链顺序不变。
2. 文档类产物(回看骨架、PENDING 大白话、perf-baseline 记录区)派后台
   子代理并行起草——与代码文件零相交;主代理收圈时随手提交。
3. 不做同仓多代理并行写代码(runtime.rs 单点合并成本 > 收益;
   Rust target 目录锁/冷编译),防冲突规程与单写者纪律不破。**

## 文件地图(规格分层,基线 §0)

```text
BoenMind-CORE-ARCHITECTURE.md   第 0 层  架构基线:原则/边界/不变量;§17 七条核心裁决;§18 里程碑;§19 回看制度
adr/                            第 0 层  架构决策记录(ADR-0001..0009;基线正文与 ADR 冲突时,以更新的 ADR 为准)
architecture/                   第 0 层  C4 模型 boenmind.c4(拓扑唯一权威)+ 辩论转录(debates/)+ 验证报告
boenmind-contracts/             第 1 层  机器可读合同(v1.0 冻结)+ validate.py 校验器 + m0/ 测试基准
milestones/                     第 2 层  里程碑实现规格与回看记录(M1 起建)
runtime/                        第 3 层  源代码(M1 起,Rust workspace;crate 划分在 M1 规格中定稿)
```

## 新会话工作流

1. 读本文件 → 2. 按手头任务读对应层文件 → 3. 动手前对照下方进度确认当前里程碑 →
4. 产出后自检(合同有变更必跑 `python3 boenmind-contracts/scripts/validate.py`,须全绿)。

## 环境与工具备忘

- gh CLI 已装;Rust 1.98、Node 24、Python 3.13;tauri-cli 未装(桌面壳手工构建于 web/src-tauri/)。
- 性能测试:`cargo test --release -p bm-testkit --test perf_smoke -- --ignored --nocapture`(perf_m2 同理);P-09/10 常驻测试套件。
- context7 MCP 可用(库文档查询;M7 真实 Provider/MCP 接入时优先用)。
- 已确认:libsqlite3-sys bundled 构建默认启用 SQLITE_ENABLE_FTS5——M5 memory 检索的 FTS5 路径实际生效,LIKE 仅兜底。
- 踩坑:大段内联脚本(python heredoc)写文件易静默失败——先 Write 成文件再执行;cargo fmt 会重排代码,文本替换前先看当前实际内容;测试先行持续抓真 bug;跨字段借用冲突用分阶段作用域解决;时间基准对照 MockClock 实际基准值换算。

## 硬纪律(违反 = 返工)

1. **合同冻结**:boenmind-contracts/ 字段只增不破;删字段/改名/改语义 = Major,走基线 §13.5。
2. **先改模型再改文字**:架构变更先改 `architecture/boenmind.c4`;文字图与模型不一致以模型为准。
3. **决策写 ADR**:新决策在 adr/ 发新文件(编号递增),不修改既有 ADR 的语义。
4. **权限以合同显式化**(ADR-0006):未列入注册合同的权力视为不存在。
5. **里程碑 = 可运行检查点**(§18):P0 测试套件全绿才算完成;完成后按 §19 回看再进下一个。
6. **真实进度只认 git**:主干应始终可校验(validate.py 全绿);提交说明写清动机。

## 进度

- [x] M0 范围/合同/测试基线(2026-08-28,tag `m0.2-contracts-frozen`)
- [x] 2026-08-29 ADR-0009 部署形态裁决:VPS 托管 + Web/交互式 TUI Surface + Windows Tauri 壳(受限解除「无远程访问」;M3 增 HTTP 传输+鉴权合同,M8 增 Web UI v1 与 Tauri 壳)
- [x] **M1 最小 Runtime 与单 Agent 闭环(2026-08-29,tag `m1-runtime-loop`;规格 `milestones/M1-implementation-spec.md`,回看 `milestones/M1-review.md`,50 测试全绿,GT-01 两场景可回放)**
- [x] **M2 持久化/事件日志/崩溃恢复(2026-08-29,tag `m2-persist-recovery`;
      规格 `milestones/M2-implementation-spec.md`,回看 `milestones/M2-review.md`,
      68 测试全绿,四项混沌验收通过,ADR-0004 四项 M2 适配映射已按默认路径落地)**
- [x] **M4 Capability/Broker/权限审批(2026-08-29,tag `m4-capability-broker`;
      规格 `milestones/M4-implementation-spec.md`,回看 `milestones/M4-review.md`,
      134 测试全绿,11 条硬约束全部落地,三 Surface 同源审批闭环,
      模型调用豁免与 capability 操作状态面留档随 M7/M5 复议)**
- [x] **M3 统一 Wire API、CLI 与跨平台启动(2026-08-29,tag `m3-surface-cli`;
      规格 `milestones/M3-implementation-spec.md`,回看 `milestones/M3-review.md`,
      74 测试全绿,CLI/桌面/Web 三形态同源可用)**
- [x] **M5 Butler、Task 和长期监护(2026-08-30,tag `m5-butler-task`;
      规格 `milestones/M5-implementation-spec.md`,回看 `milestones/M5-review.md`,
      188 测试全绿,八项前置结算条件闭合,ADR-0002 口径升级「成立」)**
- [x] **M6 Team、Delegate 和多 Agent 协作(2026-08-30,tag `m6-team-delegate`;
      规格 `milestones/M6-implementation-spec.md`,回看 `milestones/M6-review.md`,
      196 测试全绿,四门禁强制点化,ADR-0002 条件 5 余项闭合)**
- [ ] M7/M8 见基线 §18;33 项验收条件的分布见各 ADR「条件与验收」
- 注意:`architecture/deepwiki-validation.md` 的 S1-S10 修订建议为 **proposed**,
  仅在各里程碑回看时逐条裁决,勿自动采纳。
