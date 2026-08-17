 公网站点密码登录门 —— 现状核查与收尾施工单

日期：2026-08-17 · 性质：架构核查（任务1实际未完成，处于半成品状态）+ 交给 coder 的精确施工单

> 压缩前会话误报「任务1基本完成」。本次源码级核查（backend bm-server + frontend）确认：
> **功能块已写出但未接线**，当前代码无法编译/构建。本文档给出逐项差异与最小收尾改动。

---

## 1. 目标（任务定义）

- 公网站点仅密码登录，默认 `adminadmin`，设置中心可改
- 未登录禁入聊天/编程/WIKI/设置（UI 登录门）
- `BOENMIND_TOKEN` 仅继续供 API 守卫；`/api/auth/*` 豁免 token 中间件

## 2. 设计意图（auth.rs 头注释已锁，无需重议）

- 只密码、无用户名；默认密码在无记录时生效
- 会话：内存 token（`X-BoenMind-Session` 头），30 天；重启全员重登
- 密码落盘 `~/.boenmind/auth.json`（salt + SHA-256，明文不落盘）
- **UI 门是前端门**（未登录不能进聊天/编程/WIKI/设置）；API 面继续由
  `BOENMIND_TOKEN` 守卫，二者互不干扰。服务端不对 /api 强制会话——
  这是刻意范围，不是遗漏。

## 3. 核查结果：已完成 vs 缺失

### 3.1 后端（backend/crates/bm-server）

| 项 | 状态 |
|---|---|
| `routes/auth.rs`（login/status/logout/change_password + 内存会话 + 密码文件） | ✅ 已实现 |
| `routes/mod.rs` 声明 `pub mod auth` | ✅ |
| **挂载**：`lib.rs` `router()` 里 `.merge(routes::auth::router())` | ❌ **缺失** → `/api/auth/*` 全 404 |
| **豁免**：`auth_middleware` 对 `/api/auth/*` 放行 | ❌ **缺失** → 设了 BOENMIND_TOKEN 时连登录都进不去 |

### 3.2 前端（frontend/src）

| 项 | 状态 |
|---|---|
| `App.tsx` 登录门（web 查会话、未登录只渲染 LoginPage、desktop 直放行、主数据门后才加载） | ✅ |
| `components/auth/LoginPage.tsx`（UI） | ✅ |
| `components/settings/SecuritySettings.tsx`（改密+登出 UI） | ✅ |
| **`api/client.ts` 会话层**：`setUiSession` / `onUiUnauthorized` / `authStatus` / `authLogin` / `authLogout` / `changePassword` / `X-BoenMind-Session` 头注入 / `login required` 401 路由 | ❌ **整段缺失** → App/LoginPage/SecuritySettings 的 import 全是编译错误 |
| **`ClassicShell` 接 `onLogout` prop**（App.tsx 已传，组件签名无参） | ❌ → TS 报错 |
| **`boenmind:logout` 事件监听**（SecuritySettings 已 dispatch，无人接收） | ❌ → 安全页登出不会复位登录门 |
| **`app-registry.tsx` 注册 security 设置页**（SecuritySettings 已写，未登记） | ❌ → 设置菜单无入口 |
| **i18n**：`auth.*` 新文案 + `settings.security.*` + `settings.menu.security`（4 语言） | ❌ → zh 的 `auth` 段还是旧 TokenGate 文案 |

**结论：任务1 当前代码不可编译（前端缺失导出/多传 prop；后端路由不生效）。**

## 4. 收尾施工单（交 coder 实现，架构已定）

### 后端（两处）

**A1. `lib.rs` `router()` 挂载** —— 在现有 `.route(...)` 链中加入一行：

```rust
.merge(routes::auth::router())
```

**A2. `auth_middleware` 豁免 `/api/auth/*`** —— 在 `let Some(expected) = ... else { return next.run(request).await; };`
之前插入：

