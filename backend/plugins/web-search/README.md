# Web Search —— BoenMind 搜索增强插件

多源聚合搜索 + 网页正文提取。自研实现，吸收 Hermes web-multisearch 的聚合思路
（并行扇出 / URL 规范化去重 / 多源交叉验证标注 / 单源失败隔离），不复制其代码；
在此之上补强：免费源用量管理与自动切换、结果缓存、429 退避、内容级转载去重。

## 能力

| 工具 | 说明 |
|---|---|
| `web_search(query, mode?, limit?, fresh?)` | 并行调用多个搜索源，合并去重后按「多源交叉验证强度」排序，description 带 `[源|源]` 标注；返回 meta（各源成功/失败/额度状态/耗时） |
| `web_fetch(url)` | 读取单个网页正文（jina Reader → markdown），截断 8K 摘要；仅 https + 拒绝内网/本地地址（SSRF 防护） |

## 免费源策略（穷人的优雅）

- **Jina**：s.jina.ai 搜索 + r.jina.ai 提取，免费 10M tokens（一次性，与提取共用额度）
- **Tavily**：免费 1000 次/月（月度重置）
- **Serper**：Google SERP，试用 2500 次一次性（仅 `mode=deep` 档使用）

用量记录在 `~/.boenmind/web-search/quota.json`，三通道探测额度：
HTTP 429 / 错误体额度信息 / `x-ratelimit-remaining` 响应头。耗尽自动跳过该源，
按「剩余额度比例 + 今日调用次数」加权选源平均使用；全耗尽时返回降级提示。

## 配置

设置 → 插件 → Web Search（schema 驱动动态表单）：

- `search.mode`：默认档位（quick = 仅免费源 / deep = 含付费源）
- `search.cacheTtlSeconds`：同查询结果缓存 TTL（JSONL，项目级 `.boenmind/web-search-cache.jsonl`）
- `sources.*.enabled` / `sources.*.apiKey`：各源开关与密钥（密钥掩码存储，空 = 清除）

配置存于用户级 `~/.boenmind/plugin-settings/web-search.json`，**新对话会话生效**。

## 架构要点（沙箱事实）

- 网络走宿主 hostcall `pi.http`（仅 GET/POST，TLS 强制）；exec 被插件政策拒绝，不依赖 bash/curl
- 落盘必须 `pi.tool("write")`（node:fs 写是 VFS 虚拟层不落盘）；读可直接 node:fs
- 输出体积控制：单条摘要截断 300 字符，配合 ctx-compactor 修剪不撑爆上下文
- 刻意不做：不自动改写/分解查询词（控制权留给模型，工具描述里指导拆多个查询）

## 开发期验证

```bash
node scripts/web-search-verify/test-websearch.mjs   # 46 项纯函数单元验证（Node 24 原生跑 TS）
cargo test -p bm-core -p bm-server                   # 后端设置 API 测试
```

## 手动安装（非内置分发时）

```bash
cp -r backend/plugins/web-search ~/.boenmind/extensions/
# 设置 → 插件 → 启用 web-search
```
