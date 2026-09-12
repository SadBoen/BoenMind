---
status: accepted
date: 2026-09-12
summary: 模型路由关注点上移为 core 端口 ModelRouter,surface 去具体依赖;并更正早先架构报告中"持久化 schema 应在 persist"的假阳性判断
supersedes: []
superseded_by: []
---

# ADR-0042: 模型路由端口上移(去具体依赖)与持久化 schema 归属更正

- 关联: ADR-0005(万物皆插件)、ADR-0036(执行分道)、ADR-0041(插件身份契约)
- 背景: 一份基于**无 Cargo.toml 的源码快照**的架构评审报告提出若干"分层越界"欠账。对真实仓逐条核实后,发现其中一条为**假阳性**,另一条为真问题但需重新定位。本 ADR 记录核实结论与据此的改动,作为"动架构前必须对真仓核实"的方法论锚点。

## 决策

1. **更正假阳性:持久化行结构留在 `bm-core` 是**正确设计,不迁移**。**
   核实:`WorldRows`/`ApprovalRow`/`GrantRow`/`TaskRow`/`CapabilityRow`/`RecoveryReport`/
   `StoreError` 是 **`EventStore` 端口契约的组成部分**——trait 方法直接收发它们
   (`load_rows() -> StoreResult<WorldRows>`、`save_approval(ApprovalRow<'_>)` 等)。
   把它们搬到 `bm-persist` 会使 core **反向依赖** persist(方向倒置、更糟)。
   当前形态即端口-适配器(六边形):**端口连同其数据 DTO 在 core,适配器在 persist 实现**。
   原先"core 背着持久化 schema"的判断错误,予以撤销。
   (遗留:同文件内的文件工具 `atomic_write`/`filter_lines_atomic` 是**工具**而非端口契约,
   是否下沉可另行评估,不在本条范围。)

2. **更正真问题定位:越界不在"依赖 bm-providers",而在缺少路由端口。**
   核实:`bm-surface-http` 直接持 `Arc<bm_providers::routing::RoutingConnector>`
   (`lib.rs:62,122`、`webadmin/mod.rs:83`),调用 `known_models()`/`contains()`/
   `replace_table()`——这三个是**路由管理**方法,**不在 `ModelConnector` 端口上**
   (端口只有 invoke/invoke_stream/provider)。即"模型路由表"这一关注点没有端口表达,
   故 surface 只能抓具体类型。

3. **在 `bm-core::ports` 新增 `ModelRouter` 端口**:`known_models()` / `contains(model_id)` /
   `replace_table(HashMap<String, Arc<dyn ModelConnector>>)`。`RoutingConnector` 实现它
   (固有方法保留,端口实现转调)。surface 的 `model_routes` 字段改为
   `Option<Arc<dyn bm_core::ports::ModelRouter>>`(3 处),不再引用具体类型。

## 后果

- `bm-surface-http` 源码中 `RoutingConnector` 引用归零;路由读/写经端口,实现可替换。
- **零行为变更**:497 测试全绿(新增 `model_router_port_is_usable_as_dyn` 经 `dyn` 断言
  known_models/contains/replace_table 行为)。
- surface 对 `bm_providers` 仍有 webadmin 面的依赖(MCP hub、JobTable、SkillScriptManager、
  OpenAiConnector)——那是**管理面本就该管 provider**,不属越层,不在本条范围。
- **方法论留档**:早先架构报告基于无 Cargo.toml 的源码快照,存在假阳性(本条 1)。
  任何架构改动前**必须对真实仓核实**,不得据快照直接动手。后续"core 拆胖""surface 服务层"
  等条目同样需先核实用途再定,路线图见 `.work/ROADMAP.md`。
