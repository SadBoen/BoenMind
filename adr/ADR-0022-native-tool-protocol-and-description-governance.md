# ADR-0022: 工具调用原生协议还原与工具描述治理

- 状态: Accepted(用户 2026-09-06 明示「把修复与改进全都做完」,全权委托过夜交付)
- 日期: 2026-09-06
- 关联: ADR-0021(fs.* 内置化)、ADR-0019(system.exec)、ADR-0006(权限显式化)、ADR-0013(前端线)、W4(对话工具闭环)、W5(context-log)
- 证据基线: `docs/agent-tools-payload-comparison-report.md` v2 —— 对 DSH(本机 npm 包源码逐字提取)、Pi Agent(badlogic/pi-mono GitHub 源码)、Hermes Agent(NousResearch,VPS 实机 v0.20.6 只读核实)三家「发给 LLM 的最原始底层报文」的横评调研
- 背景: 用户实测 BoenMind Agent「工具使用非常别扭」。调研定性出四个底层根因:①工具结果被伪装成 `role:"user"`(openai_http.rs 为兼容第三方网关的权宜);②工具轮回喂时 assistant 消息丢失 tool_calls 结构(模型看不到自己发起过什么调用);③每次工具成功后强贴「该调用已完成…不要再次调用该工具」负向禁令(斩断链式调用);④MCP 工具描述被整层丢弃(模型只见「只读直通工具」套话),且审批 UI 措辞(「弹出审批卡片」)混入工具描述。现代 LLM 对 `role:"tool"` + `tool_call_id` 的因果链有专项训练,以上四点全部逆训练本能而行。

## 决策

### 1. 协议还原(优先级最高)

- `Role::Tool` 消息出**原生 `role:"tool"`**,并携带 `tool_call_id` 与发起调用的 assistant 消息对齐;
- assistant 消息回喂时**原样携带 `tool_calls`**(id/type/function.name/arguments),纯工具调用无文本时 content 置 null(OpenAI 形态);
- 兼容回退:仅当 tool 消息缺 `tool_call_id`(修复前的历史消息,正常路径不产生)才回落 user 伪装;**不再为「远古网关」整体降级现代协议**——真实第三方中转(opencode zen 等)实测均原生支持 function calling。
- 合同 Minor(只增不破):`model/connector.v0_1.schema.json` 的 messages.items 增发可选 `tool_call_id` 与 `tool_calls`(子结构 id/name/arguments);Rust 镜像 `bm_contract::connector::Message` 同步增发(serde default + skip_if_none,旧序列化数据可读)。

### 2. 负向禁令废除

- 删除工具成功回喂的统一后缀「(该调用已完成,请直接基于此结果回答用户,不要再次调用该工具。)」(turn.rs)与审批成功/拒绝文案中的同类祈使句;
- 审批结论**如实转述**保留:「用户已批准,工具执行成功。返回结果: …」/「用户拒绝了能力 X 的本次审批请求,工具未执行。」——信息给足,判断交还模型;
- 循环失控防线 = 既有 `MAX_TOOL_ROUNDS = 5` 熔断 + 超限空终稿显式说明(不变)。历史背景:2026-09-03 该禁令为压制 mimo 见成功结果后重复调用而加(当时协议本身就是 user 伪装,模型确有重复诱因);本次随协议还原一并撤除,若真实模型复现死循环,正确修法是按模型条件化(provider/model_id 维度)而非全局禁令。

### 3. 工具描述治理(描述随 manifest 走)

- `capability/manifest.v0_1.schema.json` 增发可选 `description`(manifest 本为开放结构,新增可选字段 = Minor,Rust 镜像 `CapabilityManifest` 同步);
- `Registry::chat_tools()` 扩为四元组 `(名, 参数schema, 需审批?, 描述)`;turn 组装工具清单时描述取 manifest,**废除硬编码 match 与「只读直通工具/需要用户审批的业务工具(调用后会弹出审批卡片)」套话兜底**;
- 内置能力自描述:fs.search/fs.read/fs.write/fs.edit(描述随 ADR-0021 的 manifest 迁移并改写)与 system.exec 各带一句功能描述,措辞只讲**运行时语义**(「调用需用户批准后执行」),不讲前端 UI 行为;并附正向导流(「看代码优先 fs_read」「纯文件查读优先 fs_search/fs_read」);
- MCP 工具自描述:`tools/list` 拿到的 description 进 manifest(MCP 工具名禁连字符等既有注册规则不变);缺省时 turn 侧按审批语义给最小兜底。

### 4. fs.edit 批量升级(对标 Pi edits 数组)

- 新增 `edits: [{old_string, new_string, replace_all?}]` 批量形态:全部编辑基于**文件当前原文**快照定位命中区间,跨编辑区间不得重叠,一次读-校-写原子提交;
- 单处 `old_string/new_string` 字段保留,向后兼容;CRLF 自动兼容逻辑逐条沿用;
- 语义收益:同文件多处修改从「N 次往返+中间态行号漂移+禁令拦截」变为「1 次调用原子完成」。

### 5. 观测面同步

- context-log 快照 messages 增记 `tool_call_id`/`tool_calls`(透视面板/DSH 吸收线可见完整「调用→结果」因果链);轨迹事件面(tool_call/tool_result)不变。

## 条件与验收

- 合同 validate.py 全绿;workspace fmt + clippy --all-targets 零警告;全量测试绿(wire 形态 3 新测 + fs 批量 5 新测);
- 真模型对话 E2E:工具链式调用(搜→读→[改→])顺畅不被禁令打断;审批流(批准/拒绝)回喂正常;UI/轨迹可见 `[调用 xxx]` 与结果配对;
- 不破坏 W4b 审批闭环与 ADR-0006 权限语义(审批判据与 Broker 管线零改动,本 ADR 只改「发给模型什么」与「描述怎么写」);
- 远期候补登记 BACKLOG:DSH Code Mode(多轮往返脚本合并)、Hermes tool_search 渐进披露、按模型条件化 schema。
