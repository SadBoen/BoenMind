---
status: accepted
date: 2026-09-13
summary: manifest 增发 wire_name(Minor)+ 异步分道删除 provider 命名前缀回退——内核不再认识具体能力名
supersedes: [ADR-0036]
superseded_by: []
---
# ADR-0054: 面向模型的工具名入合同与执行分道去前缀

- 关联: ADR-0036(execution_mode 声明为真源,本 ADR 收其"未声明回退命名约定"的尾巴)、ADR-0022(description 作为面向模型的展示属性先例)、ADR-0005 §6(可选字段 Minor)、ADR-0050(能力执行面收口)
- 背景: 内核 `bm-core` 仍认识两处**具体能力名**:`spawn.rs::wire_name_of` 硬编码短名表(`fs.read→read`/`fs.search→rgrep`/`system.exec→powershell|bash`),`registry.rs::provider_is_async` 按 `mcp.`/`skill.`/`.async` 前缀猜异步分道。前者使新增能力须改内核,后者是 ADR-0036 声明化后遗留的兼容回退。二者皆属"内核认识能力名"的耦合,与"万物皆插件"相悖。

## 决策

1. **`wire_name` 入 capability manifest 合同(Minor 增发)**。面向模型的工具名(`function.name` 字符集 `^[a-zA-Z0-9_-]{1,64}$`)由各 provider 在 manifest 声明,与既有 `description`(ADR-0022 面向模型的展示属性)同类同处。缺省 = 消费方按能力名点转单下划线兜底;声明名被占则回落默认名保唯一性。仅影响工具清单展示与模型亲和,**不改能力名、审批语义、Broker 裁决**。
2. **平台差异留在 provider**。`system.exec` 的 `powershell`/`bash` 分支由 `bm-providers` 按 `cfg!(windows)` 声明——provider 本就是平台相关代码,内核不再持有 `system.exec→powershell` 映射。
3. **删除异步分道的 provider 命名前缀回退**。`provider_is_async` 删除,`mark_async_for` 仅回读 `manifest.execution_mode` 声明;未声明 = sync(与 `register` 既有口径一致)。生产 manifest 已全部显式声明(ADR-0036 守卫测试),前缀回退只服务旧 manifest 与测试——按 ADR-0036 的原意消灭。

## 后果

- 内核 `bm-core` 零具体能力名:`wire_name_of` 只做"声明优先 + 点转单下划线"的通用转义,`mark_async_for` 只读合同字段。新增能力族不再需要改内核(兑现 decisions.md 条目 11 的方向,issue #67)。
- 合同库 `manifest.v0_1.schema.json` + Rust 镜像 `CapabilityManifest`/`ManifestSpec` 增可选字段;`tests/sync.rs` 补正/反例(含点号必拒)。
- 守卫测试:`bm-providers::builtin::builtin_tools_all_declare_wire_name` 锁死内置五件的声明(不声明则短名退化,原隐式约定现成显式契约)。
- 契约消费方必须忽略不认识的字段(合同 README 既有纪律)——`wire_name` 对旧消费方无害。
- **行为保持**:生产工具名与分道结果不变(声明值 = 原硬编码值);测试套件全绿;`validate.py` 全绿。
- 遗留:内核仍认识 `model.invoke` 能力名(内核私有,ADR-0020/0045 冻结清单,不在本次范围)。
