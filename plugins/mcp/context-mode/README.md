# context-mode Rust MCP

BoenMind 官方可选 MCP 插件：单文件 Rust 服务，提供 FTS5/BM25 上下文检索、会话快照/恢复和受限宿主命令执行。

## 安装

将发布包中的 `plugins/context-mode` 复制到数据目录的 `mcp/`，在网页 **设置 → MCP/插件 → 扫描插件 → 批准接入**，再执行 reload。它默认不启用，首次调用执行类工具仍按 Broker 审批。

## 配置

批准后服务端配置文件为 `config/mcp-context_mode.json`：

```json
{
  "data_dir": "/home/me/.local/share/boenmind/context-mode",
  "allowed_roots": ["/home/me/work"],
  "max_file_bytes": 1048576,
  "max_files": 5000,
  "max_output_bytes": 262144,
  "default_timeout_ms": 30000,
  "execution_enabled": false}
```

插件本体不需要 Node/Python。`ctx_execute*` 只调用用户机器上已有的程序；缺失运行时会返回 `runtime_unavailable`，不会自动下载依赖。

## 开发

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## 许可证说明

本 Rust 实现为独立重写，不复制上游代码。设计参考了
<https://github.com/mksglu/context-mode>；上游项目声明 Elastic License 2.0，完整上游客户端 hooks/skills 未随本插件分发。
