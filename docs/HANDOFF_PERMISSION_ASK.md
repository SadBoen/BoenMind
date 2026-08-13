# HANDOFF: 插件权限询问（聊天弹窗 + 工具条权限模式）

> 2026-08-13 · 会话交接 · ✅ **已完成并 commit/push**（见 git log）
> 最终决策（用户拍板，2026-08-13）：**方案 B** —— 接受上游"问一次记永久"语义，
> 不补丁；删除自建白名单表（上游 extension-permissions.json 即权威）；
> UI 文案"允许一次"改"允许"；超时弹窗前端计时器关闭；语义建议提上游 issue #162。

## 一、任务目标（用户需求）

插件请求能力时，在**聊天过程中弹窗询问**用户：**允许 / 拒绝 / 总是允许**。
权限模式切换放在**聊天工具条**（模型/思考一排），支持 默认/安全/宽松/YOLO。
选择权完全交给用户。附带上游能力盘点结论：插件/Skills 引擎全用上游，只补 SDK 缺口。

## 〇、关键测试结论（2026-08-13 实测）

- **询问触发组合**：`extension_policy=balanced + allow_dangerous=true`（UI 无此组合，
  需 API 直设；上游从 deny_caps 移除 exec/env → 落为 prompt 能力）
- **上游决策持久化**：任何询问决策（允许/拒绝/超时）都写入
  `~/.boenmind/pi/extension-permissions.json`，跨会话永久生效，插件版本变化
  （version_range）才重问 → "允许一次"实际 = "总是允许"（已向用户说明并接受）
- **env 读取永不询问**（deny 布尔 + secret 过滤）；safe/balanced 下 exec/env 直接
  Deny 不询问（deny_caps）
- 超时 60s fail-closed（测试验证：工具报 task cancelled）
- GUI 弹窗渲染正常（DOM + 视觉验证）；IAB 自动化对弹窗按钮点击不生效（环境限制，
  真实用户正常）；发送消息用 fill + CUA 真实 Enter

## 二、已完成（代码全部就绪，编译/测试通过）

### 1. vendor 补丁（pi_agent_rust，打标记 + 已提 issue #161）
- `vendor/pi_agent_rust/src/sdk.rs`：
  - `SessionOptions` 新增 `ui_handler: Option<Arc<dyn crate::extension_dispatcher::ExtensionUiHandler + Send + Sync>>`（带 `// BoenMind 补丁:` 注释，Default 置 None）
  - `create_agent_session`：`enable_extensions_with_policy` 后，若 `ui_handler` 存在 → `asupersync::channel::mpsc::channel` + `ExtensionManager::set_ui_sender` + `tokio::spawn` 后台任务消费请求 → `ui_handler.request_ui()` → `manager.respond_ui()` 回写；无响应/错误 → fail-closed 取消
- `vendor/pi_agent_rust/Cargo.toml`：`tokio = { workspace = true }`（补丁用）
- `backend/Cargo.toml`：workspace.dependencies 加 `tokio`（与 bm-server 同款复用）
- **上游 issue**：https://github.com/Dicklesworthstone/pi_agent_rust/issues/161 （已提交，等上游回应）

### 2. bm-core
- `agent.rs`：
  - `AgentStreamEvent` 新增 `PermissionRequest { id, extension_id, capability, message }`（serde camelCase → `permissionRequest`）
  - `create_session_handle` 新增第 10 参 `ui_handler`（透传给 SessionOptions）
- `db.rs`：`plugin_permissions` 表（extension_id+capability PK, allow, updated_at）+ 4 API：`get_plugin_permission` / `set_plugin_permission` / `list_plugin_permissions` / `delete_plugin_permission`；`PluginPermission` 结构

### 3. bm-server
- `permission.rs`（新模块）：
  - `PermissionBridge`（实现上游 `ExtensionUiHandler`）：只处理 `method=="confirm"`；**白名单查表命中直接放行/拒绝** → 注册 pending oneshot → `send_permission_request` 发 SSE 事件 → `tokio::time::timeout(60s)` 等前端决策 → 决策 `always` 时写白名单 → 超时 fail-closed 取消
  - 白名单管理路由：`GET/DELETE /api/plugins/permissions`
- `chat.rs`：
  - `send_permission_request(state, session_id, request_id, extension_id, capability, message)`：经 `state.session_streams` 找活跃 SSE 通道发事件
  - chat 请求开始时注册 `session_streams`（tx.clone()），prompt 结束按 `same_channel` 精确移除（防止误删新 prompt 的通道）
  - `POST /api/chat/permission-response`（request_id + allow + always）→ 发决策给 pending oneshot
  - `run_prompt_and_persist` 的事件匹配加 `PermissionRequest` 分支（透传不聚合）
- `lib.rs`：`AppState` 加 `session_streams` + `permission_pending` 字段 + `PermissionDecision { allow, always }`；路由注册 `POST /api/chat/permission-response`、`GET/DELETE /api/plugins/permissions`；`pub mod permission`

