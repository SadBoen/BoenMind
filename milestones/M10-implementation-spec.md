# M10 实现规格——Web Surface 配置管理与对话闭环(D-M3-1 升格)

> 状态:**冻结(2026-08-30)**。来源:用户两项裁决——①配置管理走「成熟 API」
> 路线:凡允许用户修改的配置,一律有成套的增删改查方法,不为单个功能开一次性
> 配置接口;②D-M3-1 web 线升格为正式里程碑 M10,干完走标准回看门。
> 通讯架构口径(用户问询后确认,随本规格留档):**一套合同语言(Wire API,只增
> 不破)+ 表面各配翻译垫片(CLI/TUI 直连;web 经 api_dsh)+ App=MCP(ADR-0011,
> 远程传输已排期)+ Agent 间同语言(先进程内总线,跨进程时 Agent 亦为 Wire
> 端点)**。阶段二 OS 阶段不换通讯语言,升级的是安全等级(T-13/T-14)与表面数量。
> 范围裁决:纯对话闭环(模型直接回复);工具调用/审批界面的回归不在本批
> (回归方案仍待用户裁决)。

## 零、总纪律

- 合同只增不破:config.list / config.get / config.set / config.delete 四方法
  Minor 增发(`wire/config.v0_1` + envelope method 枚举),`validate.py` 全绿;
  既有黄金轨迹与测试零变化。
- **后端核心零改动**:bm-core / bm-runtime(库)/ bm-providers 一行不动。配置节
  属服务器层(bm-surface-http::config_store);对话闭环在 api_dsh 翻译层把 dsh
  帧映射到既有合同方法(session.create / agent.send_input / store 事件回放)。
- 前端纪律:UI 抄 dsh 原样,布局零改动;「编排数据的 JS」向后端字段看齐;
  不改官方渲染逻辑;改任何 plugins 文件必须 bump index.html rev + 浏览器
  Ctrl+F5。
- 真实浏览器手测轮必过(229 测试全绿测不出那四个前端 bug 的既有教训)。

## 一、S1 配置管理 API(ADR-0012 随本批发)

1. **合同增发(已完成)**:`wire/config.v0_1.schema.json`——四方法 params/result;
   envelope 枚举同步;`bm-contract::wire` 镜像结构体。
2. **config_store(bm-surface-http 新模块)**:
   - v0 配置节 `model`:字段 baseUrl / apiKey(secret)/ modelId / stream /
     displayName;存储 `<data_dir>/config/model.json`(人可读 JSON);
   - 逐字段优先级:**配置文件 > 启动 env > 内置默认**;与
     `boenmind-server` 启动装配共用同一合并(`effective_model`);
   - 口径:secret 回显恒打码(values 中 null + secret_set 标记);config.set
     对 apiKey 缺省/null/空串 = 保持不变;清除走 config.delete(field 或整节);
   - 生效时机 = 保存后重启服务(v0 诚实边界;热生效留后续)。
3. **两条通道,同一实现**:`/rpc/config.*`(Bearer 鉴权,成熟 API,CLI 可用)+
   `/api/config.*`(dsh 界面喂食口,公开挂载 = **已登记欠账**:公网部署前必须
   补鉴权,ADR-0009 T-13/T-14 前置)。
4. **服务器装配**:boenmind-server 经 `effective_model` 装配 connector / 播种
   密钥 / 流式开关(替换原三处直读 env;来源日志如实打印)。

## 二、S2 模型节界面接通(前端「编排数据的 JS」对齐 config)

1. `settings.describe` 喂 `llm-pi-ai` 命名空间:**静态 schema**(Schemastery
   uid/refs 信封,`providers.<route>.api` = union ["openai"])+ 恒空值
   `{providers:{}}`(行数据权威来自 llm.providers)——点亮「添加自定义提供方」。
2. 表单提交 JS 改调 `/api/config.set`,字段映射:表单 API 地址→baseUrl、
   密钥→apiKey、首个模型 id→modelId、名称→displayName;布局与交互零改动。
3. `llm.providers` / `llm.models` / `session.models` 由生效配置驱动:配置齐备
   (baseUrl+modelId)即出「自定义提供方」行(declared:true)与模型组,与
   env 网关行并存;`session.selectModel` 实装(每会话选中,响应 {selected})。
4. 「获取模型列表」按钮保持禁用(后端无对应能力,不为界面造)。
5. 验收:填表单→保存→重启→模型下拉出现→可选中→输入框解锁
   (session.models routable:true)。

## 三、S3 对话闭环(纯对话)

1. **懒建真会话**:dsh `session.create` 保持登记层;首次 `session.prompt` 时经
   `RuntimeHandle.session_create`(AgentSpec.model_chain = [当前选中模型])建
   真会话,dsh↔runtime 映射存 DshState。
2. **发送**:`session.prompt` → `agent.send_input`(content = text 段拼接,
   trusted),响应 `{accepted:true}`(合同 sessionPromptValueSchema)。
3. **事件回流**:每会话转发任务轮询 `store.replay_since`,翻译为 mux 帧:
   - `user/message`(输入落账)、`assistant/chunk`(流式增量,映射
     `model.content.delta`)、`assistant/message`(终稿,映射模型回复合成)、
     `stream/error`(model.invocation.failed 等);
   - 事件信封走宽通道(type/seq/time/data),未知类型前端默认折叠。
4. **历史**:`session.history` 返回该会话已翻译事件(内存投影,与会话元数据
   同生命周期;持久化列为候选尾巴,不阻塞验收)。
5. 验收:浏览器发消息→流式回复上屏(实网 ≤2 次)。

## 四、测试与通过条件(= 回看门 A-E)

- 测试组(M10-T,新文件 `dsh_protocol_tests.rs` + `config_api_tests.rs`):
  - T1 配置 CRUD:set/get 回显打码、留空不改、delete 字段/整节、非法值拒绝、
    /rpc 鉴权与 /api 公开两通道行为一致;
  - T2 模型目录:配置齐备→providers/models/session.models 出组;selectModel
    记忆;config 空时回落 env/空态;
  - T3 prompt 闭环:mock 模型下 prompt 接受、事件翻译帧序列正确(mux 帧
    schema 逐字段断言)、history 回放一致。
- 通过条件:全量 P0 全绿 + `validate.py` 全绿 + clippy 零警告;M10-T 全绿;
  真实浏览器两轮实测全过(配置重启链 + 对话链);INTERACTIONS.md #10 / #17
  翻绿且其余项不回归;实网调用全批 ≤2 次。

## 五、欠账与风险登记

- /api 公开无鉴权(含配置写入与对话数据):公网部署前必须补(PENDING + 审计
  台账;当前仅限本机自用)。
- 保存后需重启生效(v0);热生效留后续。
- dsh 侧会话/工作区元数据仍内存存储(重启即失):列为候选尾巴,不阻塞验收。
- 自定义提供方表单的「编辑/删除已存提供方」路径 dsh 走 settings.mutate/
  credentials 语义,本批不接(界面重填表单即可覆盖);如实留档。
