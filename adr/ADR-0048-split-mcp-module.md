---
status: accepted
date: 2026-09-12
summary: 拆 bm-providers/src/mcp.rs(2182→1056 行)——按 banner 边界抽出 shape/transport_http/transport_stdio 三模块,公共路径经 pub use 不变
supersedes: []
superseded_by: []
---

# ADR-0048: 拆分 bm-providers/src/mcp.rs(四职责合一的巨物) 
- 关联: ADR-0046(反转补完,本为其 P3)、ADR-0016/0034(MCP 接入与 SDK) - 背景: `bm-providers/src/mcp.rs` 达 **2182 行**,把**传输(stdio/HTTP-SSE)+ 数据形状 + 子进程生命周期 + 完整性校验 + Hub 路由 + 异步执行器**六个关注点合于一文件——是全项目最大文件,也是"东一个补丁西一个补丁"观感的主要来源之一。 
## 决策 
**按文件内既有的 banner 边界机械抽取为子模块**,公共路径经 `pub use` 保持不变(调用方零改动): 
``` src/mcp.rs 核心:stderr 缓冲 + 传输端口 trait + Hub + 完整性 + 装载 src/mcp/shape.rs (167) 数据形状:McpToolDef/normalize_*/tool_manifest(+ 其测试) src/mcp/transport_http.rs (301) HTTP/SSE 远程传输(+ 其测试) src/mcp/transport_stdio.rs (671) stdio 子进程传输:spawn/重生/帧写入/进度聚合(+ 其测试) src/mcp/supervisor.rs (194) 装配 supervisor(既有,未动) ``` 
- 子模块用 `use super::*` 摄取父模块符号,父模块 `pub use <mod>::*` 回导出——**公共 API 面逐字不变**(已校验 15 个公有符号全部在场)。 - 跨模块共用的 `KNOWN_PROTOCOL_VERSIONS`(握手协商 + SSE 解析共用)上移父模块并 `pub(crate)`。 - 测试随其被测物移动:`x05_tests`→shape、`remote_http_tests`→transport_http、`progress_gen_tests`(访问 stdio 私有字段 `alive`)→transport_stdio。 
## 后果 
- `mcp.rs` **2182 → 1056 行**(其余四模块 167/194/301/671);关注点从"六合一"降为"Hub 核心 + 传输实现分离"。 - **零行为变更**:501 测试全绿;`clippy -D warnings` 零警告;公共路径不变(runtime/surface 等调用点零改动)。 - 采用 Rust 2018 无 `mod.rs` 风格(`mcp.rs` + `mcp/` 子目录),未引入目录重组风险。 - 过程中修复:抽取时的孤儿文档注释、边界错位(用花括号配平自动定位模块结束行,替代手数行号)。 