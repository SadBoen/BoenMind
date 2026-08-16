# 交接：LLM Provider 插件化（方案 A）——立即执行

> **✅ 已完成（2026-08-16）**：§八验收全过，本文档归档至 docs/archive/HANDOFF_LLM_PROVIDER_PLUGIN.md。
> 落地提交：93d542f（后端）+ 6eed10a（前端）。详见 docs/EXTENSION_POINTS_REGISTRY.md（provider 服务面登记）。
>
> 交接日期：2026-08-16 ｜ 拍板人：用户（已拍板，本任务直接开工，不需要再问方向）
> 背景：三工具交叉审查轮（docs/REVIEW_TOOLS_CROSS_2026-08-16.md）发现 `pi_name` 24 路映射是
> 唯一被判"真死代码"的扩展点。用户定调：**按万物皆插件，把整个 LLM provider 剥离成插件**，
> 名称不带 PI；厂商精简到几家常用的。上一轮对话已删除，本文档是唯一上下文——**自包含**。

## 一、任务目标（一句话）

把"模型厂商"从核心代码的硬编码枚举（24 家）改成**可注册的 Rust 插件**（方案 A）：
核心只留协议（一家厂商长什么样），具体厂商 = 插件注册；内置厂商精简到几家常用的；
`pi_name` 变成插件协议里的稳定标识（改名，不带 pi）。

## 二、已拍板决策（不要重新问用户）

1. **方案 A：Rust 插件**（厂商解析/连接逻辑在 Rust，与 LLM 调用同层；第三方可自定义
   LLM 提供商——Custom 必须保留且是核心价值）
2. **厂商精简**：用户点名保留 `minimax-cn`（= 现 Minimax，https://api.minimaxi.com/v1）、
   `deepseek`、`openrouter`、`opencode`（"之类的"——实施轮按常识定最终清单，见 §六建议清单，
   若与用户点名冲突以用户点名为准）。**不需要 24 家。**
3. **名称不带 PI**：`pi_name` 方法改名（建议 `stable_id`），并入插件协议
4. **立即做**（用户已说"马上就做"）；交接后新对话直接实施

## 三、现状代码地图（改动前先读这些）

| 位置 | 内容 |
|---|---|
| `backend/crates/bm-core/src/config.rs:120-200` | `ProviderKind` 枚举（24 变体 + `ALL` 数组 + `pi_name()` 24 路映射 + `is_openai_compatible_route()`）——**主要改动点** |
| `backend/crates/bm-core/src/providers.rs` | `official_base_url()`（24 家官方端点表，第 26-58 行）、`official_base_urls()`（下发前端）、`list_provider_models` / `test_provider_connection`（运行时查模型/测连接） |
| `backend/crates/bm-core/src/thinking.rs` | 思考档位白名单（is_known_non_reasoning / is_deepseek_reasoning，按模型名判定） |
| `backend/crates/bm-server/src/service_faces.rs:129-160` | `LlmPortImpl`（llm 服务面，从 config.providers 找厂商 → resolve_llm_config → JSON 返回）——**插件化后的消费方** |
| `backend/crates/bm-server/src/bm_engine.rs:442+` | `resolve_llm_config`（provider 配置 → LlmConfig；base_url 用户填写优先否则官方端点） |
| `backend/crates/bm-server/src/routes/providers.rs:23` | `GET /api/providers/presets`（官方端点表下发前端预填表单） |
| `backend/crates/bm-server/src/lib.rs`（kernel 装配段） | 服务面注册（settings/stats/llm/…13 面）——ProviderPort 注册处参照 |
| `backend/crates/bm-protocol/src/port.rs` | 服务面 Port trait 定义处（新增 ProviderPort 放这里；注意 bm-protocol 零依赖纪律，见 §五坑 3） |
| 前端 `frontend/src/components/settings/ProvidersSettings.tsx` + provider-presets 相关 | 提供商设置页（预设 24 家表单） |
| `backend/crates/bm-core/src/config.rs:418-426` | `provider_kind_pi_name` 测试（pi_name 唯一消费方，改名/删除时同步） |

**相关数据流**：`config.toml` 用户配置 `providers[]`（id/name/kind/base_url/api_key/models）→
`resolve_provider/resolve_model`（请求级 > 会话级 > 默认）→ `LlmPort.resolve_config` →
`resolve_llm_config` → `LlmConfig` → bm-loop `OpenAiClient`（OpenAI 兼容形状）。
`session.provider_id` 存用户配置的厂商 id（数据库兼容，见 §五坑 2）。

## 四、目标设计（方案 A 落地形态）

```
kernel 注册表
  ├─ ProviderPort（新增，bm-protocol）：注册/查询厂商插件
  │    trait: 厂商 id（stable_id，取代 pi_name）/ 展示名 / kind /
  │           官方端点（None=必须用户填）/ 协议形状（OpenAI 兼容 or Anthropic 方言）/
  │           模型清单 / 窗口 / 思考档位支持
  ├─ 出厂 provider 插件（内置几家常用，注册即用；其余可第三方插件注册）
  ├─ llm 服务面（现有 LlmPortImpl）→ 改经 ProviderPort 解析（不再直读硬编码表）
  └─ Custom 厂商：用户填端点+协议形状即注册（方案 A 的核心价值，必须可用）
```