```rust
// /api/auth/* 免 token：密码本身就是浏览器入口守卫
if request.uri().path().starts_with("/api/auth/") {
    return next.run(request).await;
}
```

> 注意：`origin_middleware` 仍作用于 auth 路由（CSRF 兜底），前端 `authHeaders()` 已带
> `X-BoenMind-Client: 1`，无额外改动。

### 前端（五处）

**F1. `client.ts` 加会话层**（放在 TokenGate 的 `authToken` 段旁）：

```ts
// ── UI 登录门会话（公网站点；与 BOENMIND_TOKEN 的 Authorization 分离）──
let uiSession: string = (() => {
  try { return localStorage.getItem("boenmind.session") ?? ""; } catch { return ""; }
})();
const SESSION_KEY = "boenmind.session";

export function setUiSession(token: string) {
  uiSession = token.trim();
  try { if (uiSession) localStorage.setItem(SESSION_KEY, uiSession); else localStorage.removeItem(SESSION_KEY); } catch {}
}

let uiUnauthorizedHandler: (() => void) | null = null;
export function onUiUnauthorized(handler: (() => void) | null) { uiUnauthorizedHandler = handler; }
function notifyUiUnauthorized() { uiUnauthorizedHandler?.(); }
```

- `authHeaders()` 追加：`...(uiSession ? { "X-BoenMind-Session": uiSession } : {})`
- `request()` / `readSSEStream()` 的 401 分支：当 `status===401 && body.error==="login required"` 时
  调 `notifyUiUnauthorized()`（区别于 token 门的 `"unauthorized"` → `notifyUnauthorized()`）
- `api` 对象追加 4 个方法：

```ts
authStatus: () => request<{ authenticated: boolean }>("/api/auth/status"),
authLogin: (password: string) =>
  request<{ ok: boolean; token: string }>("/api/auth/login", { method: "POST", body: JSON.stringify({ password }) }),
authLogout: () => request<{ ok: boolean }>("/api/auth/logout", { method: "POST" }),
changePassword: (body: { current_password: string; new_password: string }) =>
  request<{ ok: boolean }>("/api/auth/password", { method: "PUT", body: JSON.stringify(body) }),
```

**F2. `ClassicShell` 接 `onLogout`**：签名改 `export function ClassicShell({ onLogout }: { onLogout?: () => void })`，
在底部导航区（设置图标旁）加一个登出按钮（web 才显示；desktop 不传 onLogout 即隐藏）。

**F3. `App.tsx` 监听 `boenmind:logout`**（SecuritySettings 已 dispatch 此事件）：

```ts
useEffect(() => {
  const onEvt = () => handleLogout();
  window.addEventListener("boenmind:logout", onEvt);
  return () => window.removeEventListener("boenmind:logout", onEvt);
}, []);
```

**F4. `app-registry.tsx` 注册 security 设置页**：

- `SettingsTab` 联合类型加 `"security"`
- `SETTINGS` 加一项（`component: SecuritySettings`，labelKey/descKey `settings.menu.security` / `settings.menu.securityDesc`，group `"system"`）
- 桌面版隐藏：把 `App.tsx` 里的 `isDesktopShell()` 抽到 `@/lib/desktop.ts` 复用，`SettingsMenu`
  过滤掉 `"security"`（桌面无登录门，该页无意义）

**F5. i18n（4 语言 zh/en/ja/ko）** 新增 key：

- `auth.login` / `auth.loginDesc` / `auth.passwordLabel` / `auth.passwordPlaceholder` / `auth.loggingIn` / `auth.wrongPassword`
  （保留现有 `auth.title/desc/placeholder`，那是 TokenGate 的）
- `settings.security.title` / `.desc` / `.changePassword` / `.changePasswordDesc` / `.currentPassword` /
  `.newPassword` / `.confirmPassword` / `.savePassword` / `.passwordMismatch` / `.passwordChanged` /
  `.session` / `.sessionDesc` / `.logout`
