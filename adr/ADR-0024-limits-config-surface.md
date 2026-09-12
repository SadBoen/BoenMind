---
status: accepted
date: 2026-09-07
summary: ~40 项硬编码限制收敛单文件+安全钳制+env>文件>代码默认+LimitsCell 热生效;设置页全量可编辑;exec 默认 120s/上限 600s 对齐业界(2026-09-07)
supersedes: []
superseded_by: []
---

# ADR-0024: 运行时限制集中配置面(limits.json)

- 状态: Accepted(用户 2026-09-07 裁决:一批交付、全部可编辑、默认值一步到位最优)
- 日期: 2026-09-07
- 关联: ADR-0019(system.exec)、ADR-0021(fs.* 内置化)、ADR-0018(配置面先例 workspaces.json);调研依据 = `docs/agent-limits-timeout-comparison-20260907.md`(pi/pi_agent_rust/Hermes/DSH/ZCode 五家横评)与 `docs/runtime-limits-inventory-20260907.md`(本仓 40 项限制盘点;两报告均已随 2026-09-07 一次性报告清理移出仓,原文溯 git 史)
- 背景: VPS 实测 system.exec 60s 铁顶致 GitHub clone 必败,且模型自传 `timeout_ms` 被三层 min() 钳死无效;全仓超时/上限/熔断约 40 项全部硬编码,仅 `BOEN_TURN_TIMEOUT_SECS` 一个环境变量旋钮。五家对照结论:①「按任务大小动态算时间」五家皆无,业界标准 = 配置默认值 + 模型逐次申请 + 上限钳制,长任务走「转后台」逃生通道;②除 TS 版 pi 外全部有配置文件;③命令超时业界收敛值 = 默认 120s / 前台上限 600s / 后台不限时。

## 决策

### 1. 单文件集中配置

新增 `<数据目录>/config/limits.json`(私有管理文件,不入冻结合同),全部运行时限制收敛于此。规则:

- **缺文件 = 行为零变化**:serde 默认值 = 代码内默认;坏 JSON / 单键坏值 = 该键回退默认,**不拒启**。
- **每键加载期安全钳制**:钳制区间写在代码单点(`limits.rs`),管理面写值与手改文件同受其约束;越界值静默归界并在 GET 响应标注。
- **优先级:env > 文件 > 代码默认**(存量 `BOEN_TURN_TIMEOUT_SECS` 语义不变,启动时若 env 在场则覆写对应键并在管理面标注 source=env)。

### 2. 热生效:共享快照单元,不做 actor 回路

`LimitsCell(Arc<RwLock<Limits>>)` 挂 `RuntimeConfig`,bm-providers/bm-surface-http 各执行体与管理面持同一单元;所有消费点**读时取值**——exec 超时下一条命令生效、回合参数下一回合生效、流式上限下一条流生效,无需重启。限制是运行配置而非域状态,不走单写者命令面(McpHub 的共享 sink 为同款先例)。

### 3. 默认值一步到位(用户裁决「默认就不用改」)

命令执行:默认 60000→**120000ms**,模型可申请上限接活至 **600000ms**(前台硬顶与 capability 层 600s 钳制天花板对齐;超限走 ADR-0025 后台转轨而非报错)。其余默认保持现值——盘点已证其与业界收敛值一致。

### 4. 管理面与设置页

- 新端点 `GET/PUT /admin/limits`:GET 逐键返回 当前值/默认值/钳制区间/来源(default|file|env);PUT 校验→`bm_persist::atomic_write` 落盘→更新 Cell→响应生效时机。门户墙之后,不新增鉴权面。
- webapp 设置中心新增「限制与超时」页:**全部可编辑**(用户明示,自担误伤风险);编译期常量(前端看门狗等极少数)灰显只读并注明原因;提供单键与全局「恢复默认」。

## 后果

- 合同零变更(限制不入线协议);C4 拓扑不变(纯配置数据面)。
- `docs/runtime-limits-inventory-20260907.md` 盘点表(报告已移出仓,溯 git 史)自此有了「可调性」的落地形态;BACKLOG「exec 60s 铁顶」条目随本批闭合。
- 风险与对策:全量可编辑放大误伤面 → 服务端钳制兜底 + 每键大白话标签 + 恢复默认一键回滚。
