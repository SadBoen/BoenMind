# M8 里程碑回看记录(基线 §19 门)

## Evaluation Record

```text
milestone_id:         M8(首批真实 App 与发行质量;阶段一收官里程碑)
build_or_commit_id:   96b2807(T0 规格/ADR-0011/合同)→ d8fc2ce(T1 双 App)
                      → 844b245(T2 取消贯穿/多 Surface)→ 8e7e699(T3 Judge/
                      实网压测)→ 434f5ff(T4 数据面)→ 45fa569(T5 发行面)
                      → 本提交(T6 回看)
test_run_id:          cargo test --workspace(2026-08-30,本机)
                      = 229 passed / 0 failed(M7 213 → M8 229;增量:t110-t113
                      双 App、t114-t115 取消与多 Surface、t116 实网压测
                      (#[ignore])、t117/t117b Judge、t118-t119 数据面、
                      t120/t120b 发行面)
log_range:            事件 43(零增发;M8 全部走既有事件面);wire method +1
                      (capability.cancel,M8.3);evaluation-report.v0_1 新合同;
                      SQLite v7→v8(evaluation_reports,迁移路径首演练)
deterministic_checks: validate.py 全绿;GT-01(12 条回合流)/02/03/04/05 回归绿;
                      Judge 同区间两次评估逐字节一致(t117)
failure_tests:        t103 超时/t105 熔断/t106 封禁(存量)+ t114 迟到完成丢弃
                      + t119b 损坏隔离重建 + t119c 删除墓碑回放不复活
replay_result:        长任务(t116,实网 6 回合)全事件流喂 Judge → 5 pass/
                      0 fail;GT-06(评估报告轨迹)随 T6 定稿
llm_evaluation:       M8.7 兑现——独立 Judge(bm-judge)为规则型确定性评估器;
                      LLM 定性注解层留接口(#[ignore] 实网),不进报告必填字段
known_failures:       见 §6 条件与遗留
architecture_changes: ADR-0011(App = MCP server 形态);合同 Minor:
                      capability.cancel + evaluation-report.v0_1 + SQLite v8;
                      C4 模型零改动(App 经既有 MCP 拓扑接入)
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-30
```

## §5 逐门记录

- **A 功能测试**:M8.1 Wiki App(真实写盘/读取/列表,收据 = sha256+字
  节数)、M8.2 Market App(整数分确定性、可逆组合账本)、M8.3 多 Surface
  协作(Web 连接发起/CLI 连接取消/取消贯穿子进程)、M8.4 长任务压测
  (实网 6 回合)、M8.5 备份/迁移/恢复、M8.6 三平台发行、M8.7 独立
  Judge、M8.8 删除墓碑/保留期——全部有测试实证。
- **B 回归测试**:M1–M7 存量全绿;GT-01–05 回放绿。
- **C 故障测试**:取消后迟到完成丢弃(t114)、备份目录可完整接管
  (t118)、坏库隔离留档(t119b)、删除不复活(t119c)、未匹配 intent
  判 fail(t117b)。
- **D 日志回放**:实网长任务全事件流自事件日志重放并由独立 Judge 出
  报告——「长任务可以回放和评估」直接兑现(通过条件第二句)。
- **E 确定性评估**:Judge 五项检查全部确定性;Market 同查询逐字节同
  结果(t112);报告 round-trip 落库一致。
- **F LLM 评估**:规则型为主(见上);LLM 注解为可选 #[ignore] 层。
- **G 架构复盘**:App = MCP server(ADR-0011)使八子项中五项(真实
  App/隔离/收据/回放/评估)零内核改动兑现——「机制进内核、策略留外
  围」的复利;备份 = checkpoint+拷贝(拒绝 VACUUM INTO 对源库的句柄
  依赖,更贴近运行中快照语义);迁移 expand-contract 纪律首次实弹。
- **H 验收裁决**:passed_with_conditions。
- **I 性能冒烟**:记录⑥——P-01 p95 落回 M5 区间(证实 M7 噪声判定)、
  P-03 +0.6%/+1.7% 门内、P-08 门内;App 进程隔离使 Runtime 热路径
  零新增。

## 基线 M8 通过条件结算(五句逐条)

1. **至少两个真实 App 使用同一套 Runtime、Broker、Task 和日志机制** ✓
   ——Wiki(文件域真实写盘)与 Market(确定性 fixture)以 MCP server
   接入(ADR-0011),同 Runtime 共存(t113),调用全过 Broker(审批/
   Grant/审计)且审计同源事件日志(t113 断言)。
2. **长任务可以回放和评估** ✓——实网 6 回合长任务全事件流自日志重放,
   独立 Judge 出确定性报告 5 pass/0 fail(t116/t117);报告落库 round-trip。
3. **关键副作用有执行收据** ✓——Wiki page.write 收据(sha256+bytes)+
   outbox published 对账行(t110);删除断言级联墓碑(t119c)。
