# BoenMind 前端工作交接（2026-08-22）

## 当前状态

**v3 前端已按 `docs/FRONTEND-GUIDE.md` 一口气重写完成，且聊天已接真实后端打通。**
`frontend/` 是 React 19 + Vite 8 + Tailwind v4 + daisyUI 5 + 自建 Unit + react-resizable-panels。`tsc -b` / `npm run build` 全绿；后端 `cargo test` 31 绿（含新 `session.delete`）。

**本轮已交付（2026-08-22）：仿真聊天 → 前后端打通。**
- 前端 `lib/api.ts`：RPC 客户端（client-request 信封）+ mux/host 双 WS 下行流（自动重连）。
- `store.tsx`：会话/消息/流全部接真实后端 —— `session.list` 启动拉列表、`session.history` 切会话拉历史、`session.prompt` 发消息、`session.cancel` Stop、`session.rename`/`workspace.archiveSession`/`session.delete`/`session.selectModel` 转发；WS `session/event` 流式增量的 wire→前端消息换算（text-delta/reasoning-delta/tool-call/assistant/message/turn/end）。假流定时器已删，`stream-tick/tool/end` action 已清，`SEED_SESSIONS/SEED_MESSAGES` 种子已删。
- **命令层（cmd）**：reducer 内联 RPC 副作用全部移到 `StoreProvider` 的 cmd 命令层（new-session 生成 sid 注入 action、set-model 反查 provider 同步 selectModel + 全局 defaultModel、retry/stop/rename/archive/delete 统一收口），**根治 StrictMode 双发 RPC**。
- **模型事实源统一（用户反馈修复）**：聊天下拉不再硬编码 `MODELS`（grok 系）——改为「本地配置 providers 模型 + 后端 `llm.models` 装配模型」合并（带提供商名 label）；store boot 拉 `llm.models`（`fetchBackendModels` 经 rpc helper 剥信封）。设置里添加 MiniMax/DeepSeek 后聊天下拉即时出现其模型，与设置页完全一致。
- 后端 `api/session.rs` 新增 `session.delete` RPC（防 running，删持久化日志 + live 表 + host 广播 `host/session-removed`），单测通过。
- 验证：curl 全 RPC 实测、node WS 收流完整、e2e-sim 全闭环、浏览器快照显示后端会话列表/历史消息渲染；浏览器实测「设置添加 MiniMax → 聊天下拉出现 MiniMax 模型 + Mock · mock-1」+「新建会话 → 发送 → 用户/助手消息渲染」。

前端 RPC 契约与后端 wire 事件形状见 `bm/web-server/src/events.rs`（WireSessionEvent）与 `api/session.rs`。

相关文档：
- 📐 **界面规格**：`docs/FRONTEND-GUIDE.md` v3
- 🧊 **玻璃 token**：`.review/grok/glass-opacity-grok.md`

本轮实现时踩到并已修：daisyUI 类名冲突（`.status` 把状态栏吃成圆点、`.modal` 把确认框 `visibility:hidden`）。自定义类一律避开 daisyUI 组件名（`statusbar` / `dialog-card` / `slide-drawer` / `pop-menu`）。

---

## 1. 任务（新接手要做的唯一大事）

**按 `docs/FRONTEND-GUIDE.md` v3 §9「一口气写完」清单实现整个前端**：聊天界面、文件单元、设置框架、主题两轴。规格已齐（布局总图 §1、token §0、聊天 §4、文件 §5、设置 §6、主题 §7.1/7.2）。实现纪律写死在 §9 开头：**一口气写完不中断，token 表第一步就写进代码**。

## 2. 技术栈（已定死）

- React 19 + TypeScript + Vite 8 + Tailwind v4 + daisyUI 5 + react-resizable-panels
- 通知：sonner；布局：自建 Unit + RRP；不用 dockview
- 当前目录 `frontend/` 已是这套依赖（package.json 已配好，`npm install` 后可用）

## 3. 磁盘现状

