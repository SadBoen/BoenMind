# pdf-omni — PDF 智能解析插件（Rust 核心 + TS 薄壳）

把 PDF（本地文件或公网 URL）解析为高保真 Markdown：版面/表格/公式全保留，
面向论文、财报、合同、扫描件等复杂文档。

> 原理吸收自 Hermes 私有仓库 `hermes-plugin-pdf-omni`（Python 版，2026-08 实测
> 结论），**重写为 Rust + TS 双层架构**，服务商 API 不变。不是代码照搬。

## 引擎分工

| 引擎 | 角色 | 免费额度 | 单文件限制 | 备注 |
|---|---|---|---|---|
| MinerU Precision | 主力 | 1000 页/天(优先) | ≤200MB / ≤200页 | 中文最强，公式/表格最佳 |
| LlamaParse | 增强/交叉验证 | 1万 credits/月 | ≤300MB(兜底值) | agentic=10 credits/页 |

- `verify=True` 时按 [mineru→llamaparse] 优先级选另一个引擎跑第二遍，
  输出一致性报告（Jaccard 相似度 + 段落/表格/公式数差异）。
- `cascade=True` 时 MinerU 解析后把表格/公式/图表页自动交给 LlamaParse 增强
  （三级分桶，见下）。
- **LlamaParse 档位(tier)**：`fast`(1 credit, **不输出 Markdown,勿用**) /
  `cost_effective`(3) / `agentic`(10, **默认**,实测质量最优且更快) /
  `agentic_plus`(45)。换档位重跑会 bust 48h 缓存重新计费。
- `refine`（默认 true）：mineru-refine 式后处理（伪标题/页眉页脚/空表/
  残留标记/叠字修复，规则见 `crates/bm-server/src/pdf_omni/refine.rs`）。
- Doc2X 已封禁（2026-08），未移植。

## 级联增强（cascade，三级分桶）

`cascade=True`（仅 `engine=mineru` 本地文件）：MinerU 先做第一遍识别，问题内容
**按类型/尺寸三级分桶**交给 LlamaParse：

| 分桶 | 处理方式 | 依据（2026-08-09 实测） |
|---|---|---|
| 表格 + 小图(bbox < 25% 页) | 渲染图**按原尺寸拼进 A4 画布**（Rust image crate）| 原尺寸 = 100% 细节，mermaid 可触发；一张 A4 ≈ 1 页计费 |
| 大图/图表页(≥25% 页) | 整页单独提交 + `specialized_chart_parsing=agentic` | 保细节触发 mermaid |
| 纯公式页(无表无图) | 整页 2×2 网格拼（lopdf 矢量拼页）| 公式依赖上下文；拼页 97% 保留 |

credits 账本 = ⌈拼图组数⌉×费率 + 大图页数×费率 + ⌈公式页/4⌉×费率。
实测（Hermes 版）：8 页论文 → 2 张 A4 拼图 + 1 页公式网格 = 30 credits（全文 50），
省 40%。

## 多 Key 串行使用（先用完一把再切下一把）

LlamaParse 支持两把 API key（设置页 `LlamaParse API Key` / `API Key 2`），按
**串行策略**分配（2026-08 用户决策：交替轮换会制造"同源多账号高频切换"的
可疑画像，易被风控识别；串行更接近个人正常使用多个免费账号的行为）：

- 每把 key 有安全预算 `budget_per_key`（默认 **9500 credits**，1 万额度的 95%）
- 每个任务完成后按 `页数×费率` **本地累加**用量（`~/.boenmind/pdf-omni/budget.json`，
  key 以哈希标识不落明文）
- **先用 key1 到预算线，再主动切 key2**，全部达预算才报错
- **任务前精确检查**：选 key 时要求 `用量 + 本次任务估算 ≤ 9500`，大任务自动
  切下一把或报错，不会单任务越线撞 402
- 402 仅作意外兜底（如其他端并发消耗），触发后该 key 标记到预算线

## 大文件处理

