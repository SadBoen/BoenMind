# bm-compat（主线 B：pi-compat 拆法 A）

目标：把 vendored QuickJS 引擎**作库**接入自研 loop，pi.dev 200+ 插件当日兼容。
BoenMind 自研 loop（A6）落位后，pi 引擎整体退场；本 crate 是过渡兼容层。

## 拆法 A 任务分解（docs/HANDOFF_KERNEL_PHASE1.md §五）

- **B1 拷入 6 文件 + 5 符号**：`extensions_js.rs / scheduler.rs / hostcall_queue.rs /
  hostcall_io_uring_lane.rs / embedded_assets.rs / error.rs`（共 45K 行，
  legacy/pi_agent_rust/src/）+ `ExtensionPolicy` 等 5 符号（见 DEPENDENCIES.md §5 符号）。
- **B2 host 线程 ~300 行**：`drain_hostcall_requests → HostcallKind 分发 →
  complete_hostcalls_batch → tick`。
- **B3 加载路径**：`eval_file` + `get_registered_tools` + ExtensionBody 协议注册。
- **B4 与内核接线**：QuickJS 运行时 = 插件（ctx 注册 `quickjs` 服务）；
  工具注册进 A6 自研 loop 的工具注册表。
- **B5 权限询问桥接**：现有 PermissionBridge（vendor P5）复用为 approval 服务。
- **B6 验收**：现有 TS 插件（web_search/web_fetch/ctx-compactor）安装 →
  工具可见可调 → 权限弹窗全链路。

## 当前状态（2026-08-14）

- [x] 依赖图谱分析完成（DEPENDENCIES.md：6 文件 → 仅 8 个 crate 内模块，
      其中 scheduler/queue/lane 几乎自包含）
- [ ] 6 文件实际拷入（45K 行机械工作 + shim 适配，下一轮）
- [ ] 5 符号提取（ExtensionPolicyMode/ExtensionPolicy 在 extensions.rs 巨文件中，
      需抽最小类型层）
- [ ] B2 host 线程 / B3 加载路径
- [ ] 本 crate 加入 workspace members + CI 门禁

## 上游同步纪律

本 crate 的 6 文件与 legacy/pi_agent_rust 同源。上游升级（vendor 更新）时：
1. 对 6 文件逐文件 diff，把上游变更同步到本 crate；
2. 本地补丁（P 编号）在两者保持一致；
3. 台账更新 backend/legacy/UPSTREAM_PATCHES.md「bm-compat 同步」区。
