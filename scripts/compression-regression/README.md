# 上下文压缩回归测试工具

30 轮固定对话序列 × 三组配置的 A/B/C 对比测试，用于验证上下文压缩功能（水线注入 + ctx-compactor 插件）的回归表现：token 是否仍然省、质量是否仍然不降。

## 背景结论（2026-08-12 首次实测）

| 指标 | A（pi 默认） | C（只水线） | B（水线+插件） |
|---|---|---|---|
| 累计发送量 | 2288.6K | 1565.3K | 1169.4K |
| 峰值上下文 | 132.9K | 82.6K | 67.0K |
| 记忆题（15 问） | 15/15 | 15/15 | 15/15 |

完整方案（B）比装前（A）省 48.9%（水线贡献 31.6% + 修剪贡献 25.3%），质量持平。
**回归基准：B 组累计发送量应显著低于 A 组（~-50%），记忆题两组全对。**

## 文件

- `rounds.mjs` — 30 轮固定任务序列（含记忆检验题，可配 `COMPARE_WORKDIR` 覆盖工作子目录）
- `run-compare.mjs` — 驱动：建会话 → 逐轮 POST /api/chat → 收集回复（支持 `--resume` 断点续跑）
- `analyze.mjs` — 解析服务端 `bm.prompt_usage` 日志 → 每轮上下文曲线/累计/压缩触发检测

## 组别配置（改 `~/.boenmind/config.toml` 后重启 bm-server）

| 组 | enabled_plugins | [compaction] | 意义 |
|---|---|---|---|
| A | 不含 ctx-compactor | `enabled = false` | 装前基线（pi 默认压缩） |
| C | 不含 ctx-compactor | 不写（默认开启） | 只测内核水线 |
| B（默认回归目标） | 含 ctx-compactor | 不写（默认开启） | 完整功能 |

## 用法

```bash
# 1. 确认 bm-server 可访问；建议独立端口 + 关闭 HTTP 超时（长上下文下 MiniMax 首 token 可能超 60s）
RUST_LOG=bm_server=info PI_HTTP_REQUEST_TIMEOUT_SECS=0 BOENMIND_PORT=17322 ./target/debug/bm-server > server-B.log 2>&1 &

# 2. 切到对应组配置（见上表），跑 30 轮（耗时 10-40 分钟/组，真实 API 费用）
OUT_DIR=out node run-compare.mjs --group B --base http://127.0.0.1:17322

# 3. 分析（需要服务端日志文件与 out/ 同目录）
node analyze.mjs --group B --log server-B.log

# 4. 质量检查：记忆题轮次（r21-24/r29）回复应包含埋入的事实点
#    （2026-12-15 / 37 项监控 / 机柜 128→160 / FAQ 10+8 条 / 预算 480 万 / 800G 等）
```

跑完把 `out/` 与 `server-*.log` 移到归档目录保留（如 `artifacts/2026-08-12/`）。

## 注意事项

- **费用**：每组 30 轮约 100-250 万 tokens 输入（MiniMax 缓存命中部分折扣计费），A 组最贵。
- **公平性**：三组必须用完全相同的序列；工作区 `compression-test/` 需在每组开始前清空，
  否则后续组会读到前面组残留的文件（行为与 token 曲线都会偏离，2026-08-12 实测有此混杂）。
- **日志解析**：`analyze.mjs` 依赖服务端 `bm.prompt_usage` 日志（`bm-server` 的 chat.rs 输出，
  tracing `info` 级）；日志需与 out 同目录或 `--log` 指定。轮次 `ts` 为本地时间、日志为 UTC，
  分析脚本已按本地时区（GMT+8）换算，其他时区需调整。
- **断点续跑**：中途失败（如 API 超时）用 `--resume` 继续；重启 bm-server 后 MiniMax 缓存
  命中口径会变化（曲线出现台阶），报告中注明即可。
