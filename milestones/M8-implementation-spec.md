# M8 实现规格:首批真实 App 与发行质量(v1.0)

- 状态:冻结(2026-08-30,自冻结即生效——治理规则)
- 基线依据:§18-M8 八子项;通过条件:至少两个真实 App 使用同一套
  Runtime、Broker、Task 和日志机制;长任务可以回放和评估;关键副作用有
  执行收据;三平台完成端到端回归;历史会话不因发布和迁移损坏。
- App 形态裁决:ADR-0011(首批 App 以 MCP stdio server 接入,不新增内核形态)。
- 三平台口径:ADR-0009——CLI/TUI(本机)、Web UI(浏览器 + server)、
  Windows Tauri 壳(桌面);前端同源禁止分叉。

## 一、前置结算(承接 M7-review 遗留)

| 项 | 裁决 |
|---|---|
| lease 通道真实吞吐 | M8.4 压测内首测(Wiki 大页面写入走数据面) |
| S4 draining(两步摘除) | M8.5 热替换/备份场景实测后裁决;若压测不触则如实留档移交后续 |
| S5 quarantined 分表 | M8.5 以 open_resilient 损坏隔离路径出测试(机制 M2 已存在,补证伪测试) |
| D-M5-2 memory:user 授权面 | 不入 M8:首批 App 走 MCP 文件域,不触 memory:user;留档待多用户形态 |
| M6-review worker 自主 turn 环 | M8.3 多 Surface 协作内实现 worker 真实模型回合(真实通道) |
| M7-review 条件 1(实网稳定性复测) | **M8.4 硬条件**:长任务须在真实通道(gpt-5.6-luna)复测并留档 |
| M7-review 条件 4(用户显式取消能力调用) | M8.3 落地:capability.cancel 方法增发 + MCP notifications/cancelled 贯穿 |

## 二、裁决(实现即合同)

### S1 首批 App 形态(ADR-0011)
- Wiki App(M8.1):stdio MCP server(python 标准库),文件域真实持久
  (`<data>/apps/wiki/*.md`),工具集 page.read(只读)/page.write(外部
  副作用:真实写盘,出收据)/page.list(只读)。真实副作用 = 磁盘文件
  变更,执行收据 = 写后内容摘要 + 字节数,经 outbox 对账。
- Market App(M8.2):stdio MCP server,确定性领域——内嵌 fixture 行情
  (同查询恒同答,可回放可评估),工具 quote.get(只读)/
  portfolio.add(可逆,进程内账本)/portfolio.value(只读,纯计算)。
  确定性要求:fixture 版本随 App 版本钉死,重放逐字节同结果。
- 两 App 共用同一 Runtime/Broker/Task/日志机制(经 --mcp-config 安装,
  M7.7 显式配置 = 用户批准),不新增内核形态与旁路。

### S2 多 Surface 协作(M8.3)
- capability.cancel 方法增发(Minor:envelope method 枚举 + wire 镜像 +
  rpc 分发):语义取消——收据落 cancelled,迟到完成丢弃;MCP 侧经
  notifications/cancelled 通知 server(InProc 同步置位),传输层尽力终止。
- e2e:同一 Runtime 上 HTTP 建任务 → 第二连接(Web 形态)审批 →
  CLI 形态取消 → 收据 cancelled;三 Surface 同源审计链完整。

