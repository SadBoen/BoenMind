# ADR-0008 架构即代码与外部实证验证

- 状态: accepted
- 日期: 2026-08-28
- 决策类型: 工具与流程裁决 + 验证结论
- 来源: 架构图结构化重构(基线 §15/§24)与 DeepWiki 对照验证(architecture/deepwiki-validation.md)

## 裁决

1. 基线全部架构图以 Structurizr C4 DSL 为唯一权威载体(`architecture/boenmind.c4`,经 structurizr-dsl 4.1.0 解析验证:66 元素 / 111 关系 / 11 视图);正文文字图降级为示意,拓扑变更先改模型、再改文字,不一致以模型为准。
2. L0-L5 分层与插件热替换设计经 Erlang/OTP、Kubernetes、VS Code 三真实系统对照验证成立(`architecture/deepwiki-validation.md`,C1-C8 逐条裁决:C7/C8 确认、C1-C6 部分确认、无偏差);单写者租约与验证期禁止真实外部副作用为 BoenMind 相对三系统的加强项,未发现先例反例。
3. 外部对照产生的修订建议 S1-S10(restart 类型字段、迁移回放测试入验收、升级停滞检测、draining 两步化、注册期 manifest 校验、懒启动扩展点、rest_for_one 级联重启、Wire API 按方向拆分、verification 三分法、Patch 级承认维护窗口)全部列为 **proposed**:逐条在对应里程碑回看(§19)时裁决采纳与否,不自动生效。

## 后果

- 架构复盘(§19-G)新增检查项:「架构模型是否已随正文同步更新」。
- S1-S10 的处理结论必须留痕(采纳则发新 ADR,否决则记录理由)。
