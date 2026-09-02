# web-multisearch-rs — 聚合搜索 MCP Server(Rust 版)

`web-multisearch`(Python 版)的 Rust 重写:**单 exe、零运行时依赖**(release 构建约 5MB,拷到目标机器即可跑)。

## 工具面(与 Python 版等价)

| 工具 | 源 | 说明 |
|---|---|---|
| `web_search_lite` | searxng + ddgs + jina + marginalia(全免费源) | 日常搜索优先 |
| `web_search_all` | 全部 12 源 | 最大覆盖/交叉验证 |

核心算法逐行对齐 Python 版:RRF 融合(k=60)+ CJK 二字 bigram 镜像合并(Jaccard≥0.9)+ URL 规范化去跟踪参数 + 逗号多 Key 401/403/429 自动轮换 + 每源 limit+2 / 全局 25s 兜底丢慢源 / 源耗时遥测。工具结果同时给 `content[].text`(pretty JSON,任何 MCP 客户端可用)与 `structuredContent`(BoenMind 客户端直读对象)。

两个工具均标注 `readOnlyHint` → BoenMind 侧映射 approval=not-required(联网=只读默认直通,用户裁定 2026-09-02)。

## 已知差异(vs Python 版)

1. **ddgs 源是尽力而为**:Python 版靠 ddgs 库(底层 primp 浏览器 TLS 指纹伪装)通过 DDG 的反爬;Rust 侧 reqwest(rustls/Schannel 指纹)与系统 curl 均会被 DDG 发人机验证页(2026-09-02 实测)。本版 ddgs 走「系统 curl → reqwest 直连」降级链,单源失败不影响聚合整体,但**指纹问题根治需集成 wreq(=primp 的 Rust 本体),其 Windows 构建链(LLVM/CMake/NASM)待拍板**(BoenMind 仓 BACKLOG 有登记)。
2. **jina 的 extract(r.jina.ai 正文抓取)未迁**:Python 版也未暴露为 MCP 工具,Hermes 时代的内部能力,BoenMind 工具面用不到。
3. Hermes 的 `~/.hermes/.env` 配置层不迁(BoenMind 场景无此层)。

## 配置

```bash
web-multisearch.exe --config <path-to-json>
```

配置文件即 BoenMind 的 `config/mcp-web_multisearch.json`(设置页「MCP 配置」表单写入)。**按 mtime 热读**:改 Key 下一次搜索立即生效,无需重启。可配置项与 Python 版 manifest.json 的 config_schema 一致(searxng_url、9 家 API Key、default_limit)。Key 也可用同名环境变量兜底(SERPER_API_KEY 等)。

## 构建

```bash
cargo build --release   # 产物 target/release/web-multisearch.exe
cargo test              # 26 个单测(融合/轮换/协议/解析)
```

## BoenMind 接线示例(mcp.json 条目)

```json
{
  "name": "web_multisearch",
  "transport": "stdio",
  "command": "D:\\96_CoderWorld\\boenmind-mcp-servers\\web-multisearch-rs\\target\\release\\web-multisearch.exe",
  "args": ["--config", "C:\\Users\\Boen\\AppData\\Roaming\\boenmind\\config\\mcp-web_multisearch.json"],
  "trust": "explicit-config",
  "tool_timeout_ms": 30000,
  "restart_limit": 3
}
```

`--config` 指向数据目录的 `config/mcp-web_multisearch.json`——这是对 Python 版接线的修正:原接线从未把配置文件传给子进程,设置页写的 Key 实际无人读取;Rust 版热读该文件,设置页「改 Key 立即生效」才真正成立。