- MinerU 单文件 >200 页：lopdf 按 190 页切块，逐块上传解析后按顺序合并 Markdown。
- 超大小限制（200MB/300MB）直接报错提示，不硬传。
- 上传一律流式（reqwest 文件对象直传），大文件不爆内存。
- 轮询有 600s 超时上限，超时报 TimeoutError。

## 架构

```
backend/
├── crates/bm-server/src/pdf_omni/     # Rust 核心（全部重活）
│   ├── mod.rs         # parse_pdf_any 编排（引擎选择/verify/cascade/refine/落盘）
│   ├── mineru.rs      # MinerU 客户端（签名上传/URL/轮询/zip 解压）
│   ├── llamaparse.rs  # LlamaParse 客户端（multipart/轮询/档位）+ 级联三级分桶
│   ├── pdf_ops.rs     # lopdf 封装：页数/切分/提取/2×2 拼页 + image A4 拼接
│   ├── verify.rs      # 交叉验证（Jaccard + 统计差异 + LCS ratio）
│   ├── refine.rs      # mineru-refine 式后处理（5 类规则）
│   └── budget.rs      # 多 key 串行预算账本（持久化 ~/.boenmind/pdf-omni/budget.json）
├── crates/bm-server/src/routes/pdf_omni.rs  # POST /api/plugins/pdf-omni/parse（loopback）
└── backend/plugins/pdf-omni/           # TS 薄壳插件（工具注册 + 设置页 + 透传）
    ├── extension.json  # manifest + settings（secret API keys）+ testSources
    └── index.ts        # registerTool(parse_pdf) → POST loopback 端点
```

- 插件沙箱（QuickJS）能力有限（npm 包不可导入、pi.http 仅 GET/POST）→ 全部
  重活下沉 Rust 宿主端点；TS 壳只做 schema/参数校验/透传（~120 行）
- 端点经 `PI_HTTP_ALLOW_LOOPBACK=1`（bm-server 启动时设置）供插件 loopback 访问；
  全局 auth_middleware（BOENMIND_TOKEN）覆盖；本地文件路径用
  `workspace::safe_join` 校验（拒绝越界/`..`）
- API keys 由端点从插件设置文件读取（单源，设置页写入），不在 loopback 上传

## 设置

插件设置页填写（extension.json settings secret 字段）：
- `sources.mineru.apiKey` — MinerU Precision API token（mineru.net）
- `sources.llamaparse.apiKey` — LlamaIndex 账号 key（1 万 credits/月）
- `sources.llamaparse.apiKey2` — 第二账号 key（可选，预算轮换）

## 用法（agent 侧）

```
parse_pdf(file="/path/to/paper.pdf")                          # MinerU 默认
parse_pdf(file="/path/to/paper.pdf", verify=True)             # + 第二引擎交叉验证
parse_pdf(file="/path/to/scan.pdf", is_ocr=True)              # 扫描件强制 OCR
parse_pdf(file="https://example.com/a.pdf")                   # 公网 URL(仅 MinerU)
parse_pdf(file="x.pdf", cascade=True)                         # 级联增强(省 credits)
parse_pdf(file="paper_en.pdf", engine="llamaparse", tier="agentic")  # 指定引擎/档位
```

返回 JSON：`markdown`（≤200K 截断）、`markdown_path`（落盘路径）、`stats`、
`refine_report`（后处理报告）、`verify_report`（交叉验证报告）、
`cascade_report`（级联报告，含 credits 账本）、`elapsed_seconds`。

## 测试

```bash
cargo test -p bm-server pdf_omni   # 25 个单测：pdf_ops 拼页/切分/A4 拼接、
                                   # budget 串行预算、verify Jaccard、refine 规则
```

## 与 Hermes 版的差异（吸收 vs 重写）

| Hermes（Python） | BoenMind（Rust/TS） |
|---|---|
| requests/Pillow/pypdf | reqwest/lopdf/zip/image（Rust 生态）|
| API keys 读 ~/.hermes/.env | 插件设置页 secret 字段 |
| 插件内阻塞执行 | Rust 宿主端点（同进程，无沙箱限制）|
| 无后处理 | mineru-refine 式后处理（refine.rs，默认开启）|
| — | 预算账本持久化（Hermes 版仅进程内）|
