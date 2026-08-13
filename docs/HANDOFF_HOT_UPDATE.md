# HANDOFF: 热升级（自更新）实施纪要

> 2026-08-13 · 完成并推送（commit `e29c7eb`）· 对应 HANDOFF_PRIME_AGENT_ADVANTAGES.md 遗留待办 #1

## 一、需求澄清（关键）

用户说的"静默升级" = **热升级**：点一下升级，**不重装软件、最好不退出程序**。
不是 Tauri updater 的"下载安装包→退出重装"模式（该模式 v0.1.1 已就绪但用户明确不想要）。
拍板结论：
- 桌面壳改「管家模式」✅ / Docker 不参与（维持 docker pull）✅ / 秒级自重启 ✅
- **无任何自动检查/提示**：只有用户主动点「检查更新」才查（用户明确要求）

## 二、架构（核心事实）

- 服务器版整个应用 = **1 个 bm-server 二进制**（`--features embed` 内嵌前端 dist + 插件）
  → "更新" = 替换这一个文件
- 桌面版（Windows/macOS）bm-server 用 `--features embed-plugins` 构建：仅内嵌内置插件
  （backend/plugins 三个目录），不内嵌前端（页面由壳提供）；便携包与热升级 runtime
  均自带插件，启动时自动写出到 ~/.boenmind/extensions/ 并默认启用
- 运行模式（`BOENMIND_MANAGED=1`，壳 spawn 子进程时设置）：
  - **standalone**（Linux 裸进程/systemd）：原子替换自身 + `exec` 重启（**PID 不变** → systemd 无感知）
  - **managed**（桌面壳子进程）：新二进制落盘 `~/.boenmind/runtime/bm-server-<ver>-<triple>`
    （版本号命名不覆盖 → 天然回滚），壳换新版重启，窗口不关

## 三、链路

1. 用户点「检查更新」→ `GET /api/updates/check` → GitHub Releases API → 按平台选资产
   `boenmind-runtime-<ver>-<triple>[.exe]` → 与当前版本比较
2. 点「立即升级」→ `POST /api/updates/apply`：下载 → **验签**（失败拒绝）→ 落盘
   （standalone：备份旧版 + rename 替换自身 + 写 `.update-pending.json`）
3. 重启：桌面版前端调壳 `backend_restart`（kill/shutdown → 监控循环按最新版拉起）；
   Linux 部署 `POST /api/updates/restart`（延迟 300ms exec，先返回响应）
4. 前端轮询 health 检测版本=目标 → `window.location.reload()` 完成
5. 崩溃兜底：启动时 `consume_pending_update` 检测标记 → exec 完成升级

## 四、实测结论（全部 ✅）

- **签名链路**：tauri signer 真实签名 → Rust 验签 OK；篡改 1 字节 → 拒绝；
  单行 base64 包装与明文多行两种 .sig 格式都支持
- **check**：真实 GitHub 响应解析（当前 0.1.1 = 最新 → latest: null）
- **apply**：已是最新 → 400；running 任务 → 409 拒绝（单测覆盖）
- **restart**：exec 重启 PID 不变、health 秒级恢复
- **pending 兜底**：标记被消费、日志确认 exec 路径
- **managed**：启动正常、restart 被拒（提示由壳重启）
- 单测 67+10 全过、clippy 清零、前端 tsc/build 通过

## 五、关键格式结论（踩坑记录）

- **tauri signer CLI 的 `--private-key` 接受单层 base64**（= 密钥文件内容），
  而 CI secret `TAURI_SIGNING_PRIVATE_KEY` 是双重 base64（= base64 密钥文件）
  → CI 里必须先 `base64 -d` 一层再传（release.yml 三处已按此修正）
- **minisign/tauri 签名体 = 74 字节**：`"ED"(2B) + key_id(8B) + ed25519 sig(64B)`，
  不是 72 字节（key_id 无前缀）
- **签名消息 = 文件内容的 Blake2b-512 digest**（ed25519 普通 verify，非 RFC8032 prehash）
- tauri .sig 文件 = 整体单行 base64（解码后才是多行 minisign 文本）
- 公钥 key_id 在公钥字节偏移 2..10（little-endian 存储），与 untrusted comment 里
  的 hex 显示（大端）相反，字节比较无需转换

## 六、文件清单

| 文件 | 说明 |
|---|---|
| backend/crates/bm-core/src/updates.rs | **新**：check/验签/下载/落盘/替换 + 6 单测 |
| backend/crates/bm-server/src/routes/updates.rs | **新**：check/apply/restart 端点 |
| backend/crates/bm-server/src/lib.rs | serve_managed 优雅关闭、exec_self、consume_pending_update、路由 |
| backend/crates/bm-server/src/main.rs | 启动时消费 pending 标记 |
| backend/crates/bm-core/src/db.rs | has_running_tasks + 单测 |
| frontend/src-tauri/src/lib.rs | **壳管家模式**：runtime 扫描/spawn/监控/backend_restart + 单测 |
| frontend/src/components/settings/AboutSettings.tsx | 热升级流程（移除 plugin-updater 调用） |
| frontend/src/api/client.ts | UpdateCheckInfo/ApplyUpdateResult + 3 API |
| frontend/src/i18n/locales/*.ts | 四语（upgradeNow/restarting/restartTimeout） |
| .github/workflows/release.yml | 三平台 runtime 资产 + 签名 |
| README.md / packaging/linux/README.md | 热升级说明 |

## 七、遗留（下次会话）

1. **v0.2.0 发布时全真验证**：完整 apply→验签→替换→exec 链路需真实新版本资产
   （本次各环节已分别实测；发布后应跑一遍端到端 + 桌面版壳重启实测）
2. 桌面壳 GUI 级实测未做（需 tauri dev/build 环境）：壳的 spawn/监控/backend_restart
   逻辑已单测 + 代码审查，建议发布后真机过一遍
3. `gui-test-screenshots/`（5 张）与工作区 `.tmp/` 测试残留可清理
4. 上游 P9（#163）/P10（#164）若合入按台账删除对应补丁
