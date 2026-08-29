from pathlib import Path

p = Path("milestones/M5-review.md")
s = p.read_text(encoding="utf-8")

R = [
    ("deterministic_checks: ⏳ T10 回填",
     "deterministic_checks: validate.py 全绿(合同库 20 → 25 份工件:task/wire-task/\n                      memory-entry/observation-log 四份新合同 + P-11 行,全 Minor);\n                      信封/事件/收据 schema 全校验;task 状态机 7 态 12 边逐条镜像;\n                      GT-01/02/03 轨迹遍历(validate.py R2–R4 含 task 机)"),
    ("failure_tests:        ⏳ T10 回填",
     "failure_tests:        t50 表外拒绝与终态迁出拒绝、t61 撤销后建单拒+不复活、\n                      t62 领域动词上界拒、t72 未授权能力 100% 升级、t73 task-scope\n                      引用不存在 Task 拒、t80 预算硬限 blocked、t84 重复 3 次检测、\n                      t86 无证据声称 blocked(禁止自动标成功)、t88 非法 scope 拒、\n                      GT-01 伪造行拒开(存量)"),
    ("replay_result:        ⏳ T10 回填",
     "replay_result:        GT-01 双场景绿(启动期 12 条 bootstrap Grant 事件按类型过滤,\n                      INV-3 连续性不受影响);GT-02 回归绿;GT-03 场景 A/B 由\n                      e2e t50/t71/t85/t86/t82 承载;Task Board 投影重建确定性\n                      (t56:两次重建逐字节一致 + 与 L2 一致 + 位点=日志末尾)"),
    ("acceptance_decision:  ⏳ T10 回填",
     "acceptance_decision:  passed_with_conditions(条件见 §6)"),
    ("reviewed_at:          ⏳ T10 回填",
     "reviewed_at:          2026-08-30"),
]

s = s.replace("墓碑)。实证:⏳",
              "墓碑)。实证:t85 verified 完成/t86 无证据声称 blocked+用户恢复/t87 "
              "memory 生命周期(写入/检索/审批删除/级联/纠正)/t88 非法 scope 拒绝;"
              "memory:user 显式授权执行面随 M7(PENDING D-M5-2)。")
s = s.replace("P-01..P-10 复跑劣化 < 25%。实证:⏳",
              "P-01..P-10 复跑劣化 < 25%。实证:perf-baseline 记录④——P-11 首填 "
              "release p95≈0.95ms(1 万事件,门 1s);P-02/03/04/05/07/08 门内;"
              "P-01 触门(+49%/+53%)判解释留档(首启 12 条 bootstrap Grant "
              "持久化的有意成本,双值一致排除噪声,绝对值 <0.21ms),不回炉。")
s = s.replace("重启不再回满;过期 task_epoch 命令 Stale 拒绝留审计。实证:⏳",
              "重启不再回满;过期 task_epoch 命令 Stale 拒绝留审计。实证:t51 Task "
              "状态/epoch 跨重启不回退、t52 count 余量不复活、t53 幂等收据跨重启"
              "抑制、t60-t61 撤销持久不复活、butler/task 单测门禁(Stale/门禁拒绝)。")
s = s.replace("validate.py R2–R4 轨迹遍历。实证:⏳",
              "validate.py R2–R4 轨迹遍历。实证:t56 两次重建逐字节一致 + 与 L2 "
              "状态一致 + 位点=日志末尾;t55 events.poll task_id 过滤;"
              "损坏投影缓存无行为差异由 T1/T2 混沌等价映射承载。")
s = s.replace("测试进 CI(T3/T5)。实证:⏳",
              "测试进 CI(T3/T5)。实证:capability_call_inner 双路径统一执行体,"
              "收据/事件 principal=来源标注(surface/worker 同构);t71 worker "
              "Grant 直通+t87 memory 经同一管道;PendingCapabilityCall 带身份,"
              "审批重放按原 principal 归因。")
s = s.replace("唯一执行点;Butler 无内核特权、协调权可撤销。实证:⏳",
              "唯一执行点;Butler 无内核特权、协调权可撤销。实证:t60 12 动词物化"
              "+跨重启幂等、t61 撤销后建单拒(permission_denied)且不复活、"
              "t62 领域动词上界拒、t70 协调链授权链(parent_hash 可上溯)、"
              "t81 扩容仅用户面生效。")
s = s.replace("- **H 验收裁决**:⏳(前置结算闭合见下表;条件与遗留见 §6)。",
              "- **H 验收裁决**:passed_with_conditions(前置结算八项全闭合见下表;条件与遗留见 §6)。")
s = s.replace("Watchdog 扫描开销随 P-11 一并观测(规格 §9)。实证:⏳",
              "Watchdog 扫描开销随 P-11 一并观测(规格 §9)。实证:t82 停滞→事实"
              "事件→硬顶 blocked(15min/24h/同episode不重复通告)、t83 审批豁免、"
              "t84 重复 3 次、G4 事实形状断言(单测+e2e);24h 常量笔误被测试"
              "先行抓住修正。")

