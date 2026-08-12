# Refine Suggest —— 自我改进建议采集插件

借鉴 Prime Agent `/refine` 的心智（代理用执行经验反哺知识库），但采用**宿主审批模式**：
代理只提交建议，用户审批后才生效——避免把坏经验 refine 进知识库。

## 工作方式

1. 代理完成任务后，若发现某 skill 的 description 或系统提示词存在误导/不准确/明显可改进之处，
   调用 `submit_refinement_suggestions` 提交结构化建议（target/quote/suggested/reason）；
2. bm-server 在工具调用事件流中截获参数，写入 `refinement_suggestions` 表（status=pending）；
3. 用户在设置页「改进建议」中审批：
   - **批准** `skill:<id>` → 修改 `~/.boenmind/skills/<id>/SKILL.md` 的 frontmatter description
     （quote→suggested 替换，不匹配则追加），改前备份 `.bak-<ts>` 可一键还原；
   - **批准** `system_prompt` → 追加到配置 `custom_system_prompt`（随系统提示词注入）；
   - **拒绝** → 丢弃。

## 设计要点

- 插件本体是"记录桩"：不落任何状态、不直接生效（生效由宿主完成）；
- 幂等：工具无副作用，宿主截获失败不阻塞对话；
- 与 `/refine` 的差异是有意为之：审批权始终在用户/宿主（上游 Factorio 演示曾因代理自改手册把作弊经验 refine 进知识库）。

## 文件

- `extension.json`：pi 扩展清单（capabilities: tool）
- `index.ts`：工具注册（QuickJS 沙箱内运行）
