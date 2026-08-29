# M7 实现规格:Provider、MCP 和 App 隔离(v1.0)

- 状态:冻结(2026-08-30,自冻结即生效,不外发评审——治理规则)
- 基线依据:§18-M7 七子项(M7.1 内置 Capability Provider / M7.2 MCP Server 接入 /
  M7.3 Provider handshake 和能力发现 / M7.4 Provider 崩溃、重启和 unavailable /
  M7.5 Provider 进度、超时和取消 / M7.6 App 数据域隔离 / M7.7 插件与 MCP 信任)
- 通过条件(基线 §18-M7):调用方只依赖 Capability;MCP Provider 可以发现、调用和
  报告进度;Provider 崩溃不会拖垮 Runtime;失败调用不会无限等待;
  App 不能通过内部数据库绕过 Broker。
- 真实 Provider:用户提供第三方中转网关(NewAPI 形态,OpenAI 兼容),
  base_url `https://yujianwudi.top/v1`,模型 `gpt-5.6-luna`。信任边界见 ADR-0010。

## 一、前置结算

| 项 | 裁决 |
|---|---|
| M4 §5.8 模型调用豁免 Broker | **撤销**(M7.1 主体):模型调用过 Broker 决策表与注册表分发,T2 落地 |
| M4-review「真实副作用 Provider 收据/幂等/outbox 对账实证」 | 以 MCP 工具调用全链路(收据+幂等+outbox)实证,T3/T4;真实外部副作用 App 仍留 M8 |
| M4-review「lease 通道真实吞吐」 | 无真实 lease 型 Provider(第三方网关无 lease),**移交 M8**(真实 App 落地时首测) |
| S4 draining / S9 verification / S5 quarantined 分表 | S9 verification 已随 M5 completion gate 落地(留档 M5-review);S4/S5 本里程碑仍不动,随 M8 真实 App 裁决 |
| D-M5-2 memory:user 主体裁定 | 主体裁定已成立(M6 per-task principal);memory:user 授权面**随 M8 首个用户数据 App** 落地 |
| M6-review 遗留(worker 自主 turn 环) | T2 落地真实模型链后,worker LLM turn 环仍属 M8 编排面;M7 只打通「worker 可调真实能力」的管道 |

## 二、裁决(实现即合同)

### S1 模型调用过 Broker(M7.1)
- 注册 manifest:`model.invoke`(provider `model.connector`,version 0.1.0,
  effect=read-only,idempotent=false,cancellable=true,approval=not-required,
  timeout=turn 超时,scopes=[domain:model])。
- turn 循环不再直接持有 connector:spawn 前经 Broker 查表(查表直通路径,
  与 GT-02 场景 A 同构)→ 签发内部凭证 → dispatch → `capability.invoked`
  审计事件(principal=agent:<id>,binding_epoch/provider_instance_id 照实)。
- 连接器可替换性不变:装配层按环境选 Mock(缺省,测试确定性)或
  OpenAiHttpConnector(真实网关);合同 model/connector.v0_1 不破。

### S2 密钥与真实网关(ADR-0010)
- FileSecretStore:本地 JSON 密钥文件(路径 env `BOEN_SECRETS_FILE`,
  缺省 `<data>/.secrets/local.json`,**gitignored**),原子写、0600 语义;
  仓库只提交 `local.example.json` 模板。
- 装配:`BOEN_MODEL_BASE_URL` + `BOEN_MODEL_ID` 存在且密钥文件含
  `secret:model.<model_id>` → 真实连接器;否则 Mock。密钥明文永不入
  事件/日志/错误(INV-5 既有纪律,expose_for_scan 覆盖文件后端)。

### S3 MCP 接入 v1(M7.2/M7.3)
- 传输:stdio(newline-delimited JSON-RPC 2.0)+ InProc(测试);HTTP/SSE → M8。
- 接入 = 安装批准(见 S6)+ 握手:initialize(protocolVersion 2024-11-05)
  → tools/list → 逐工具动态注册 manifest。
- manifest 生成:capability=`mcp.<server>.<tool_norm>`(段字符集
  `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`,连字符规范化为 `_`,非法拒注册);
  input_schema=工具 inputSchema 直通;effect 由 annotations 映射:
  readOnlyHint→read-only、destructiveHint→external-side-effect、
  缺省 reversible-command;approval=required(首调审批,批准可签发
  count/ttl Grant 扩量);timeout_ms=配置缺省 30s;idempotent=false;
  mutation_class 按 effect 派生。
- 调用:tools/call;MCP `notifications/progress` → `capability.progress` 事件。
- 发现:动态注册的能力自动进入既有发现面(capability list / capability.discover)。

