# web-server 内置前端快照（dsh rc.6 + 皮肤/Web-UI 全家桶）

M2.5 前端直连的静态产物：dsh 官方 web 前端壳层 + 真实 boot 清单 + 全部插件
client bundle，供 Rust 兼容层 `web-server --dist` 直接服务。

## 内容

- `index.html` —— 壳层入口，**已含 `window.__DSH_BOOT__` 注入**（63 个插件 entry，
  rev `c7d29214a72e`；`<` 已按 dsh 规则转义 `\u003c`）。
- `assets/` —— vite 构建的壳层 chunk（index/vendor + 语法高亮 langs + 字体）。
- `plugins/<id>/client.js` —— 每个插件的浏览器 bundle（`WebBootEntry.url` 指向）。
- `boot.json` —— 注入用的原始 boot 清单（便于程序化复用/刷新）。
- `manifest.webmanifest` / `favicon.svg` —— 壳层静态资源。

## 插件面（63 条 = 官方 38 + 皮肤/Web-UI 25）

- **官方 38**（dsh rc.6 全家桶：client-runtime/locale/ui-theme/ui-layout/ui-conversation/
  ui-settings/ui-sidebar/...）。
- **`dsh-frosted-window`**（SenryLee）——参数化玻璃皮肤：浓度/模糊/饱和/压暗 4 滑杆
  + 背景图上传（IndexedDB 存图、localStorage 存参数）。
- **`@linxin666/dsh-web-ui-all` 聚合 + 11 子包 + 12 皮肤子包**（dsh-web-ui 全家桶）：
  - `dsh-client-ui-skin-center`——**皮肤中心**：12 款内置皮肤试穿→应用→互斥切换
    （皮肤子包 = blue-fantasy/whale-song/harbor/qq98/ths/xp/dragon-heir/minecraft/
    trading/miku/whale-mom/matrix，**必须单独安装**——skin-center 只依赖它们运行，
    聚合包不自动带）；
  - `dsh-client-ui-task-board`（任务看板）、`dsh-client-ui-git-graph`（Git 图谱）、
    `dsh-pet`（鲸鱼娘宠物）、`dsh-ssh`（SSH 终端/SFTP）、`dsh-remote-web-ui`
    （移动端远程）、`dsh-live-stats`（实时统计）、`dsh-tool-describe-image`
    （图像理解工具）、`dsh-liangshen`（梁神模式 preset）、
    `dsh-client-ui-aionui-panel` / `dsh-client-ui-community-plugins` /
    `dsh-client-ui-web-ui-settings`（配套面板/设置）。

## 来源与刷新

快照取自 dsh Node 后端真实下发（profile = `dsh-home/profiles/web`，
bundle 组合 = base + web-app + frosted-window + dsh-web-ui-all + 12 皮肤）：

1. 启动 Node 后端（**必须带 DSH_HOME**，否则读默认 `~/.dsh` 回退官方 38 条）：
   `cd dsh-home && DSH_HOME="D:/96_CoderWorld/BoenMind/dsh-home" node profiles/node_modules/@deepseek-ai/dsh/lib/bin.js web --port 3090`
2. `curl http://127.0.0.1:3090/` 取注入后的 index.html 覆盖 `.tmp/web-snapshot/index.html`
   （**先更新 index.html 再跑抓取脚本**，脚本从快照目录读 boot），解析 `__DSH_BOOT__` 的 entry
3. 逐 entry `GET /plugins/<id>/client.js?rev=...` 落盘（`.tmp/grab-snapshot.mjs` 一键）
4. 壳层静态资产（assets/favicon/manifest）从已发布 npm 包
   `@deepseek-ai/dsh-web-frontend` 的 `dist/` 拷贝
5. 整体同步到 `kernel/web-server/frontend/`（含 README 本文件）

dsh 版本升级或换皮肤组合后按上述步骤重抓刷新（快照 rev 与 index.html/boot.json 需同步更新）。

> 注意：`static_spa.rs` 的 boot 注入逻辑在本快照下不启用（index.html 已含
> `__DSH_BOOT__`，重复注入会使模块系统双 boot 抛错）。`web-server` 默认 `--dist`
> 即本目录、默认不注入；对自备 dist 用 `--boot-json` 显式注入。
