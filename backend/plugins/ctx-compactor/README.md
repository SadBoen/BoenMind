# ctx-compactor —— BoenMind 上下文压缩补强插件

自研实现（仅借鉴 Hermes / context-mode 的行为思路，不复制任何第三方代码）。
随 BoenMind 仓库分发，首次启动时预装到 `~/.boenmind/extensions/ctx-compactor/`，
在"设置 → 插件"中启用后，聊天应用内即可使用。

## 能力

| 工具 / 事件 | 说明 |
|---|---|
| `ctx_execute` | 沙箱内执行 JavaScript（Think in Code）。只回 console 输出与结果摘要，大输出不污染上下文。 |
| `tool_result` 修剪 | 内置工具输出超过阈值（默认 200 字符）时，进模型前替换为自描述占位符（含检索 key 与摘要），原文经秘密扫描过滤后落库。 |
| `ctx_search` | 检索被修剪的工具输出索引（简易词频打分）。占位符里的 key 可精确定位。 |
| `session_before_compact` | 自动压缩触发时输出观测日志（tokensBefore），不干预压缩。 |

## 配置

项目级配置文件 `<cwd>/.boenmind/ctx-compactor.json`（不存在时用默认值）：

```json
{
  "trimEnabled": true,
  "trimThreshold": 200,
  "placeholderHead": 300,
  "maxIndexBytes": 8388608,
  "indexDirName": ".boenmind/ctx-index"
}
```

- `trimThreshold`：修剪阈值（字符）。输出超过则修剪。
- `placeholderHead`：占位符里保留原文前 N 字符作摘要。
- `maxIndexBytes`：索引文件超过该大小后轮转（`entries-<ts>.jsonl`）。
- `indexDirName`：索引目录（相对 cwd；按项目天然分桶，新项目干净起点）。

## 设计说明（与计划的差异及原因）

- **事件落库放在 `tool_result` 而非 `ToolExecutionEnd`**：Phase 0 验证发现
  `tool_execution_*` 事件只在 CLI/rpc 路径派发，SDK 路径（BoenMind 聊天应用）
  收不到；且 `ToolExecutionEnd` 携带的是修剪**后**的内容，原文只有
  `tool_result` 处理器里拿得到。
- **索引写在项目 cwd 内**（`.boenmind/ctx-index/`）：扩展默认只有 cwd 写权限，
  无需额外 capability scope；按项目分桶语义天然满足。
- **修剪会写入会话存储**（Phase 0 验证：`tool_result` 修改后的内容进入
  `ToolResultMessage` 持久化）→ 占位符自描述（含摘要 + 检索 key），原文在
  索引可查，历史回放不丢信息。

## 限制

- `ctx_execute` 仅支持 `js`（QuickJS 内执行）；同步死循环无 interrupt 预算
  兜底，会被工具 60s 超时终止。
- 索引为纯文本 JSONL + 扫描检索，适合中规模项目；超大索引建议调大
  `maxIndexBytes` 或清理 `entries-*.jsonl`。
- 索引目录会出现在项目 git 工作区，建议加入 `.gitignore`（`.boenmind/`）。
