# 工具 A · code-architecture（Step 3A）独立报告

> 2026-08-17。硬排除 WIKI 功能。未读 B/C 结论。对象 = 架构文件 + 已实现骨架对照。不改库。

**不要对架构动大手术。** 脊梁成立。更合理的方案是主文档升一版做状态校准 + 交接瘦身 + 登记表补面，不是换内核、不是重写分层、不是合并 crate。

| 维度 | 分 | 一句话 |
|---|---|---|
| 架构合理性 | 8 | 三铁律、Port、转接器、权威分界与代码同方向 |
| 精简 | 5 | 主文档 145KB 兼设计+日记；章节乱序；交接仍是流水账 |
| 优美 | 6 | OS 同构漂亮；编号/版本/面数像多份未合并的稿 |
| 复用 | 8 | Port / 转接器 / ChatPane / DockLayout / bm-compat 用对了 |
| 完善 | 5 | 设计深、落地登记浅：MCP / Provider / 设置 / 皮肤 / 排队未闭环 |

---

## 1. Architecture map（A-1）

BoenMind 是寄生在宿主 OS 上的用户空间 Agent 运行时：内核保证插件能装、服务按 key 找、事件能记；聊天/记忆/网络/UI/厂商协议声称在核外面。

可执行切片：前端壳 → `bm-server` 组装根（Port + 门闩 + MCP）→ `bm-loop` → 消息写 SQLite `messages`，事件日志 sidecar（冻至 M3）。TS 插件走 `bm-compat`。

文档角色：`everything-is-plugin-architecture.md` = 宪法+日记；`HANDOFF_KERNEL_PHASE1.md` = 过期指针；`EXTENSION_POINTS_REGISTRY.md` = 防空货架但落后；设置/MCP 设计文未进地图。

缺口：§四·C 应用插件仍是宿主组件冒充 App；平台驱动 / 网络策略 / 把关链五事件仍是设计面。

## 2. What's working well（A-2～A-7）

- 铁律与 L9 依赖守卫可执行；§15.4 不换 dsh 仍成立。
- 协议+kernel+loop 远低于 1.5 万行预算；压缩/记忆/MCP/厂商在核外面。
- 服务面已从「建成未接线」变成「面在、实现在」；08-17 配置单锁已收。
- 宿主能力（ChatPane / dockview / 皮肤只改材质）在吃饭。
- §5.1 sidecar 诚实标注比再写十页溯源有用。
- 转接器原则与 `bm-mcp`/`bm-compat` 同构。

## 3. Concerns（按影响）

| ID | 级 | 问题 | 文档手术 |
|---|---|---|---|
| A-8 | 高 | 版本/面数/桌面壳/设置是否存在，活文档互撕 | 页眉+地图+设置文阶段表+HANDOFF 对齐登记表 |
| A-9 | 高 | HANDOFF 自称 4KB 实为 32KB 流水账，日期停 08-16 | 砍流水账；权威顺序：宪法 > 登记表 > HANDOFF > design/* |
| A-10 | 高 | 登记表无 mcp；credentials/scheduler 仍待接线 | 补面；拆「已注册」vs「插件 lookup」 |
| A-11 | 中 | 十一→十三→十五→十二附录；3.4 插在 3.6 后 | 附录改称文末附录；3.4 加锚，不搬表 |
| A-12 | 中 | MemoryPlugin ABC / 14 Port 愿望 / settings.json 三层 vs 代码 | §6.1 写 memory-file；§5.5 写 `[apps]` 单源 |
| A-13 | 中 | §15.1 未接线、enqueue 写成已有、聊天排队未登记 | 快照标注；排队=DE 输入策略≠ loop inbox |
| A-14 | 低 | HAL/把关链五事件未做——Simplicity 已刹 | 节首标「设计保留/未立项」，勿当 P0 |

## 4. Opportunities / 拒绝（A-15～A-19）

升 v0.26 状态校准（<2 页 diff）。借鉴清单可日后降附录。登记表当唯一计数器。

**拒绝**：换 dsh；M3 前收口双写；删挂点；拆 `bm-server` 多进程；重写 145KB 宪法。

## 5. Verdict（A-20～A-23）

无结构性运行时缺陷需要靠改架构来救。严重的是文档系统自相矛盾，会制造错误工程。

代码骨架略过工程（面先挂、第二实现未到，已接受）。架构文档过工程（日记化）+ 欠工程（当前态）。

建议顺序：登记表 → 主文档页眉/§四·B/§5/§6.1/§7.2 → README → 设置文删重复行 → HANDOFF 砍账 → 附录编号说明。