- `settings.menu.security` / `settings.menu.securityDesc`

## 5. 验收清单（coder 完成后按此核对）

1. `cargo check -p bm-server` 通过；`pnpm build`（或 `tsc --noEmit`）通过
2. 无 BOENMIND_TOKEN：浏览器访问 → 登录页；输 `adminadmin` → 进主界面
3. 设 BOENMIND_TOKEN 后：登录页可进（豁免生效）→ 主界面数据 401 → TokenGate 弹窗
4. 设置中心「安全」页：改密成功（新密码 ≥4 位）；改密后当前会话仍有效
5. 安全页/壳层登出 → 回登录页；刷新后仍为未登录（服务端已删 token）
6. 桌面版（Tauri）：不出现登录页与「安全」设置项
7. 4 语言文案齐全

## 6. 遗留与后续加固（本任务不阻塞，记录在案）

- 密码哈希为 SHA-256+salt，未做 PBKDF2/argon2 拉伸；改密时未使其他会话失效。
  公网暴露且密码是唯一 UI 门时，建议后续升级（低优先级，先保闭环）。
- 会话仅内存（重启全员重登）——已写入设计，接受。
- `/api/auth/status` 未设限流；暴力破解防护可后续加（IP/失败计数），先记录。

## 7. 二次核验记录（2026-08-17 收尾轮，源码级确认）

### 7.1 施工单 A1–A2 / F1–F5 全部落地 ✅（逐项核对）

| 项 | 证据 |
|---|---|
| A1 挂载 | `lib.rs:231` `.merge(routes::auth::router())` |
| A2 豁免 | `lib.rs:439-442` `/api/auth/*` 先于 token 校验放行 |
| F1 会话层 | `client.ts` `uiSession`/`setUiSession`/`onUiUnauthorized` + `authHeaders()` 注入
  `X-BoenMind-Session`；`request()`/`readSSEStream()` 401 分流（`unauthorized`→TokenGate、
  `login required`→登录门）；`api.authStatus/authLogin/authLogout/changePassword` 就位 |
| F2 onLogout | `ClassicShell.tsx` 签名接 `onLogout?: () => void`，底部登出按钮（`App.tsx:134`
  仅 web 传值）；`boenmind:logout` 事件 `App.tsx:90-92` 监听 |
| F4 设置注册 | `app-registry.tsx` `SettingsTab` 含 `"security"`，`SETTINGS.security` 注册
  （`component: SecuritySettings`，`desktopHidden: true`）；`SettingsMenu.tsx:49` 过滤桌面隐藏项 |
| F5 i18n | zh/en/ja/ko 四语言 `auth.login*`、`settings.security.*`、`settings.menu.security*` 齐备 |
| 桌面豁免 | `lib/desktop.ts` `isDesktopShell()`（`__TAURI_INTERNALS__`）由 App/注册表/安全页共用 |
| `.gitignore` | 追加 `var/`（运行时数据 `.boenmind` 镜像不入库） |

### 7.2 架构评审观察（记录，不改设计）

- **安全边界（既定）**：UI 门是前端门；后端 `auth_middleware` 仅当设置了 `BOENMIND_TOKEN`
  才校验 Bearer，未设 token 时非 `/api/auth/*` 全部放行（lib.rs:439-442）。这是第 2 节
  **刻意范围**，但意味着：公网监听 `0.0.0.0` 且未设 token 时，API/聊天记录对未认证者可见。
  现有启动警告（lib.rs:980 `!local.ip().is_loopback() && !has_token`）已提示。**建议公网部署
  必设 BOENMIND_TOKEN 或经反向代理加访问控制**；若要服务端也强制 UI 会话，可后续在
  `auth_middleware` 加「非回环监听时校验 `X-BoenMind-Session`」分支（本任务不做）。
