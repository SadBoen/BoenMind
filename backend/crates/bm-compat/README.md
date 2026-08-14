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
- [x] 6 文件实际拷入（45,122 行逐字节一致，diff 验证通过）
- [x] 5 符号提取（ExtensionPolicyMode/ExtensionPolicy/PolicyProfile 自
      extensions.rs 提取最小类型层，连同 serde 形状与传递闭包；HostcallKind/
      PiJsRuntime 随 extensions_js.rs 自带）
- [x] shim 层就位（extensions/tools/provider_metadata/provider 提取 + 
      http_shim/crypto_shim/buffer_shim/hostcall_s3_fifo 整文件照抄）
- [x] build.rs 精简版（仅 gzip 资产生成）+ 4 资产拷入 + lib.rs/Cargo.toml
- [x] **B2 host 线程**（`src/host.rs`）：`drain_hostcall_requests → policy 裁决
      + HostcallKind 分发（HostServices 六端口）→ complete_hostcalls_batch →
      tick → 二轮补收`，结构镜像 legacy `pump_js_runtime_once_for_owner`；
      `check_capability` 三模式裁决 + `request_approval` fail-closed 询问口（B5）；
      测试在 `tests/host.rs`（集成测试，7 用例全绿：六 kind 路由 / policy 拒绝 /
      审批放行 / QuickJS 真链路 eval→pump→complete）
- [x] **入 workspace members + CI 门禁**：backend/Cargo.toml members 登记；
      CI 加 test（--test host）与 clippy（--lib --test host -D warnings）两行；
      B1 存量 lint（死代码/unused import/collapsible_if，均在拷贝/shim 文件）
      经 manifest [lints] 表放行（红线：拷贝文件逐字节一致，B3/B4 接上后收紧）
- [ ] B3 加载路径（eval_file + get_registered_tools + ExtensionBody 协议注册）
- [ ] B4 与内核接线（QuickJS 运行时 = 内核插件；工具注册进 bm-loop ToolRegistry）

## 上游同步纪律

本 crate 的 6 文件与 legacy/pi_agent_rust 同源。上游升级（vendor 更新）时：
1. 对 6 文件逐文件 diff，把上游变更同步到本 crate；
2. 本地补丁（P 编号）在两者保持一致；
3. 台账更新 backend/legacy/UPSTREAM_PATCHES.md「bm-compat 同步」区。

### B1 偏离 / 补丁登记（与 legacy 逐字节 diff 的例外）

| 项 | 处理 | 说明 |
|---|---|---|
| 6 拷贝文件 | **零偏离**，diff 逐字节一致 | extensions_js/scheduler/hostcall_queue/hostcall_io_uring_lane/embedded_assets/error |
| 整文件 shim | **零偏离**，diff 逐字节一致 | http_shim/crypto_shim/buffer_shim/hostcall_s3_fifo |
| extensions.rs shim | 提取（新文件，非整文件拷贝） | 从 legacy extensions.rs 按行段 verbatim 提取：193-298（类型）、901-906+910-1054（exec mediation 调用面）、1364-1472（canonicalize）、1539-1625（hash）、1685-1699（envelope）、1941-2081（PolicyProfile/Override/Mode/Policy）、2384-2447（QuotaConfig）；exec_mediation.rs 整文件以 `mod exec_mediation { … }` 内联，其 `use super::{…}` 与 `pub(super)` 原样保留 |
| tools.rs shim | 提取（新文件） | 9720-9798 + 9800-9927 行段 verbatim（kill_process_group_tree 传递闭包 + SIGPIPE trampoline + isolate） |
| provider_metadata.rs shim | 提取（新文件） | 1-1666 行段 verbatim（类型 + 全量 PROVIDER_METADATA 表 + 两个访问器）；上游 `use crate::provider::InputType;` 经本地 `src/provider.rs` shim 解析（InputType 枚举自 provider.rs:194-202 verbatim 提取） |
| build.rs | 精简 | 只保留 gzip 资产生成；vergen-gix 与 benchmark fingerprint 删除 |
| Cargo.toml | standalone | 本 crate 位于 backend/ 子树内，cargo 默认会向上找 backend/Cargo.toml workspace；为避免动 backend/Cargo.toml 的 members/exclude，本 manifest 内加了空的 `[workspace]` 表使自己成为独立 workspace 根（target/ 与 Cargo.lock 均落在 crates/bm-compat/ 下） |
| embedded_assets.rs 421-445 | **不改** | `include_bytes!("../…")` 相对路径全在 `#[cfg(test)]`（312 行起）内，默认 cargo check 不编译；4 个源资产已按 legacy 相对路径拷入 crate 根，未来开 `--tests` 时路径可直接解析 |

## 待办（TODO）

1. **测试目标未编译**：验收只跑默认 `cargo check`（无 `--tests`）。拷贝文件内大量
   `mod tests` 是后续议题；其中 embedded_assets.rs 测试用 `crc32c` 需要补
   dev-dependency，且引用 `../docs/evidence/…` 等 legacy 外资产。
2. **wasm-host feature**：extensions_js.rs 对 `crate::pi_wasm` 的引用全部在
   `#[cfg(feature = "wasm-host")]` 内，lib.rs 以同名 cfg 声明 `mod pi_wasm`。
   启用前需拷入 pi_wasm.rs + wasmtime 依赖（已预置 optional dep）。
3. **shim 与上游同步**：extensions/tools/provider_metadata shim 的行号头注释
   基于当前 vendored 基线；上游变更后按头注释行号重提取。
4. **B3/B4**：eval_file 加载路径 / ExtensionBody 协议注册 / 内核接线（下一步）。
5. ~~加入 workspace~~（✅ B2 完成时已入 members + CI 门禁，见「当前状态」）。

