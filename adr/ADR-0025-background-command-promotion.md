---
status: accepted
date: 2026-09-07
summary: exec 增 run_in_background+超限自动转轨(Hermes 式);system.job_output 轮询收取(DSH 式);回合 prompt 注入作业摘要;不做主动推注入(2026-09-07)
supersedes: []
superseded_by: []
---

# ADR-0025: 长命令后台转轨(run_in_background 与超限自动转轨)

- 状态: Accepted(用户 2026-09-07 裁决「一批全含后台转轨」)
- 日期: 2026-09-07
- 关联: ADR-0019(system.exec 异步管线)、ADR-0024(limits 配置面,exec_max_ms 为转轨阈值);对标 = Hermes 超前台 600s 自动转后台+完成通知 / DSH `run_in_background` 无超时+job_output 收取 / ZCode run_in_background / pi_agent_rust 后台任务
- 背景: 240 秒能跑完的 clone 在 60s 铁顶下必死;即便上限提到 600s,冷编译/大仓库下载仍会超。四家先例一致:超长任务的出路不是放大前台超时,而是转出前台生命周期。

## 决策

### 1. 显式后台 + 超限自动转轨(双通道)

`system.exec` 入参 schema 增 `run_in_background: bool`(模型显式选择);同时 **`timeout_ms` 超过 `limits.exec_max_ms` 的前台请求不再报错,自动转轨**(Hermes 式拒绝避免):立即回执 `{backgrounded: true, job_id, log_path, note}`,note 提示模型用 `system.job_output` 收取、勿盲目重跑。

### 2. 执行与产出

后台进程脱离回合生命周期(tokio 独立任务,kill_on_drop 改为显式守护),stdout/stderr 合并追加写 `<数据目录>/jobs/<op_id>.log`;进程退出码与耗时记入作业台账。台账 = 进程内表 + 落盘日志,保留上限可配(默认 50 作业 / 100MB,LRU 清理,limits.json 可调)。

### 3. 收取:模型轮询,不做主动推注入

新**同步直调能力 `system.job_output`**(免审批,读语义):入参 `job_id` + 可选 `wait_ms`(钳 ≤60s)→ 状态(running|succeeded|failed)/输出尾部/退出码,可反复调用(对标 DSH job_output 的 wait+反复收)。每回合 system prompt 追加在跑/近期完成作业摘要(对标 ADR-0018 workspace 注入先例),模型因此知道该收哪个 job。

**不做**(v1):作业完成主动开新回合推送(Hermes notify 式)——自动开回合触碰回合语义与审批边界,留待后续裁决;轨迹可见性由「模型轮询 job_output 的 tool_call/tool_result 天然入 context-log」+ 对话区 Ticker 展示在跑作业兜底。

### 4. 生命周期边界

- 转轨后原 operation 即以回执终态,审批/幂等语义已在转轨前完成(命令仍走审批卡)。
- 服务器重启:后台进程随宿主消亡,日志留存;重启后的 job_id 查询如实报「已随重启丢失」。跨重启作业接续不在 v1。
- v1 不提供后台作业取消(原 op 已终态,现有取消通道不可达);列为候选。

## 后果

- 合同零变更:exec schema 是能力 manifest(运行时),`system.job_output` 走既有直调注册面,不动线协议。
- VPS 场景 60s/600s 顶之上的长任务(clone/冷编译/大下载)首次有正路;配合 ADR-0024,默认值+转轨开箱即用。