### S3 长任务压测(M8.4)
- 真实通道长任务(#[ignore] + BOEN_LIVE):Task 编排 N(=6)个 worker
  回合 × Wiki/MCP 工具调用,验收 = 全部终态、无挂起、副作用收据齐全、
  事件流可回放;一轮至多 1 次实网,失败不重试超 1 次。
- lease 首测:Wiki page.write 大载荷(≥64KB)数据面准入与吞吐采样留档。
- 产出事件流喂 M8.7 Judge 出评估报告(回放 + 评估闭环)。

### S4 迁移、备份与恢复(M8.5)
- 备份:SQLite 在线备份(VACUUM INTO)+ 事件日志快照拷贝,运行中可取。
- 恢复:副本打开 + 投影重建 + 状态一致性校验(会话/任务/收据行数与
  校验和比对);历史会话 resume 语义不损坏(基线通过条件)。
- 迁移演练:SCHEMA_VERSION v7→v8 增发(评估报告落库表),expand-contract
  既有纪律;旧库开启后自动迁移,数据零丢失断言。
- S5 quarantined:损坏库经 open_resilient 隔离 + 日志重建,证伪测试落档。

### S5 独立 Judge 与评估报告(M8.7)
- bm-judge crate:独立于核心回路的评估器——输入事件日志区间,输出
  评估报告(contract:evaluation/evaluation-report.v0_1,新增):
  确定性检查(INV 抽查:单终态、事件形状、副作用收据在位、延迟分桶、
  序号连续),逐条 verdict + 证据;同输入恒同报告(可复跑)。
- LLM 定性注解为可选层(#[ignore] 实网,不进报告 contract 必填字段)。
- Judge 独立性:只读事件日志与收据,不依赖 World 内存态。

### S6 数据保留期、用户删除与墓碑回放(M8.8)
- 用户删除:session/task 删除 → tombstone 落墓碑表(M2 既有机制);
  重放后删除不复活(墓碑回放验证);收据/事件保留(事实不篡改)。
- 保留期:execution log 按 retention_days 修剪(缺省 0=不修剪,配置面
  显式开启);事件日志不修剪(审计本体)。
- 发布/迁移损坏验证:M8.4 长任务日志 → 迁移到 v8 → 备份/恢复 →
  重放 + Judge 报告一致(三段串联端到端)。

### S7 三平台发布包(M8.6)
- CLI/TUI:cargo build --release 产物(bm-cli + boenmind-server),Windows
  本机出包;跨平台交叉编译不在阶段一(ADR-0009 口径)。
- Web UI v1:静态单页(bm-surface-http GET / 托管,与 Tauri 同源):
  会话列表/发送回合/审批列表裁决/任务看板,Wire + SSE 直连,零构建链
  (纯 HTML/JS,免 node 依赖)。
- Windows Tauri 壳:复用同一静态页(caller = tauri 内嵌 WebView 加载
  本地 server 地址);tauri-cli 经 cargo install 提供,构建产物留档;
  若构建链不可用,交付工程骨架 + 复现命令并如实留档(M8-review)。
- E2E 回归:CLI 形态(m3 既有)/HTTP 形态(m4/m7 既有)/Web UI 服务
  冒烟 + 静态资源断言;三形态同源 Runtime。

## 三、边界(本里程碑不做)

1. macOS/Linux 桌面壳(ADR-0009 非目标);移动端。
2. 多用户与账号体系(memory:user 随之延后)。
3. Web UI 富交互框架(零构建链单页为限,框架化随后续)。
4. 实时行情等外部数据源(Market App 用确定性 fixture,外部 API 不入)。
5. 事件日志修剪(审计本体保留;仅 execution log 可修剪)。

## 四、任务分解

| 任务 | 内容 | 验收 |
|---|---|---|
| T0 | 合同增发:capability.cancel 方法、evaluation-report.v0_1、SCHEMA v8;ADR-0011;本规格 | validate.py 全绿 |
| T1 | Wiki/Market App servers + 安装清单样例 + e2e(t110-t113) | 两 App 全链路绿 |
| T2 | capability.cancel + 多 Surface 协作 e2e(t114-t115) | 取消语义 + 三 Surface 审计链绿 |
| T3 | bm-judge + 评估报告 + 长任务压测(真实通道)t116 | 压测回放 + 报告一致 |
| T4 | 备份/恢复/迁移 v8 + 保留期/删除/墓碑(t117-t119) | 历史会话不损坏断言绿 |
| T5 | Web UI v1 + Tauri 壳 + release 出包 + 三平台 e2e(t120) | 三形态回归绿 |
| T6 | GT-06(评估报告轨迹)、perf 记录⑥、M8-review、AGENTS.md、tag m8-apps-release、push | §19 回看门 |

## 五、测试编号(续 M7:t105-t109)

- t110 Wiki App:page.write 真实写盘 + 收据 + outbox 对账
- t111 Wiki App:page.read/list 发现与直通(只读)
- t112 Market App:确定性(同查询两次逐字节同结果)+ portfolio 可逆路径
- t113 双 App 同 Runtime 共存与能力隔离(scopes 域)
- t114 capability.cancel:运行中取消 → 收据 cancelled → 迟到完成丢弃
- t115 多 Surface 协作:HTTP 建 → 第二连接批 → CLI 取消,审计链同源
- t116 长任务压测(#[ignore] BOEN_LIVE):真实通道全终态 + 回放 + Judge
- t117 Judge 确定性:同事件区间两次评估报告逐字节一致;副作用收据检查
- t118 备份/恢复:运行中备份 → 副本恢复 → 会话/任务/收据一致
- t119 迁移 v7→v8 + 删除墓碑回放(删除不复活)+ 保留期修剪
- t120 三平台:Web UI 服务冒烟 + 静态资源 + release 产物存在性
- 实网:gpt-5.6-luna 长任务一次(#[ignore],env 门控)
