---
status: proposed
date: 2026-09-13
summary: surface 领域逻辑归属提案——config_store/rebuild_routes/providers CRUD 是否上移内核端口(仅提案,不动码)
supersedes: []
superseded_by: []
---
# ADR-0056: surface 领域逻辑归属(提案,待拍板)

- 关联: ADR-0044(surface 服务层边界核实与巨型 handler 拆分)、ADR-0042(模型路由端口上移)、ADR-0046(surface→providers 依赖反转)
- 状态: **proposed**——本文只提出候选与取证,不落代码,待用户拍板后再转 accepted 并实施。
- 背景: ADR-0044 核实了 surface 的**依赖面**已正确反转(无具体 provider 类型,编译期证明),并裁定了巨型 handler 拆分。但 surface 仍是「依赖面薄、**逻辑面厚**」:它自持若干领域策略,这些策略本可归内核端口,使多界面复用、与 Broker 审计同侧、且可独立单测。

## 候选清单(取证于当前代码)

1. **`config_store.rs`(447 行)**:file > env > 内置默认的合并语义 + 字段校验(valid_base_url/valid_model_key/valid_window_tokens)+ `effective_model` 装配。当前 `providers.rs` 复用其校验谓词——已是跨模块共享,却仍留在 surface。
2. **`webadmin/providers.rs::rebuild_routes`(250-294 行)**:providers.json → 路由表构建 + 首现去重 + secret_ref 派生 + 密钥播种 + 脱敏登记。属「模型路由策略」,而 `ModelRouter` 端口只暴露 `replace_table`/`build_connector` 两个原语——编排留在 surface。
3. **providers CRUD + 历史(墓碑)**:providers.json 的增删改查与历史恢复。
4. **`webadmin/mcp/*`(约 1200 行)MCP 生命周期**:配置/墓碑/播种/扫描。ADR-0023/0035 已把信任链与资源上限收口,但装载编排仍在 surface。

## 取证(为何是「关注点归属」而非「越层」)

- surface 主依赖面只有 `bm-core`/`bm-contract`(`Cargo.toml` 编译期证明),**无违反依赖方向**。
- 上述逻辑均经端口访问(`ModelRouter`/`SecretStore`/`McpAdmin`/`SkillHost`),无具体类型泄漏。
- 故这不是分层错误,而是**同一领域逻辑只服务 HTTP 一个界面**、无法被 CLI/TUI 复用、无法与内核单测同侧的问题。

## 候选处置(三选一,实施前须逐一评估)

- **A. 上移内核端口**:为 provider 库/模型配置抽端口 + 内核侧编排(如 `ModelConfig` 端口承载合并语义)。成本中,收益:多界面复用。
- **B. 保留 surface**:承认阶段一单界面,收口为 surface 内共享模块(如 `config` 子模块),不引入内核概念。成本低。
- **C. 部分上移**:仅把跨模块共享的**校验/合并语义**(候选 1)上移内核,CRUD 编排留 surface。

## 后果

- 未拍板前**零代码变更**;本文登记候选与取证,避免后续评审重复发现同一问题。
- 若采纳 A/C,须发实施 ADR 并走合同演进流程(surface 领域逻辑上移涉及 `ModelRouter` 端口扩容)。

## 待决问题

- 阶段一单用户单界面下,「多界面复用」的收益是否已发生?(无则应选 B 或 C,避免为不存在的消费者过度设计——与 ADR-0005 §6 双重门槛一致。)
- providers.json 的合并语义是否属「内核可信配置」范畴,还是纯 HTTP 管理面便利?
