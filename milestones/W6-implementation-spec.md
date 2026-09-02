# W6 实现规格:模型改名 + 常用清单 + 对话级模型选择/热切换/持久化

日期:2026-09-02 · 来源:用户三点需求(原话见 §1) · 状态:**已交付(当日,全验收门通过)**

## 1. 需求(用户原话)

1. 设置,模型提供商,改为「模型」;
2. 连通并拉取模型后(如 33 个),用户只勾选其中三四个常用;**不是设默认的概念**;
   显示「清单共 33 个,常用设置为 X、Y、Z」;
3. 聊天输入框允许选模型;选定后持久化(刷新/开新对话不变);**对话中途任意时刻可切,
   下一条消息即生效,不需要新开对话**。(动因:VPS 正式安装后默认落在 mock 测试模型。)

## 2. 方案(总)

- **常用清单**(静态数据):providers.json 每 provider 增 `modelsCommon: string[]`
  (⊂ models;只增不破,配置文件非冻结合同)。设置页「模型」里多选勾选,卡片显示
  「清单共 N 个 · 常用:A、B、C」。
- **对话选模型**(运行期):前端每条消息把所选模型放进 OpenAI 兼容 body.model
  (localStorage `bm_active_model` 持久化;未选 = "auto" = 服务器默认,现状不破)。
- **热切换**:合同 **Minor** 一笔——`agent.send_input` 请求增可选字段
  `model_override`(turn.rs 降级链整体替换为 `[override]`,工具轮/重试同回合仍用
  同一模型);会话新建时 body.model 烤入 `AgentSpec.model_chain[0]`。
- **多网关路由**:bm-providers 新增 `RoutingConnector`(按 `req.model_id` 查
  model_id→连接器表分发;未命中回落默认连接器;**必须覆写 invoke_stream 保真流式**)。
  World/RuntimeConfig 零改动(单连接器插槽装的就是路由器)。
- **凭据**:providers.json 每个模型启动/配置变更时向加密密钥库播种
  `secret:model.<model_id>`(缺则种,INV-5 不破——明文仍只落盘 providers.json)。
- **默认模型语义不变**:config/model.json「设为当前」= 服务器默认(重启生效),
  与「常用清单」是两个正交概念(用户明确要求不是默认概念)。
- **VPS mock 根因**:未配 provider 时默认连接器是 mock;W6 后用户在设置页加
  provider+勾常用 → 免重启即可在输入框选到真模型(body.model 命中路由表)。

## 3. 改动清单

| 层 | 文件 | 内容 |
|---|---|---|
| 合同 Minor | wire.rs `SendInputParams` + wire/agent.v0_1.schema.json | 可选 `model_override`(缺省不出字段,旧载荷不破) |
| 合同 Minor | boenmind-contracts validate 载荷同步 | validate.py 全绿 |
| bm-providers | src/routing.rs 新增 | RoutingConnector(表分发/回落/流式透传)+单测 |
| bm-core | turn.rs spawn_turn + handlers.rs | 增 override 参数;审计 json 用实际首模型 |
| bm-surface-http | openai_compat.rs | body.model 校验(表非空且未命中→400)+新会话烤链+send_input 带覆盖 |
| bm-surface-http | webadmin.rs | providers CRUD 存取 modelsCommon(校验 ⊂ models)+写后重建路由表+播种;AdminConfig 增 model_routes/secrets 已有 |
| bm-runtime | boenmind-server.rs | 启动装配 RoutingConnector(默认=现行为)+全 provider 模型入表+播种;AppState 传 Some |
| 前端 | SettingsPage/ProvidersPage | 导航与标题改「模型」;常用多选勾选;卡片显示「清单共 N 个 · 常用:…」 |
| 前端 | thread.tsx/runtime.tsx | 输入框模型下拉(常用并集,持久化 localStorage,中途热切不重开会话);body.model 随每条消息 |

## 4. 语义细节

- body.model = "auto"/缺省 → 服务器默认(现状);未知名且路由表非空 → 400
  「模型不在已配置清单」(防静默落 mock/错网关);路由表空(纯 mock 开发态)→ 不校验。
- 未知名且路由表空 → 放行默认(mock),W1 测试与开发态不破。
- provider health(熔断)按路由器整体一个桶(现状口径),已知简化,P3 演进项。
- 角色切换仍重开会话(system_prompt 烤入);**模型切换不重开**(override 每回合携带)。

## 5. 验收门(全部须实测)

1. 设置导航与页标题显示「模型」;provider 卡片显示「清单共 N 个 · 常用:…」;
2. 编辑页可勾选常用(⊆清单),保存后 list 回读一致,providers.json 落盘 modelsCommon;
3. 输入框出现模型下拉:选项=各 provider 常用并集;选择写入 localStorage;
   刷新页面/新建对话后选择保持;
4. **热切换**:同一会话第 1 条消息用模型 A(/admin/context 快照 model_id=A),
   中途切到 B 后第 2 条消息 model_id=B,无需新开会话;
5. 未知名模型 → 400 报错文案可见;不选(默认)→ 行为与 W1 全同;
6. 后端:cargo test 全绿(含新增路由/覆盖/400 用例);validate.py 全绿;
7. 真实网关实测(本机 OpenCode Go):真模型切换生效(快照证据截图 shots-w6/)。