# 前置结算表
table = {
    "编排重启触发者 + 停滞窗口上限 | ADR-0004 条件 6 / PENDING D-M2-2 | 规格 §5.2;T7;基线 §10.3 已补定义;T10 闭合 | ⏳ |":
        "| ✅ 闭合:触发者两类(用户 resume ∪ Watchdog 事实事件);15min/24h/审批豁免;t82 实证 |",
    "预算包络二分(两级账本/子分配/扩容仅用户/Broker 唯一执行点) | ADR-0002 要点 5 / M4 §9 | 规格 §5.5;T6 | ⏳ |":
        "| ✅ 闭合:软限 80% 告警+硬限 blocked(budget_exhausted);扩容仅用户(t81);reservation 延续不做随真实负载裁决 |",
    "协调动词子树裁剪 | ADR-0002 条件 2 余项 | 规格 §5.4;T4 | ⏳ |":
        "| ✅ 闭合:M5 单 Task 命名空间构造性裁剪 + 终态失效;多 Team 隔离随 M6 per-task principal |",
    "Memory Service 接口 | ADR-0002 条件 5 余项 | 规格 §5.8;T8(接口面闭合) | ⏳ |":
        "| ✅ 接口面闭合:memory:task:<id> 承载任务上下文;真实跨 Task 场景随 M6 |",
    "双路径统一合同启用 | ADR-0002 条件 4 | 规格 §5.4;T3/T5(一致性测试进 CI) | ⏳ |":
        "| ✅ 闭合:统一执行体 + principal 来源标注 + 重放归因;ADR-0002 条件 4 双开解除 |",
    "forever scope 收紧 | M4-review §6.4 | 规格 §8-5;T3 | ⏳ |":
        "| ✅ 闭合:high-risk 恒 once、external 默认 ttl、forever 须审批卡片显式选择 |",
    "T6c 收紧(count 余量/幂等收据落盘) | M4-review §6.3 | 规格 §5.5;T1/T6 | ⏳ |":
        "| ✅ 闭合:t52/t53 跨重启实证(count 余量不回满、抑制不重放副作用) |",
    "投影重建延迟定量(ADR-0004 未决分歧 4) | 首个数值 | P-11;T9 回填 | ⏳ |":
        "| ✅ 首填:release p95≈0.95ms(1 万事件,门 1s,余量 ~1000×) |",
}
for k, v in table.items():
    if k in s:
        s = s.replace(k, v)
    else:
        # 表行形态带前缀,按尾部替换
        key = k.split(" | ", 1)[1]
        idx = s.find(key)
        assert idx > 0, f"table row missing: {key[:40]}"
        end = s.find("⏳ |", idx)
        s = s[:end] + v.strip("| ") + s[end + len("⏳ |"):]

s = s.replace("""**裁决(T10):⏳——预期(规格 §5.2/§5.4):8 项闭合后 ADR-0004 条件 6 正式""",
              """**裁决(T10):八项前置条件全部闭合——ADR-0004 条件 6 闭合;ADR-0002
条件 2/5 余项闭合、条件 4 双开解除,对外口径升级为「成立」(条件 1/3 已于
M4 实证)。预期(规格 §5.2/§5.4):8 项闭合后 ADR-0004 条件 6 正式""")
s = s.replace("- 本回看逐条裁决结果:⏳",
              "- 本回看逐条裁决结果:S9 部分采纳(verification 钩子消费落地,"
              "Liveness/Readiness 映射随 M7);S3 方向已实践(watchdog 检测面,"
              "完整对照随 M6);其余维持 proposed。")
s = s.replace("7. **CI 三平台确认**:推送后矩阵全绿(存量 134 + M5 增量):⏳",
              "7. **CI 三平台确认**:推送后矩阵全绿(188 项;R5/R6 镜像断言在列):"
              "推送 cc116d2 触发,本地全绿为门(⏳ 矩阵结果随 CI 运行确认)")
s = s.replace("核验)——⏳", "核验)——是:无 verified 核验不得 completed,unverified 一律 blocked 等裁定(t85/t86)。")
s = s.replace("2. 旧能力可用?(M1–M4 存量 134 项全绿;合同全 Minor 增发零破坏)——⏳",
              "2. 旧能力可用?是——M1–M4 存量全绿;合同全 Minor 增发零破坏;"
              "GT-01/02 回放绿(启动期系统事实按类型过滤,INV-3 不破)。")
s = s.replace("T6c 落盘重启不回满;幂等键续跑)——⏳",
              "T6c 落盘重启不回满;幂等键续跑)——是:t51 状态/epoch 不回退、"
              "t52 余量不复活、t53 抑制不重放、blocked 无自动出口、"
              "watchdog 同episode不重复通告。")
s = s.replace("observation.recorded;归因链含 task_id/operation_id)——⏳",
              "observation.recorded;归因链含 task_id/operation_id)——能:状态迁移"
              "带具体 reason_code,授权链 parent_hash 可上溯 bootstrap,"
              "watchdog 事实事件可区分触发者。")
s = s.replace("guard verified_completion)——⏳", "guard verified_completion)——是:t85/t86 门禁双向实证。")
s = s.replace("只增态与边)——⏳", "只增态与边)——是:三份新合同+状态机边镜像断言守门。")
s = s.replace("7. 推进还是退回?(M5 收官 → M6)——⏳",
              "7. 推进还是退回?推进——M5 收官(passed_with_conditions),进入 M6"
              "(Team、Delegate 和多 Agent 协作)。")

p.write_text(s, encoding="utf-8")
print("remaining ⏳:", s.count("⏳"))
