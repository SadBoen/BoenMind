# web-server 内置前端快照（dsh rc.6 官方 38 bundle，无皮肤）

M2.5 前端直连的静态产物：dsh 官方 web 前端壳层 + 真实 boot 清单 + 全部官方插件
client bundle，供 Rust 兼容层 `web-server --dist` 直接服务。

## 内容

- `index.html` —— 壳层入口，**已含 `window.__DSH_BOOT__` 注入**（38 个官方插件 entry，
  rev `b14991c14466`；`<` 已按 dsh 规则转义 `\u003c`）。
- `assets/` —— vite 构建的壳层 chunk（index/vendor + 语法高亮 langs + 字体）。
- `plugins/<id>/client.js` —— 每个插件的浏览器 bundle（`WebBootEntry.url` 指向）。
- `boot.json` —— 注入用的原始 boot 清单（便于程序化复用/刷新）。
- `manifest.webmanifest` / `favicon.svg` —— 壳层静态资源。

## 插件面（38 条 = 官方全家桶）

- **官方 38**（dsh rc.6 全家桶：client-runtime/locale/ui-theme/ui-layout/ui-conversation/
  ui-settings/ui-sidebar/...）。皮肤/Web-UI 全家桶（@linxin666 12 皮肤、frosted-window 等）
  已按架构定稿删除，只留官方默认（2026-08-19）。

## 来源与刷新

快照取自 dsh Node 后端真实下发（profile = `dsh-home/profiles/web`，
bundle 组合 = base + web-app 官方默认）：

1. 启动 Node 后端（**必须带 DSH_HOME**，否则读默认 `~/.dsh` 回退官方 38 条）：
   `cd dsh-home && DSH_HOME="D:/96_CoderWorld/BoenMind/dsh-home" node profiles/node_modules/@deepseek-ai/dsh/lib/bin.js web --port 3090`
2. `curl http://127.0.0.1:3090/` 取注入后的 index.html 覆盖 `.tmp/web-snapshot/index.html`
   （**先更新 index.html 再跑抓取脚本**，脚本从快照目录读 boot），解析 `__DSH_BOOT__` 的 entry
3. 逐 entry `GET /plugins/<id>/client.js?rev=...` 落盘（`.tmp/grab-snapshot.mjs` 一键）
4. 壳层静态资产（assets/favicon/manifest）从已发布 npm 包
   `@deepseek-ai/dsh-web-frontend` 的 `dist/` 拷贝
5. 整体同步到 `bm/web-server/frontend/`（含 README 本文件）

dsh 版本升级或换皮肤组合后按上述步骤重抓刷新（快照 rev 与 index.html/boot.json 需同步更新）。

> 注意：`static_spa.rs` 的 boot 注入逻辑在本快照下不启用（index.html 已含
> `__DSH_BOOT__`，重复注入会使模块系统双 boot 抛错）。`web-server` 默认 `--dist`
> 即本目录、默认不注入；对自备 dist 用 `--boot-json` 显式注入。
