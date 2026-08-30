# runtime/web 来源说明(2026-08-30)

本目录前端为 **deepseek-harness 官方 Web 前端原样复刻**(MIT License,
© DeepSeek AI),仅做品牌层替换,未改任何功能代码:

- `index.html` + `assets/` + `favicon.svg` + `manifest.webmanifest`:
  来自 npm 包 `@deepseek-ai/dsh-web-frontend@0.1.1-rc.2` 的 dist 构建产物;
  其中 index.html 为 dsh server 运行时注入版(ModuleLoader 引导 +
  `__DSH_BOOT__` 启动清单,静态化)。
- `plugins/**/client.js`(42 个客户端模块):自 dsh server `/plugins/`
  端点抓取的官方界面模块(侧栏/对话/设置/工作区等 UI 本体),
  路径结构保持与 dsh server 一致。

## 品牌替换点(全部为纯显示层,零功能改动)

1. `plugins/@deepseek-ai/dsh-client-ui-brand-official/client.js`:
   鲸鱼标(FishLogo)与 deepseek 字标(BrandWordmark)替换为
   BoenMind 文字圆标("B" + "BoenMind"),插槽注册结构不变。
2. `plugins/@deepseek-ai/dsh-client-ui-conversation/client.js`:
   locale 词典 `hero.headline` "探索未至之境" → "个人生态的 AI Runtime"。

## 后端连接现状(2026-08-30,dsh 宿主协议适配已起步)

dsh 前端经 api-gateway 协议与宿主通信,BoenMind 侧适配层:
`runtime/crates/bm-surface-http/src/api_dsh.rs`。已接通:

- `POST /api/host.describe`(连接握手,host 元信息)
- `GET /api/events.mux`、`/api/events.host`(WebSocket 事件流 + SSE 回退,
  当前为空事件流心跳)
- `POST /api/workspace.list` / `session.list` / `agentPreset.list`(合法空态)
- `POST /api/settings.describe` / `settings.mutate`(内存存储,预置
  ui-onboarding 关闭内测声明;重启重置)
- `POST /api/llm.providers` / `llm.models` / `llm.discoverModels`
  (单 provider=服务器 env 网关配置)
- 未适配方法 → `{ok:false, error:{code:"bad-request"}}`(dsh 错误码封闭
  枚举内合法值)

设置面板已按用户裁决精简:仅留「模型(LLM provider)」节(通用/插件/
Agent 预设三节注册已注释保留原码,见各 client.js 内 BoenMind 标记)。

许可证:上游 MIT 文本见 deepseek-harness 仓库 LICENSE;本仓库第 0 层
AGENTS.md 不变。
