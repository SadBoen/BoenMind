# W4 实现规格:角色与对话工具闭环

- 序列:WEBUI W4(前置:W2 管理面/W3 主题/MCP 搜索插件均已收官)
- 状态:已实现交付(2026-09-02;核心 commit b2fc6a8+合同 Minor 40988a7,反馈轮 8cb2316;280+ 测试全绿)——§4 验收门证据待回填=W4b 收尾项(见 BACKLOG §3)
- 触发:用户实测「问模型有没有搜索工具,它说没有」→ 对话工具闭环缺失;用户三项裁定与架构查证(基线 §11.3 授权公式/§4.1 Skill/实现原则「不改内核」)全部对上

## 1. 用户裁定(本轮生效)

1. **联网搜索=只读操作,默认开启**(免审批直通)——落法:MCP 工具 manifest 标
   `readOnlyHint` → approval=not-required(已落地,mcp-servers 仓);
2. **角色定义**:每个角色可定义独立 prompt、允许的工具/Skill/MCP;工具与
   Skill、MCP 均有公共授权(Task/用户层)与私有授权(角色层)——原始架构
   依据:基线 §11.3 授权公式(App 能力 ∩ Task 授权 ∩ 成员角色授权)+
   ADR-0002(Grant 物化,默认拒绝/可撤销/重启可恢复)+ §4.1 Skill=
   提示模板+allowed_capabilities 清单(数据非执行体);
3. **万物皆件**:对话工具挂载不做旁路——能力注册表(已支持运行期追加)+
   turn 循环注入,合同 tools 字段启用(M1 预留占位,maxItems 0→N)。

## 2. 技术设计(本轮范围 = 对话工具闭环核心 + 默认角色;多角色/Skill 挂载为 W4b)

### 2.1 合同扩展(Minor,字段/枚举值只增)

- `connector.rs`:
  - `FinishReason` 加 `ToolCalls => "tool_calls"`;
  - `InvokeResponse::Completed` 加 `tool_calls: Vec<ToolCallPayload>`(默认空,
    序列化非空才出);
  - `Role` 加 `Tool => "tool"`(工具结果回喂消息);
  - `AgentSpec` 加 `system_prompt: Option<String>`(角色 prompt,session 级);
  - `tools` 注释更新:maxItems 0→16(schema 同步)。
- `boenmind-contracts` JSON schema 同步(connector/model 相关),
  validate.py 全绿;既有 golden traces 不受影响(默认不启用 tools)。

### 2.2 runtime(bm-core)

- `CapabilityRegistry` 增枚举方法:列出全部 `approval=not-required` 能力的
  (capability, description, input_schema)——对话工具白名单=免审批直通集,
  架构口径「默认拒绝」下只暴露免审批集,危险能力不进对话;
- `spawn_turn`:
  1. 进任务前(world 可用)枚举直通工具 → OpenAI tools 格式;
  2. `AgentSpec.system_prompt` 非空 → messages 头部插 System 消息;
  3. 响应 `tool_calls` 非空 → 逐个经 `Cmd::CapabilityCall` 回核心循环执行
     (走完整 Broker 裁决/幂等/审计管道;直通即执行)→ 结果作为 Tool 消息
     追加 → 重新调模型;**上限 5 轮**防循环;每轮发 ProviderDelta 标注
     「[调用 <工具>]」让用户可见;
  4. 非 tool_calls 回复 = 现状收尾(零变化)。

### 2.3 connector(bm-providers openai_http)

- 请求体:`req.tools` 非空 → 透传 OpenAI `tools` + `tool_choice:"auto"`;
- 非流式:解析 `message.tool_calls`;
- 流式:聚合 `delta.tool_calls` 分片(id/name/arguments 按 index 拼)→
  完成时并入响应(on_delta 只走文本,工具分片不过 SSE)。

### 2.4 server/前端

- `GET/PUT /admin/roles`(config/roles.json,ADR-0012 口径):默认角色
  「助理」= system_prompt(默认空)+ 工具策略(v0 固定「全部直通工具」);
- 设置页新增「角色」导航:角色名/system prompt 编辑/保存 + 直通工具清单只读
  展示;会话侧 v0 全局套用默认角色(多角色/会话级选择 = W4b)。

## 3. 已知边界(W4b 候选)

- 多角色与会话级角色选择;Skill 挂载(合同 Skill 实体未建);
- 非直通工具的对话内审批联动(当前不暴露进对话);
- mock 模型不支持 tools(回归走文本路径;工具路径用真实网关实测)。

## 4. 验收门(浏览器 MCP 实测)

1. 对话问「有没有搜索工具/搜一下 X」→ 模型声明有工具并实际调用
   `web_search_lite` → 真实搜索结果进入回复;
2. 直通语义:调用过程无审批弹窗(只读直通),审计有 capability.invoked;
3. 回归:不启用工具路径(普通问答/banana)零退化;280 测试全绿;
4. 设置页「角色」可编辑 prompt 并保存(重启保持)。

### 4.1 实测验收记录回填(2026-09-02 W4/W4b 真实实测)

- **验收结果**: 全部通过 (Passed)。
- **证据记录**:
  1. **直通工具闭环**: 对话提问「搜一下最近科技资讯」，模型自发调用 `web_search` / `web_search_lite`，结果以 `role: tool` 回喂至模型并生成最终回复；上下文透视面板中核验到两轮往返及完整工具调用快照，未触发任何中断或弹窗；
  2. **非直通工具审批流 (W4b)**: 触发写操作及命令执行时，对话区以 `BM_APPROVAL` 协议上屏审批卡片，用户在页面点击「批准」或「拒绝」后，`/admin/approvals/{id}/respond` 成功裁决并由 turn.rs 唤醒后续回合，实测代码生成/写文件全闭环（见截图 `shots-w5-context/06.png`、`shots-w5-context/07.png`）；
  3. **角色与技能挂载**: 角色管理页编辑「代码架构师」与「押韵诗人」，挂载技能后在 System Prompt 中实时生效，问答语气及格式与设定完全吻合（见截图 `shots-w5-context/04.png`、`shots-w5-context/08.png`）；
  4. **工程门禁**: 冒烟测试套件 `npm run test:smoke` 9 项全绿，全仓 316 套 Rust 测试无一失败，合同校验器 validate.py 全绿。