- **后续加固（延续第 6 节）**：改密时未作废其他会话；无登录限流；SHA-256 未拉伸。
- **验收缺口**：构建（`cargo check` / `tsc --noEmit`）与 git 提交需在宿主环境执行，
  本工作区静态核对无法覆盖——交 coder 在宿主完成并按第 5 节验收。

### 7.3 收尾核验记录（2026-08-17 二轮，编译依赖确认）

- **依赖确认 ✅**：`bm-server/Cargo.toml` 已含 `sha2 = "0.10"` 与
  `uuid = { version = "1", features = ["v4"] }`（auth.rs 的 `Sha256` 与
  `Uuid::new_v4()` 所需），无缺依赖风险。
- **符号确认 ✅**：auth.rs 引用的 `crate::{ApiResult, api_error}`
  （lib.rs:1120/1151）、`bm_core::config::app_dir()`（bm-core config.rs:271）、
  `crate::AppState`（lib.rs:68 `pub struct`）均存在；axum/tokio/serde 依赖齐备。
- **路由/中间件确认 ✅**：lib.rs:231 挂载、lib.rs:248 应用 auth_middleware、
  lib.rs:439-442 豁免 `/api/auth/*`（先于 BOENMIND_TOKEN 校验放行），与 7.2 记录一致。
- **遗留**：静态核验可排除缺依赖/缺符号类编译错误，但 `cargo check -p bm-server`
  与 `pnpm build` 仍须由 coder 在宿主环境跑通（第 5 节验收清单 1）。

### 7.4 三轮回溯核验记录（2026-08-17 三轮，全部施工单点逐项在位）

本轮把施工单 A1–A2 / F1–F5 全部点重新源码级复核一遍，无一遗漏：

| 项 | 证据（本轮重新确认） |
|---|---|
| A1 挂载 | `lib.rs:231` `.merge(routes::auth::router())`；`routes/mod.rs:5` `pub mod auth` |
| A2 豁免 | `lib.rs:439-442` `/api/auth/*` 先于 BOENMIND_TOKEN 校验放行 |
| F1 会话层 | `client.ts:489` `uiSession`、`:499` `setUiSession`、`:523` 注入 `X-BoenMind-Session`、`:544/:579` 401 分流（`login required`→`notifyUiUnauthorized`）、`:622/:624/:630/:632` `authStatus/authLogin/authLogout/changePassword` |
| F2 onLogout | `ClassicShell.tsx:44` 签名、`:136-141` 登出按钮（`onLogout &&` 条件渲染） |
| F3 事件 | `App.tsx:91-92` `boenmind:logout` addEventListener + cleanup |
| F4 注册 | `app-registry.tsx:143` `SettingsTab` 加 `"security"`、`:213-215` `SETTINGS.security`、`:218` `desktopHidden:true`；`SettingsMenu.tsx:49` 过滤 `desktopHidden` |
| F5 i18n | zh/en/ja/ko 四语言 `auth.login/loginDesc/passwordLabel/wrongPassword`、`settings.security.*`、`settings.menu.security*` 齐备（zh.ts:8-10/:308-309/:452-465） |
| 登录门 | `App.tsx:46` `uiAuthed` 初始=desktop、`:119-124` 未登录只渲染 `LoginPage`、`:99` 主数据门在 `uiAuthed` 后 |
| 桌面豁免 | `lib/desktop.ts:5` `isDesktopShell()`；App/注册表/SecuritySettings 共用 |
| `.gitignore` | 末尾含 `var/` |

**结论**：代码改动完整闭环，静态层面无缺依赖/缺符号/断 import。剩余动作仅剩
宿主机侧的「编译验证 + git 提交 + 推送」（见第 8 节）。

### 7.5 四轮核验记录（2026-08-17 新会话独立复核，确认终态稳定）

新会话对 A1–A2 / F1–F5 全部落点重新源码级复核，全部在位，与 7.4 一致；另补录
`components/auth/LoginPage.tsx`、`components/settings/SecuritySettings.tsx` 的
`auth.*` / `settings.security.*` 消费点（非注册点）存在性：

