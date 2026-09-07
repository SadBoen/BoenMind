# W10 限制配置面 + 长命令后台转轨 实现规格

来源:2026-09-07 用户实测 VPS exec 60s 铁顶(clone 必败)+ 追问「还有多少隐形限制」;五家横评
(pi/pi_agent_rust/Hermes/DSH/ZCode,报告 `docs/agent-limits-timeout-comparison-20260907.md`)后用户
三裁决:①一批全做(报告+配置面+设置页+后台转轨)②设置页全部可编辑 ③默认值一步到位最优。
治理:ADR-0024(limits 配置面)+ ADR-0025(后台转轨)。

## 一、limits 配置面(ADR-0024)

1. `bm-core/src/limits.rs`:`Limits` 结构(约 40 键,覆盖盘点全部运行时项)+ `LimitsCell(Arc<RwLock<Limits>>)`
   + 加载(缺文件/坏值回退默认)+ 每键安全钳制 + 原子写(`bm_persist::atomic_write`)。
2. `RuntimeConfig` 增 `limits: LimitsCell`;server 启动装配(env BOEN_TURN_TIMEOUT_SECS 在场则覆写并记来源)。
3. 接线消费点(全部改读 Cell):
   - bm-core:`spawn.rs`(工具轮等待 60/300s、5 次熔断阈值+窗口、模型重试次数)、`history.rs`(20 轮/24K)、
     `watchdog.rs`(停滞 15min/硬顶 24h/节拍)、`audit.rs`(16KB)、`runtime.rs`(turn_timeout 读 Cell,
     env 已启动期折算);
   - bm-providers:`system_exec.rs`(默认/上限/输出截断)、`fs_tools/ops.rs`(读写 16MB/搜索 80·500/输出
     16K/1MB 跳过/万行)、`mcp.rs`(默认工具超时/远程 60s/重生窗口与上限)、`skill_wasm.rs`(默认 10s);
   - bm-surface-http:`openai_compat.rs`(流式 900s/非流式 180s/keepalive 10s)、`webadmin.rs`(下载
     256MB/5000 条、预览 512KB、删除 100、浏览 1000)、`portal.rs`(5 次/15min 锁定、Cookie 30 天,带下限)、
     `about.rs`(检查更新 20s/下载 600s)。
4. exec manifest:`timeout_ms` 静态提为 600000(=钳制天花板),行为由 Cell 动态钳;description 去 Now 硬数、
   改「默认 120 秒、最长 10 分钟,管理端可调」量级表述 + `run_in_background` 说明。
5. 端点:`GET /admin/limits`(值/默认/区间/来源)、`PUT /admin/limits`(钳制→原子写→热生效)、
   `GET /admin/jobs`(后台作业列表)。

## 二、后台转轨(ADR-0025)

1. `bm-providers/src/jobs.rs`:`JobTable`(进程内台账+`<data>/jobs/<op_id>.log` 落盘+LRU 保留上限)+
   `JobBoard` 端口实现(摘要注入)。
2. `system.exec`:schema 增 `run_in_background`;`timeout_ms > exec_max_ms` 自动转轨;立即回执
   `{backgrounded, job_id, log_path, note}`;后台进程独立 tokio 任务,退出码/耗时入账。
3. 新直调能力 `system.job_output`(免审批):`job_id`+`wait_ms`(钳 ≤60s)→ status/output_tail/exit_code。
4. 回合 system prompt 追加作业摘要(`RuntimeConfig.job_board: Option<Arc<dyn JobBoard>>` 端口,bm-core
   不反向依赖 bm-providers);不做完成主动推注入(ADR-0025 §3)。

## 三、前端(webapp)

1. 设置中心新增「限制与超时」页:全部可编辑(编译期项灰显注明),分组=五类盘点,每键大白话标签+区间
   提示+单项恢复默认,底部全局恢复;env 覆盖徽标;风格走主题令牌/圆角层级/lucide(用户风格标准)。
2. 对话区 Ticker 增在跑后台作业计数(`/admin/jobs` 并入既有轮询);轨迹页自然可见 job_output 收取链
   (context-log 既有事件流,零改动)。

## 四、验收(P0)

1. 单测:limits 缺文件零变化/坏值回退/越界归界;exec 按 limits 生效(短 sleep 命令验证 120s 默认与
   timeout_ms 生效);超限自动转轨回执;job_output 轮询到终态;各消费点读 Cell 热生效。
2. `cargo test --workspace` 全绿;`python boenmind-contracts/scripts/validate.py` 全绿(合同零改动);
   前端 eslint/tsc/build 绿。
3. 真实浏览器手测(铁律):设置页改 exec 上限→对话让模型跑 sleep 验证生效;后台转轨实测(长 sleep 自动
   转轨+job_output 收取+Ticker 计数);截图留档 `runtime/shots-w10-*/`。
4. 收尾:BACKLOG 闭合「exec 60s 铁顶」;PLAYBOOK §4 补 limits 说明;HISTORY 登记。

## 不做

- 按任务大小动态估算超时(五家先例皆无);完成主动推注入(ADR-0025 §3);后台作业取消与跨重启接续(候选);
  合同变更(零)。