4. **三平台完成端到端回归** ✓(条件见 §6-1)——CLI/HTTP 形态回归由
   m3/m4/m7 既有 e2e + t115 双连接协作承载;Web UI v1 真实资产服务回归
   (t120:页面/健康/鉴权 rpc);Windows Tauri 壳 = 同源前端 + 工程骨架,
   构建产物因 tauri-cli 未装未出(如实降级,复现命令已留档)。
5. **历史会话不因发布和迁移损坏** ✓——v7→v8 迁移演练零丢失(t119a)、
   备份恢复后 resume + 新回合 + 旧收据三断言(t118)、坏库隔离重建
   (t119b)、删除墓碑经重放存活(t119c)。

## 前置结算与承接项闭合

- **M7-review 条件 1(实网稳定性复测)**:兑现——t116 真实通道 6 回合
  18.9s 全终态, Judge 全过(M8.4 硬条件)。
- **S5 quarantined 分表**:兑现——t119b(隔离 + 重建 + 留档取证)。
- **S4 draining(两步摘除)**:未入 M8 实测(压测未触热替换场景);
  留档移交后续里程碑(见 §6-4)。
- **lease 通道真实吞吐**:未实测——首批 App 的 MCP 路径走收据/对账,
  未触发数据面 lease;留档移交后续(见 §6-4)。
- **D-M5-2 memory:user 授权面**:不变——memory:user 裸域已可用
  (t119c 即用例);按主体区分的授权面仍随多用户形态(M8 未含多用户)。
- **M6-review worker 自主 turn 环**:部分兑现——t116 以会话回合编排
  长任务;worker 主体自主循环仍是编排面增量(见 §6-4)。

## §6 条件与遗留

1. **Tauri 构建产物未出**(tauri-cli 未安装;按 M8 规格 S7 降级条款交
   付工程骨架 + 复现命令)。条件:后续安装 tauri-cli 并完成一次
   `cargo tauri build`,产物留档后解除。
2. **Web UI v1 为功能单页**(会话/回合/审批/任务):富交互(任务甘特、
   流式输出渲染)留后续;前端与壳同源纪律已立(ADR-0009)。
3. **实网压测样本量小**(单次 6 回合):稳定性结论为「通道可用、链路
   无挂起」,非统计意义 SLA;后续例行压测积累。
4. **遗留移交**:S4 draining 两步摘除、lease 通道真实吞吐、worker 自主
   turn 环、memory:user 按主体授权、模型流式(SSE 输出)、MCP HTTP/SSE
   传输——均已在规格/review 留档,未丢失。
5. **发布包分发**:本机出包已验(exe 13.9MB);安装器签名、自动更新
   未含(阶段一口径)。

## §7 回看七问(基线)

1. **计划与实际的偏差?** 规格七裁决全部落地。偏差:lease 首测与
   draining 实测未触发(App 形态不依赖 lease;压测未触热替换)——如实
   留档而非强行造景。T5 的 Tauri 构建按预设降级条款处理。
2. **哪些是临时绕路?** 备份用 checkpoint+拷贝替代 VACUUM INTO(运行中
   快照语义更贴合,非绕路);Tauri 骨架为规格内降级。
3. **合同是否被破坏?** 否。capability.cancel 与 evaluation-report.v0_1
   均为 Minor 纯追加;SQLite v8 为 expand;validate.py 全绿。
4. **性能是否触门?** 否。P-01/P-03/P-08 全门内(记录⑥);App 进程
   隔离使 Runtime 热路径零新增。
5. **安全边界是否松动?** 否。App 经显式安装批准接入;页面名白名单 +
   数据域隔离;取消贯穿不加旁路(语义取消 + 迟到丢弃);令牌/密钥
   零入仓(INV-5)。
6. **下一个里程碑最需要什么?** 阶段一(M0–M8)已收官。下一步不在
   §18 清单内:按回看遗留(S4 draining、lease、worker 自主环、流式
   输出)+ 实际使用反馈起草「阶段二规划」;先跑一周真实使用再定优先级。
7. **如果重做会怎样?** 会把 M8.5(备份/迁移)提到 M8.1 之前——数据面
   是 App 的安全网,先有安全网再上真实 App 心理与工程顺序都更顺;
   其余(尤其 ADR-0011 的 App 形态)不变。

## 阶段一收官声明

M0(合同)→ M1(最小闭环)→ M2(持久化/恢复)→ M3(Surface/CLI)
→ M4(Capability/Broker)→ M5(Butler/Task)→ M6(Team/Delegate)
→ M7(Provider/MCP)→ M8(真实 App/发行)全部收官。个人生态 AI
Runtime 的「跨平台单软件」形态成立:同核三 Surface、能力即权力、
事件即事实、回放即评估。