```
frontend/src/
├─ style.css          token + 黑白/玻璃两轴
├─ store.tsx          会话/消息/文件/设置（localStorage）
├─ layouts/           IconNav · Shell · StatusBar · Topbar
├─ components/        Unit · ConfirmDialog · ContextMenu · SettingsForm
├─ panels/            Workspace · Chat · Session · FileDock · Settings
└─ settings/          声明式注册表 + 分区懒加载
```
- `cd frontend && npm run dev` → http://127.0.0.1:5173
- 后端反代 `/api` → `127.0.0.1:3080`（`changeOrigin:false`；WS 也代理 `/api/events.mux`、`/api/events.host`）。**聊天已接真实后端**：会话列表/历史/发送/Stop/重命名/归档/删除/模型选择全走 RPC，实时流走 WS `session/event`（`lib/api.ts` + `store.tsx` 的 backend-event/apply-history）。

## 4. v3 规格锚点（接手人必须逐条对齐）

- **唯一布局总图**（v3 §1）——rail(48px) 三图标(聊天/编程/WIKI占位)+底设置齿轮；右侧文件 dock 仅聊天视图挂；statusbar 通栏。**离线没有第二套图**。
- **token**（v3 §0）：尺寸全名 token（`--space-1/2/3`, `--radius-sm/md/pill`, `--font-sm/ui/body`, `--control-h:48`, `--hairline:1px` 唯一裸 px 例外）；全文禁裸 px。
- **文件入口定死**：聊天 topbar 右侧「文件」按钮 + 目录树顶抽屉图标两层收起；非聊天视图无文件 dock；minSize 用**百分比换算**（库 API）不是 px 直传；禁止拖到 0。
- **聊天**：消息层级硬规则（用户右气泡头像/助手左 prose 头像/工具卡折叠/推理卡）+ Stop + 复制/重试/编辑 + 空态 + tag 闭环。
- **设置**：声明式注册表 + SettingsForm 表单原语 + SKILL/插件=独立分区整页表格（搜索/卸载确认/设置居中 modal）。
- **主题两轴**：风格(现代/卡通)×材质(纯色/毛玻璃)；黑白=现代×纯色默认；禁渐变。
- **黑白视觉**（§7.1）：「状态即光线」签名——静默灰阶、只有活动状态亮 accent #3b82f6。
- **玻璃视觉**（§7.2）：黑纱 α 分层 + 白散射霜 + blur 最多两层 + `--g` 滑块(20-95%，只控结构层) + accent #9AABB7 ≤3px；**禁第三方玻璃生成器值**（参考 glass-opacity-grok.md）。

## 5. 必须躲开的坑（来自实测 + grok 评审 + 历史）

1. **RRP `minSize` 是 0–100 百分比**，不是 px——`minWidthPx` 要内部换算，禁止直接 `minSize={220}`。
2. **收起只能命令式**（`collapse()/expand()`），不许靠拖拽收到底（拖到 0 后找不到分割线无法恢复）。
3. **面板组件注册表不要内联箭头函数**（React 每次渲染变引用触发重建）。
4. **Vite 反代必须 `changeOrigin:false`**（后端信任栅栏）。
5. **TS 钉 `~5.8.3`**（v7 会让工具链炸）。
6. **IAB 实测**（web-gui-tester）：Playwright locator 对 daisyUI `btn-primary` 圆形按钮 click 超时，cua 坐标点击正常；ghost 按钮 locator 正常。**不是 UI bug**，是自动化定位问题——真测试用 cua 或坐标。
7. **玻璃档禁止对文字容器写 `opacity`**（字会淡），只改 background 的 alpha。
8. **不用 dockview**；不用第三方玻璃生成器临时值；不用 workspace 做会话分类（tag，铁律）。

## 6. 交付验证（做完这一整套才算完）

- `tsc -b` / `npm run build` 全绿。
- 浏览器（IAB 或真实）：宽窄屏两档、会话栏 minWidth 拖拽不塌、抽屉进出、topbar ☰、文件 dock 进出/编辑区收起、设置表格搜索/卸载确认/设置 modal、toast 出现与消失、流式 Stop。
- 截图存 `gui-test-screenshots/` 作证据。
- 做完在 HANDOFF 顶部更新"当前状态"段，把历史清空。

## 7. 交接者注

- **这份文档是给"从零开工"的**，不是"续写"。若接手人只想在旧实现上改，先把 v3 §9 读一遍再说——大概率直接重写更省。
- 调 grok 恶补评审时，模型选 `grok-4.6`、temperature 0.3、max_tokens 8000，payload 参考 `.review/grok/guide-review-payload.json` 格式。

签名：交接者（现场实现）