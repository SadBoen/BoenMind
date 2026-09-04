# context_inspector: BoenMind 官方大模型交互透视与诊断 MCP 插件

> 插件标识: `context_inspector`
> 插件形态: 独立编译的外部 MCP 可执行文件 (Stdio JSON-RPC 2.0，基于 MCP 2024-11-05 标准)
> 源码目录: `plugins/mcp/context-inspector/`
> 适用平台: 跨平台 (Windows `.exe` / Linux x86_64 单二进制)

---

## 一、插件定位与边界

1. **绝对只读，零状态写入，绝不篡改数据**：
   - 本插件仅以只读方式读取 BoenMind 数据目录下的追加日志 `context-log.jsonl` 与配置表 `config/model.json`；
   - 严禁任何写文件操作；上下文滚动压缩与会话裁剪归后续专用压缩插件负责。

2. **为人类提供白话配方，为 Agent 提供自省工具**：
   - 作为独立 MCP Server，它不仅可以被 Web UI 挂载探活，更可以作为工具直接下发给自治 Agent，让模型能够自我查看当前 Prompt 构成、检查上下文水位与诊断 Token 激增。

---

## 二、MCP 工具契约清单 (全部带 readOnlyHint)

本插件对外暴露 4 个只读直通工具：

| 工具名称 | 作用说明 | 关键入参 | 关键出参 |
|:---|:---|:---|:---|
| `context_inspect_snapshot` | 深度拆解单次模型调用的 Prompt 配方与真实 Token 水位 | `session_id` (可选), `seq` (可选) | `metrics` (进出/思考/缓存/TTFT/窗口余量), `recipe` (人设/技能/目录/工具/历史轮次/当前输入) |
| `context_diagnose_spikes` | 多轮历史 Token 暴增与刺客诊断 | `session_id` (必填), `threshold_diff`, `threshold_ratio` | `timeline` (各轮次 Token 增量比对与异常标记) |
| `context_track_file_effects` | 本地工程文件副作用追踪 | `session_id` (必填) | `files` (文件路径、最终操作行为 read/write/edit/exec 与调用详情) |
| `context_search_history` | 跨会话搜索历史上下文快照与交互记录 | `query` (必填), `limit` (默认 20) | `hits` (匹配的快照与事件记录) |

---

## 三、命令行参数与自描述规范

- `--self-describe`：向标准输出打印一行紧凑的自描述 JSON（符合 BoenMind 插件中心扫描规范），退出码为 0，不启动 stdio 循环。
- `--data-dir <path>`：显式指定 BoenMind 数据目录；未指定时自动回退读取环境变量 `BOEN_DATA_DIR` 或系统默认路径。

---

## 四、构建与测试

```bash
# 语法与代码风格检查
cargo check --manifest-path plugins/mcp/context-inspector/Cargo.toml
cargo clippy --manifest-path plugins/mcp/context-inspector/Cargo.toml --all-targets -- -D warnings

# 单元与端到端集成测试 (验证 stdio 管道、握手、自描述与工具调用)
cargo test --manifest-path plugins/mcp/context-inspector/Cargo.toml

# Release 二进制构建 (开启 LTO 与 Strip 优化)
cargo build --release --manifest-path plugins/mcp/context-inspector/Cargo.toml
```
