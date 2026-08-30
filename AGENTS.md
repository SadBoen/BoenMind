# BoenMind — AI 工作规程(新会话必读)

## 这是什么

BoenMind:个人生态的 AI Runtime / AI OS,当前为**阶段一(跨平台单软件)**。
设计已定稿,经三模型辩论复核(五裁决修订后成立 + 两条新裁决,见 `adr/`)与三真实系统
对照验证(Erlang/OTP、Kubernetes、VS Code,见 `architecture/deepwiki-validation.md`)。
合同库已冻结 v1.0(字段只增不破)。

**当前进度:M9 已收官(2026-08-30,tag `m9-stage2-batch1`):
`milestones/M9-review.md`——阶段二第一批三轨全落地(S1 记忆抽屉授权闭合
D-M5-2 / S2 模型真流式 BOEN_MODEL_STREAM=1 / S3 worker 自主环 v0 哨兵完成+
停滞+超限+暂停四出口),254 测试全绿,t144 实网流式通过。
下一步 = 按既定策略先真实使用一周攒手感;候选队列:远程 MCP(用户拍板
下一批)、自主环真工具闭环、web 版调整(用户指定优先于桌面壳)。**

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
adr/                            第 0 层  架构决策记录(ADR-0001..0011;基线正文与 ADR 冲突时,以更新的 ADR 为准)
architecture/                   第 0 层  C4 模型 boenmind.c4(拓扑唯一权威)+ 辩论转录(debates/)+ 验证报告
boenmind-contracts/             第 1 层  机器可读合同(v1.0 冻结)+ validate.py 校验器 + m0/ 测试基准
milestones/                     第 2 层  里程碑实现规格与回看记录(M1 起建)
runtime/                        第 3 层  源代码(M1 起,Rust workspace;crate 划分在 M1 规格中定稿)
```

## 新会话工作流

1. 读本文件 → 2. 按手头任务读对应层文件 → 3. 动手前对照下方进度确认当前里程碑 →
4. 产出后自检(合同有变更必跑 `python3 boenmind-contracts/scripts/validate.py`,须全绿)。

## 环境与工具备忘

### 真实用户面踩坑(2026-08-30,浏览器实测四连)
1. **事件信封 JSON 字段名是 `type`**(serde rename),不是 Rust 字段名
   `event_type`——前端按后者读永远 undefined;
2. **EventSource(SSE)无法携带 Authorization 头** → /events 被 401 静默
   拒绝;前端改走合同方法 `events.poll` 轮询(1.5s);
3. **静态页无缓存头,浏览器缓存旧页**——发版后必须 Ctrl+F5 或带查询串;
4. 内联脚本的语法错误会**整页静默失效**(所有按钮无反应且无报错),
   改完必须 `node --check`。
教训:229 个测试全绿测不出这四个 bug——**P0 之外必须有真实浏览器
端到端手测轮**。

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
- [x] **M7 Provider、MCP 和 App 隔离(2026-08-30,tag `m7-provider-mcp`;
      规格 `milestones/M7-implementation-spec.md`,回看 `milestones/M7-review.md`,
      213 测试全绿,五句通过条件逐条结算,真实网关实网验证通过,
      ADR-0010 第三方网关信任边界)**
- [x] **M8 首批真实 App 与发行质量(2026-08-30,tag `m8-apps-release`;
      规格 `milestones/M8-implementation-spec.md`,回看 `milestones/M8-review.md`,
      229 测试全绿,双真实 App + Judge + 实网压测 + 备份迁移 + 三平台,
      ADR-0011 App=MCP 形态)——阶段一收官**
- [x] **全面回看 M1-M9 整体(2026-08-30,`milestones/FULL-REVIEW-2026-08-30.md`;
      四道门禁全绿 260 测试,架构红线主体干净,结论 passed_with_conditions;
      新发现 F-01..F-11 入审计台账;条件:C4 模型回写列下一批开工前置)。
      随回看用户拍板五项全落定:下一批=先用一周再开工远程 MCP / 桌面包搁置
      骨架保留 / 看护闹钟 15min·3 次·24h 定案 / 三笔追认+GT-01 示例已修正;
      待拍板队列清零**
- [x] **Web 界面改版 v2(2026-08-30,D-M3-1 方案落地;`runtime/web/`:tokens.css
      设计令牌表机械复刻自 dsh ui-theme(MIT)→ 明暗双主题 + dsh 式布局 +
      前端界面插槽(window.boenmind,声明即授权/拒 HTML 串)+ Enter 发送;
      JS 功能逻辑逐字保留;真实浏览器手测通过(流式/审批/任务/CLI/主题/
      插槽冒烟),260 测试全绿;Penpot 速成页 PENPOT-quickstart.md)**
- 注意:ADR-0011(App 形态)与各回看「§6 条件与遗留」是后续规划的
  输入清单;deepwiki S1-S10 逐里程碑裁决中(S5/S9 已闭合,S3/S4/S8 部分
  采纳,余 proposed;总表见 FULL-REVIEW §2.4),勿自动采纳
- 注意:`architecture/deepwiki-validation.md` 的 S1-S10 修订建议仅在各
  里程碑回看时逐条裁决,勿自动采纳。