| 项 | 本轮证据 |
|---|---|
| A1 挂载 | `lib.rs:231` `.merge(routes::auth::router())` |
| A2 豁免 | `lib.rs:439-442` `/api/auth/*` 先于 BOENMIND_TOKEN 校验放行 |
| F1 会话层 | `client.ts:489` `uiSession`、`:500` `setUiSession`、`:511` `onUiUnauthorized`、`:523` 注入 `X-BoenMind-Session`、`:544/:579` 401 分流（`login required`→`notifyUiUnauthorized`）、`:624` `authLogin` 等 4 方法 |
| F2 onLogout | `ClassicShell.tsx:44` 签名 + `:136-141` 登出按钮（`onLogout &&`） |
| F3 事件 | `App.tsx:91-92` `boenmind:logout` addEventListener + cleanup |
| F4 注册 | `lib/app-registry.tsx:143` `SettingsTab` 含 `"security"`、`:213-215` `SETTINGS.security` |
| F5 i18n | 四语言 `auth.loginDesc`（zh.ts:9 等）与 `settings.security.securityDesc`（zh.ts:309 等）、`settings.security.logout`（zh.ts:465 等）齐备；`LoginPage.tsx:44` / `SecuritySettings.tsx:56` 消费点存在 |
| 桌面豁免 | `lib/desktop.ts:5` `isDesktopShell()` |
| `.gitignore` | `:34` `var/` |
| git 状态 | HEAD=main=39ec59c；`.git/logs/HEAD` 末条为 `pull --ff-only`（无本地提交），改动全在工作区未暂存 |
| 代理/执行 | 无 `pi/` 或 `.pi/` agent 定义 → subagent/coder 不可用；无 shell → 构建与提交须宿主执行 |

**结论**：与 7.4 完全一致，终态无漂移。剩余动作仍为第 8 节宿主侧命令。

## 8. 交付命令清单（宿主执行；本工作区无 shell/git 工具，由宿主跑通）

> 本工作区（架构师只读环境）无 shell/git 执行通道，subagent（coder 等）亦不可用，
> 因此 commit/push 须在宿主仓库目录 /var/lib/boenmind/workspaces/BoenMind 执行。

```bash
cd /var/lib/boenmind/workspaces/BoenMind

# 1) 构建验证（先证编译再提交）
cargo check -p bm-server
pnpm --dir frontend build          # 或 cd frontend && tsc --noEmit

# 2) 确认改动集（预期：auth.rs / client.ts / App.tsx / ClassicShell.tsx /
#    LoginPage.tsx / SecuritySettings.tsx / app-registry.tsx / desktop.ts /
#    SettingsMenu.tsx / i18n 四语言 / .gitignore / 本文档）
git status --short

# 3) 暂存并提交
git add -A
git commit -m "feat(auth): 公网站点密码登录门（adminadmin 默认，设置可改）

- 后端：/api/auth/{status,login,logout,password} 内存会话 + SHA-256 密码落盘；
  auth_middleware 豁免 /api/auth/*（BOENMIND_TOKEN 仍守卫其余 API）
- 前端：web 登录门（未登录只渲染 LoginPage）；client.ts 会话层
  （X-BoenMind-Session 注入、401 login required 复位）；ClassicShell 登出按钮；
  SecuritySettings 设置页注册（桌面隐藏）；i18n 四语言
- docs: AUTH_LOGIN_GATE_WIRING_2026-08-17.md（核查 + 施工单 + 核验记录）"

# 4) 推送
git push origin main
```

**提交后按施工单第 5 节验收清单回归**（无 token 登录、有 token 登录门豁免+TokenGate、
改密、登出复位、桌面豁免、四语言）。若 `cargo check`/`pnpm build` 报错，回到第 4 节
按施工单逐项修，改动均为最小增量。
