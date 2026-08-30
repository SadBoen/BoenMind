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

## 后端连接现状

dsh 前端经由其自有 WebSocket 协议与宿主通信(`connection` 模块);
BoenMind 后端(rust)尚未实现该协议,故界面为未连接空状态。
后续按"后端连接一点点做好"路线逐项适配(见 milestones/PENDING.md D-M3-1)。

许可证:上游 MIT 文本见 deepseek-harness 仓库 LICENSE;本仓库第 0 层
AGENTS.md 不变。