### 4. 前端
- `client.ts`：`ChatStreamEvent` 加 `permissionRequest` 变体；`respondPermission(requestId, allow, always)` API
- `stores/app-store.ts`：`pendingPermission` state + `respondPermission` action；`permissionMode` state + `loadPermissionMode` / `setPermissionMode`（default/safe/balanced/yolo ↔ config.extension_policy + allowDangerous）
- `components/chat/PermissionDialog.tsx`（新）：弹窗，按钮 拒绝 / 总是允许（amber）/ 允许一次（primary）；无关闭按钮（不选择 = 超时自动拒绝）
- `components/chat/ChatInput.tsx`：工具条加权限模式 Select（ShieldCheck 图标，default/safe/balanced/yolo 四档），挂载时 loadPermissionMode
- `components/chat/ChatWindow.tsx`：挂载 `<PermissionDialog />`
- i18n 四语（zh/en/ja/ko）：`chat.permission.*`（弹窗 + 工具条模式文案）

### 5. 质量门（通过）
- 后端：`cargo test -p bm-core -p bm-server` 58+10 全过；clippy 我们代码零警告
- 前端：tsc -b / oxlint / vite build 全过

## 三、端到端测试（✅ 全部完成，2026-08-13）

| 验证项 | 结果 |
|---|---|
| balanced + allow_dangerous=true → `permissionRequest` SSE 事件 | ✅（清上游缓存后稳定触发） |
| 决策"允许" → exec 放行 → 工具结果落库 | ✅ |
| 决策"拒绝" → 工具 is_error + 模型如实报告 | ✅ |
| 60s 无响应 → fail-closed（task cancelled） | ✅ |
| "总是允许" → 上游缓存落盘 + 二次不询问 | ✅（我们的表同录，但已随方案 B 删除） |
| 白名单 DELETE → 上游缓存仍在 → 仍不询问 | ✅（佐证上游缓存是权威） |
| 弹窗 GUI 渲染（DOM + 视觉 MCP 复核） | ✅ 三按钮 + 提示文案 + 遮罩正常 |

### 浏览器自动化限制（非产品 bug，已确认）
- IAB 合成点击对**聊天输入框区域**不生效：textarea 用 `playwright.fill()` 有效；
  发送用 **fill + CUA 真实 Enter** 有效；弹窗按钮 Playwright/CUA 点击均不生效
  （弹窗浮层，自动化盲区；真实用户鼠标键盘正常）
- SSE 事件验证用 curl 直连最稳

## 四、完成状态 / 遗留

1. ✅ 触发询问端到端验证（见上表）
2. ✅ 弹窗 GUI 实测（渲染验证；按钮点击受自动化限制，链路已由 curl 等价验证 + 代码审查）
3. ✅ 清理：perm-demo 插件、config.toml（extension_policy 行 + perm-demo/hello）、
   16 个测试会话、上游 extension-permissions.json、DB 旧表 plugin_permissions（已 DROP）
4. ✅ commit + push（message: feat(permission) 插件权限询问全链路，见 git log）
5. **可选后续**：PluginsSettings.tsx 的权限模式逻辑与 app-store 重复，可重构复用；
   上游若采纳 issue #162（per-session 决策作用域），可再补"允许一次"真语义
6. 关注上游 issue #161（ui_handler 透传）、#162（决策持久化语义）回应

## 五、关键文件清单

| 文件 | 改动 |
|---|---|
| backend/legacy/pi_agent_rust/src/sdk.rs | 补丁：ui_handler 字段 + 透传 |
| backend/legacy/pi_agent_rust/Cargo.toml | 补丁：tokio 依赖 |
| backend/Cargo.toml | workspace tokio |
| backend/crates/bm-core/src/agent.rs | PermissionRequest 事件 + ui_handler 参数 |
| backend/crates/bm-server/src/permission.rs | **新**：PermissionBridge（查表/写表白名单逻辑已按方案 B 删除） |
| backend/crates/bm-server/src/chat.rs | session_streams + send_permission_request + respond_permission |
| backend/crates/bm-server/src/lib.rs | AppState 字段 + 路由 + mod（白名单路由已移除） |
| backend/crates/bm-server/Cargo.toml | async-trait 依赖 |
| frontend/src/api/client.ts | 事件类型 + respondPermission |
| frontend/src/stores/app-store.ts | pendingPermission + respondPermission + dismissPermission（超时）+ permissionMode |
| frontend/src/components/chat/PermissionDialog.tsx | **新**：弹窗 + 60s 超时自动关闭计时器 |
| frontend/src/components/chat/ChatInput.tsx | 工具条权限模式 |
| frontend/src/components/chat/ChatWindow.tsx | 挂载弹窗 |
| frontend/src/i18n/locales/{zh,en,ja,ko}.ts | chat.permission 文案（允许一次→允许 + 记住提示） |