**改动要点**：
1. `ProviderKind` 枚举精简到保留清单（§六），`ALL` 同步
2. `pi_name()` → `stable_id()`（或并入 ProviderPort 协议），全仓替换/删除，测试同步
3. `bm-protocol/port.rs` 新增 `ProviderPort` trait + `bm-server` 实现（ProviderPortImpl
   持 config；或出厂插件形态）；kernel 装配注册
4. `LlmPortImpl`/`resolve_llm_config` 改经 ProviderPort 取官方端点/协议形状
5. `official_base_urls()` 随精简清单同步；前端 presets 自然缩小
6. `thinking.rs` 白名单按保留厂商收敛（不删功能，只收敛数据）

## 五、坑与约束（实施轮必读）

1. **质量门**：推送前 `hooks/pre-push` 全绿才放行——bm-server+bm-core 测试、bm-compat
   5 套件（`--test host load execute events session`）、clippy 两档（`-D warnings`）。
   改 config.rs 后 `bm-core` 测试必须全跑（provider_kind_pi_name 等会红，是预期，同步改）。
2. **配置/数据兼容**：现有 `config.toml` 可能配了被删的厂商（如 groq/mistral）；`session.provider_id`
   存的是用户配置 id 不是 kind。策略建议：被删 kind 的既有配置仍可解析（kind 反序列化失败会
   炸配置加载——需要兼容映射或迁移提示），实施轮要处理并验证 `bm-core` 配置加载测试。
3. **bm-protocol 零依赖纪律**：ProviderPort trait 只能引用 serde 级类型，不能依赖 bm-loop/
   bm-core 类型（协议 crate 零运行时依赖是物理锁，架构 §3.1）。厂商的"连接/客户端"逻辑
   放 bm-server 或插件 crate，协议层只有数据形状。
4. **万物皆插件理念**（用户定调）：不删挂点/扩展点；新增扩展点必须登记
   `docs/EXTENSION_POINTS_REGISTRY.md`（ProviderPort 登记一行，pi_name 行状态改为"已并入
   provider 插件化"）。LlmPort 是消费方不是被删对象。
5. **Custom 是灵魂**：用户说"可以自定义 LLM 提供商"——Custom 路径（用户填端点+key+
   协议形状，不经内置表）必须端到端可用，这是"厂商=插件"理念的最小闭环。
6. **提交纪律**：完成即 commit+push（自动推送政策）；commit 消息风格看 `git log`（中文，
   `feat/fix/refactor/docs` 前缀 + 描述）。
7. **用户沟通**：技术解释用大白话；实施中若出现新的拍板点（如"opencode"到底指什么、
   被删厂商的配置迁移策略），**列拍板点给用户**，不要自己拍方向级决策。
8. **opencode 待确认**：用户点名的 "opencode" 在现有 24 家里没有对应 kind。实施轮先按
   "保留清单包含用户点名的四家 + 常识补充"执行，把 opencode 的归属作为拍板点问用户
   （可能指某个新厂商/本地工具，需要用户给端点或确认忽略）。

## 六、建议保留清单（实施轮按此 + 用户点名执行）

- 必留（用户点名）：Minimax（minimax-cn）、Deepseek、Openrouter、Custom（自定义）
- 建议保留（"之类的"常识补充，理由一句话）：OpenAI（生态基准）、Anthropic（Claude 方言
  路径 is_anthropic 唯一消费者）、Ollama（本地模型，127.0.0.1 端点）
- 其余 24 家中的：删
- opencode：拍板点（见坑 8）

## 七、实施步骤（建议顺序，每步可独立 commit）

1. **调研+拍板点**：确认 opencode 归属、被删厂商配置迁移策略（问用户，大白话列选项）
2. **精简枚举**：ProviderKind 收敛 + ALL + official_base_url + is_openai_compatible_route +
   thinking.rs 白名单收敛；`provider_kind_pi_name` 测试同步；跑 bm-core 测试全绿
3. **pi_name → stable_id**：改名（保留"稳定标识"语义：Custom = `custom-{id}` 前缀规则保留），
   全仓替换；无生产调用方，只动定义+测试
4. **ProviderPort 协议**：bm-protocol/port.rs 加 trait（数据形状，零依赖纪律）；bm-server 实现
   + kernel 注册；登记表登记一行
5. **消费链路改造**：LlmPortImpl/resolve_llm_config 经 ProviderPort 取官方端点/协议形状；
   bm-server 测试全绿
6. **前端同步**：presets 自动缩小（数据驱动，验证设置页表单）；前端硬编码引用清理
7. **兼容验证**：带旧 config.toml（含被删厂商）启动验证；session.provider_id 会话恢复验证
8. **收尾**：质量门全绿 + 全量测试 + 更新登记表/交接状态 + 记忆

## 八、验收标准

- [ ] ProviderKind 只剩保留清单（+Custom），全仓无 pi_name 痕迹
- [ ] ProviderPort 服务面注册在 kernel，LlmPort 经它解析
- [ ] Custom 厂商端到端可用（配置 → 模型列表 → 发消息 → 流式回复）
- [ ] 被删厂商的既有配置不炸启动（迁移提示或兼容映射，有明确策略）
- [ ] 质量门全绿（pre-push 脚本）；docs/EXTENSION_POINTS_REGISTRY.md 已更新
- [ ] 前端设置页预设与精简清单一致

---
*交接完成标志：§八验收全过 + 本文件移入 docs/archive/（参照 HANDOFF 归档惯例）。*