### S4 异步 Provider 调用路径(M7.2/M7.5)
- 慢外部 provider(MCP)调用:单写者 spawn → 完成经 `Cmd::ProviderCall`
  回到核心回路(与 TurnEvent 同构);deadline=tokio timeout;
  取消=CancellationToken 贯穿;receipt/幂等/outbox 走既有 capability 路径。
- 模型调用维持专用 turn 异步路径(M1 既有),但入口过 Broker(S1)。

### S5 健康与熔断(M7.4)
- `provider.health.changed` 事件 {provider, from, to, reason},
  状态机 healthy→unavailable→healthy(进程内,不入 core-transitions)。
- HTTP 连接器:连续 3 次失败 → unavailable,冷却 30s 后半开放行一次探测,
  成功即恢复;unavailable 期间调用快速失败(error_code=unavailable,retryable=true)。
- MCP:子进程退出/通道断裂 → unavailable 立即;下次调用重连,上限 3 次,
  超限保持 unavailable 直至重装。
- 崩溃不拖垮 Runtime:provider panic 捕获(catch_unwind)映射 internal 失败;
  MCP 故障隔离在子进程。

### S6 插件与 MCP 信任(M7.7)
- 安装批准:配置文件显式列出 MCP server = 用户显式动作(视为已批准);
  运行期动态注册(未来 Wire 面)一律 approval.requested——M7 仅实现前者。
- 未知风险首次调用审批:MCP 工具 approval=required,首调走既有审批闭环;
  input_trust=untrusted 时 reversible 及以上照旧 100% 升级(ADR-0002 条件 3 不变)。
- 数据域隔离(M7.6):MCP 能力 scopes=[domain:mcp.<server>];App 主体
  (surface:app:<name>)跨 provider 调用默认拒绝,Broker 既有默认拒绝语义 +
  新增显式测试;结构断言:Wire 16 方法之外无存储暴露面。

## 三、边界(本里程碑不做,防蔓延)

1. 模型流式(SSE)与 token 级进度 → M8(进度面由 MCP progress + invocation 事件满足)。
2. MCP HTTP/SSE 传输、resources/prompts 桥接 → M8。
3. 多模型降级链(网关单模型 gpt-5.6-luna;mock 链仍可测 attempt 逻辑)。
4. lease 真实吞吐、真实外部副作用 App → M8。
5. 模型连接器重试策略沿用合同 attempt(1..=3)与既有 turn 循环,不引入新重试面。

## 四、任务分解

| 任务 | 内容 | 验收 |
|---|---|---|
| T0 | 合同增发:+2 事件(capability.progress / provider.health.changed)、mcp/mcp-server.v0_1 合同、sync.rs 镜像、ADR-0010、M7-GT-05 骨架 | validate.py 全绿 |
| T1 | OpenAiHttpConnector + FileSecretStore + mock HTTP server 测试(成功/500/429/超时/坏载荷/密钥缺失)+ 实网 #[ignore] 测试;server 装配 env | 离线套件全绿;实网 1 次真调用通过 |
| T2 | model.invoke manifest + turn 循环过 Broker + 审计事件 + 存量测试修复 | 全套件绿;capability.invoked 断言 |
| T3 | MCP 客户端(InProc+stdio)+ 动态注册 + 异步调用路径 + 进度/超时/取消 | t100-t104 绿 |
| T4 | 健康状态机 + 熔断 + MCP 崩溃重连 + 安装/首调信任 + 数据域隔离测试 | t105-t109 绿;S1-S6 裁决各有测试钉住 |
| T5 | GT-05 定稿、perf 记录⑤(turn 经 Broker 开销)、M7-review、AGENTS.md、PENDING、tag | §19 回看门 |

## 五、测试编号(续 M6:t90-t93)

- t100 MCP 握手与发现(initialize/tools/list → manifest 注册、非法工具名拒注册)
- t101 MCP 工具调用收据(content 形态、幂等键、outbox 对账)
- t102 MCP 进度通知 → capability.progress 事件
- t103 MCP 调用超时与取消(deadline 到点 Failed{timeout};cancel 生效)
- t104 MCP stdio 真子进程(#[ignore],python fixture)
- t105 HTTP 连接器健康:连续失败 → unavailable → 半开恢复
- t106 MCP 崩溃:子进程死 → unavailable → 重连恢复;重连超限保持 unavailable
- t107 安装信任:配置文件 server 启动即注册;动态注册需审批(单元面)
- t108 首调审批:MCP 工具 approval=required 全链路(批准 → Grant → 消费)
- t109 数据域隔离:App 主体跨 provider 默认拒绝;显式 grant 后放行;Wire 面无存储暴露
- 实网:gpt-5.6-luna 一次真实 chat completion(#[ignore],env 门控)
