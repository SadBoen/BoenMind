# BoenMind — AI 工作规程(新会话必读)

## 这是什么

BoenMind:个人生态的 AI Runtime / AI OS,当前为**阶段一(跨平台单软件)**。
设计已定稿,经三模型辩论复核(五裁决修订后成立 + 两条新裁决,见 `adr/`)与三真实系统
对照验证(Erlang/OTP、Kubernetes、VS Code,见 `architecture/deepwiki-validation.md`)。
合同库已冻结 v1.0(字段只增不破)。

**当前进度:M4 已收官(2026-08-29,tag `m4-capability-broker`)。
M5 前置结算与规格已冻结(2026-08-29):`milestones/M5-implementation-spec.md`
v1.0——ADR-0004 条件 6(触发者两类:用户 resume / Watchdog 自动;停滞窗口
默认 15min、硬顶 24h,基线 §10.3 已补定义,数值大白话见 PENDING D-M5-1)
与 ADR-0002 预算包络二分已落入规格,随 M5 回看逐条闭合。
M5 实现进度:T0 合同增发九项已落地(全 Minor,镜像同步);
T1 已落地(bm-core task.rs 对象/状态机/完成判定门禁/epoch 门禁 + SQLite v4
五表 + T6c 两项收紧经跨重启实证);
T2 已落地(Wire task.* 六方法启用 + events.poll task_id 过滤 + CLI task 组 +
Task Board 投影:重建确定性/启动重建/emit 增量双路等价,P-11 骨架测试);
T3 已落地(butler.rs:bootstrap 协调权 Grant 集物化+跨重启幂等+可撤销、
task.create 协调权门禁与授权上界校验、领域动词不可授权矩阵、GT-01/M1
生命周期测试按启动期系统事实同步更新);
T4+T5 已落地(coordinator.rs 三方交集物化为 task:<id> Grant+授权签发链
parent 哈希+Worker 单成员闭环+双路径统一收据(principal 来源标注)+
Worker 调用命令+Task 终态 Grant 失效+approval task-scope 启用,170 测试全绿;
M5-review.md 骨架已建,T10 回填)。
下一步 = T6+T7 合批(budget.rs 两级账本/包络子分配/扩容受控变更/Broker
预算执行点收编 + watchdog.rs 八项检测/停滞窗口/编排重启触发/G4 守护)。**

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
- [ ] M4/M5-M8 见基线 §18;33 项验收条件的分布见各 ADR「条件与验收」
- 注意:`architecture/deepwiki-validation.md` 的 S1-S10 修订建议为 **proposed**,
  仅在各里程碑回看时逐条裁决,勿自动采纳。
