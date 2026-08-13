# bm-compat 依赖图谱（B1 前置分析，2026-08-14）

来源：`legacy/pi_agent_rust/src/`（vendored 基线 44ddf80 + 台账补丁）。

## 六文件规模

| 文件 | 行数 | 说明 |
|---|---|---|
| extensions_js.rs | 33,945 | QuickJS 运行时主体：`PiJsRuntime`（:16629）、`HostcallKind`（:175）、ExtensionBody 协议执行 |
| scheduler.rs | 4,591 | 确定性调度器（`MacrotaskKind`/`Macrotask`/`DeterministicClock`）——**crate 内零引用**，自包含 |
| hostcall_queue.rs | 2,180 | hostcall 请求队列——仅依赖 `hostcall_s3_fifo` |
| hostcall_io_uring_lane.rs | 1,085 | io_uring 通道——**crate 内零引用**，自包含 |
| embedded_assets.rs | 448 | LZSS 压缩的内嵌 JS 资产（lzss_compress/decompress） |
| error.rs | 2,873 | 错误类型——仅依赖 `provider_metadata::provider_auth_env_keys`（4 处） |

合计 45,122 行。

## extensions_js.rs 的 crate 内依赖（全表）

| 依赖模块 | 引用次数 | 具体符号 |
|---|---|---|
| extensions | 40 | `safe_canonicalize`(35)、`hostcall_params_hash`(2)、`strip_unc_prefix`(1)、`PolicyProfile`(1) |
| tools | 5 | `kill_process_group_tree`、`isolate_command_process_group`、`command_with_default_sigpipe_in_dir`、`SIGPIPE_TRAMPOLINE_EXEC_FAILURE_PREFIX` |
| scheduler | 4 | `MacrotaskKind`、`Macrotask`、`DeterministicClock` |
| embedded_assets | 3 | `lzss_decompress`、`lzss_compressed_len`、`lzss_compress` |
| pi_wasm | 2 | wasm-host feature（WASM polyfill） |
| http_shim | 2 | HTTP hostcall 垫片 |
| crypto_shim | 2 | 加密 hostcall 垫片 |
| hostcall_queue | 1 | 队列结构 |
| hostcall_io_uring_lane | 1 | io_uring 通道 |
| error | 1 | 错误类型 |
| buffer_shim | 1 | 缓冲区垫片 |

## 拷入策略（下一轮执行）

1. **6 文件整体拷入** `bm-compat/src/`（保持文件名）；
2. **shim 层**：`extensions / tools / pi_wasm / http_shim / crypto_shim / buffer_shim /
   provider_metadata / hostcall_s3_fifo` 是 extensions_js 的外部依赖——
   逐符号最小化提取（约 10 文件、<2K 行），或首版以内联 stub 替代并标记 TODO；
3. **5 符号**（拆法 A 的"拷 5 符号"）：`ExtensionPolicyMode`(extensions.rs:2029)、
   `ExtensionPolicy`(:2046)、`PolicyProfile`、`HostcallKind`(extensions_js.rs:175)、
   `PiJsRuntime`(:16629)——后两者随 extensions_js.rs 自带，前两者从 extensions.rs
   提取最小类型层（连同其 serde 形状，前端权限档位依赖）。
4. **B2 host 线程**落点：`HostcallKind` 分发 → `complete_hostcalls_batch` →
   `tick`（extensions_js.rs 内现成函数，抽作公开入口）。

## 上游同步纪律

见 `backend/legacy/UPSTREAM_PATCHES.md`「bm-compat 同步」区。要点：
上游升级 → 6 文件逐文件 diff → 同步到本 crate → 补丁（P 编号）两边一致。
