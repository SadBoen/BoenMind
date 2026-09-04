# context-inspector: BoenMind 官方大模型交互透视与诊断插件

> 插件标识: `context-inspector`
> 插件类型: 外部扩展分析插件 (External Diagnostic Plugin)
> 访问入口: 主界面「上下文」双栏透视大盘 + `/admin/context` 数据总线

---

## 插件职责与边界

1. **绝对只读，零数据篡改，绝不压缩**：
   * 专注于真实上下文快照的可视化透视与诊断分析；
   * 上下文滚动摘要与压缩由后续专用 MCP 压缩插件负责。

2. **核心特性矩阵 (全面超越普通日志，对标 DSH & Pi-Web)**：
   * **真实窗口水位与余量倒计时 (Context Headroom)**：动态按当前模型（如 128k / 64k / 32k）计算剩余安全 Token 与预警；
   * **深度思考链独立分账 (Reasoning Tokens)**：将推理思考消耗与答复正文分离展示，算清思考代价；
   * **工程文件读写副作用追踪 (File I/O Tracker)**：对标 Pi-Web，自动提取当轮被 `fs.*` / `system.exec` 涉及读写的文件清单；
   * **输出生成速率 (Tokens/s) 与性能测量**：首字延迟与生成速率全透明；
   * **Token 暴增诊断 (Spike Diagnostics)**：多轮对比，一眼揪出引起上下文暴增的工具调用；
   * **全域双栏联动交互**：左侧人性化卡片 + 右侧专家代码/报文联动平滑滚动定位与高亮加深；
   * **一键脱敏导出 (Scrubbed Snapshot Export)**：一键导出 JSON 报文供离线分析。
