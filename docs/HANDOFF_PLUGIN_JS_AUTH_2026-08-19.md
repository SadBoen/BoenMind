# HANDOFF：JS 插件管理面 RPC + 认证插件（2026-08-19）

> 状态：**已落地并全绿**。JS 插件管理面（`plugin.js.list`/`plugin.js.invoke`）接通；
> 认证插件 `plugin-auth`（AuthPort：登录/登出/改密/会话）落地，web-server `--auth`
> 启用登录门控（敏感方法 + 全部 RPC 需会话 token）。
> 多插件实测：5 个 JS 插件（含同名覆盖/无 host 面）全链路 curl 通过；登录门控
> 全链路（未登录拒绝 → 登录 → 授权 → 改密 → 登出失效）curl 实测通过。
> workspace + clippy + gate1 全过。下轮：前端登录页 + 设置中心「安全」页改密。

---

## 1. 一句话交接

**万物皆插件**落地两块：① JS 插件从"装配但不执行"到**可管理**——新增
`plugin.js.list`（列已装配插件 + faces 最小权限展示）与 `plugin.js.invoke`
（执行插件主函数 `__main`）；② **安全尽早做**——dsh/cordis 生态均无认证插件
（调研确认），吸收 BoenMind 旧 `backend/.../routes/auth.rs`（用户已接受方案）
升级为 Rust 认证插件 `plugin-auth`，web-server `--auth` 开启登录门控。

## 2. 落地清单（kernel 95ab265 + 主仓 a14f9ae）

| 部分 | 内容 |
|---|---|
| kernel-contracts | `AuthPort`（`is_authenticated`/`login`/`logout`/`change_password` + `AuthResult`）——认证落成可变策略，未装配 fail-loud |
| plugins/plugin-auth | 认证插件（category=Feature）：默认密码 `adminadmin`、**PBKDF2-SHA256**（盐 + 210k 迭代，升级旧 auth.rs 的裸 SHA-256）、内存会话 token（30 天）、`auth.json` 持久化（salt+hash，明文不落盘）、常数时间比较。5 测试 |
| bm-assembly | `Runtime.auth: Option<Arc<dyn AuthPort>>` + `install_auth`/`install_default_auth`（L0 不依赖 plugin-auth，经组合根装配）；`plugin_manifest()` 追加 auth（Feature） |
| web-server | `--auth` 开关；dispatch **认证门控**（`auth_methods()` 白名单：auth.* + host.describe 放行，其余 fail-closed `auth-required`）；`auth.status/login/logout/changePassword` RPC；`plugin.js.list/invoke` RPC（invoke 经 `spawn_blocking` 脱离 tokio 上下文执行 JS 引擎） |
| 边界守卫 | plugin-auth 登记 layer 2 |

## 3. 验证矩阵（全绿）

| 项 | 结果 |
|---|---|
| workspace | 全过（plugin-auth 5 / web-server 22 / bm-assembly 22 / quickjs-bridge 32…） |
| clippy `-D warnings` | 零警告 |
| gate1 | ALL PASS |
| **多插件实测**（release 起服 5 插件） | `plugin.js.list` 5 条（counter 无面/echo 3 面/greeter 同名覆盖 Override 2.0.0/hello/toolbox）；invoke 逐个执行成功；未知插件/缺 pluginId 错误路径正确 |
| **登录门控实测**（release `--auth`） | 未登录所有 RPC auth-required；错误密码 wrong-password；登录签发 token；带 token 放行；改密后旧密码失效新密码生效（auth.json 落盘）；登出后 token 立即失效；auth.json 仅 salt+hash 无明文 |

## 4. 关键机制与坑（下轮不重踩）

- **JS 插件执行线程纪律**：`JsBridge::call` 内部 block_on 自带 runtime，**必须经
  `tokio::task::spawn_blocking` 脱离 tokio 上下文**（否则 "Cannot start a runtime
  from within a runtime"）。web-server 测试同理：装配 JS 插件用 `#[test]`（同步），
  invoke 用 `#[test]` + 手建 tokio runtime（`Builder::new_current_thread().block_on`），
  不用 `#[tokio::test]`。
- **边界守卫**：L0（web-server）**禁止依赖 plugin-\***（含 dev-deps）——认证装配
  收敛在 bm-assembly（`install_default_auth`），web-server 只调组合根 + re-export 常量。
- **AuthPort 失败模型**：登录失败返回 `Ok(AuthResult::failure(code))`（非 Err）——
  "密码错"是业务结果不是端口故障；端口 `Err` 留给 IO/后端故障（fail-loud 纪律）。
- **密码哈希**：PBKDF2-SHA256（210k 迭代，OWASP 建议 ≥600k，本地单用户取平衡值）；
  盐 = UUID v4 hex；比较用常数时间（长度先行 + XOR 累加）。
- **认证门控范围**：`auth_methods()` = auth.* + host.describe（前端启动先查状态）；
  其余全部门控。**改密需会话内校验**（当前密码 + 新密码），未登录改密 → login-required。

## 5. 下轮指针

1. **前端登录页**：dsh 前端快照无登录页（官方本地单用户无认证）——需自建登录
   UI：未登录显示密码输入，登录后存 token 到 localStorage，请求带
   `x-boenmind-session` 头；`auth.status` 探测。设置中心「安全」页挂
   `auth.changePassword`。
2. **token 持久化**：当前会话内存态（重启全员重登）——前端 localStorage 持有
   token 跨刷新；若要跨重启保会话，auth.json 可加 sessions 段（低优先）。
3. **速率限制/防爆破**：登录接口目前无速率限制——LAN/公网部署前加（简单
   失败计数 + 退避即可，不引第三方）。
4. **dsh-rust-plugins 吸收**：按台账 `docs/PLUGIN_ABSORPTION_LEDGER_2026-08-19.md` 流程。

## 6. 环境纪律（沿用）

- 每轮先杀 web-server：`taskkill //F //IM web-server.exe`
- 跑服务用 release（debug exe 2GB 超 PE 限制）
- 验证三件套：cargo test / clippy / gate1
- rquickjs 版本锁 0.6.2（crates.io 0.12.2 不在本地缓存，离线不可用）
