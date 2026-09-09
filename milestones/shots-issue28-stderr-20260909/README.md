# issue #28 MCP stderr 采集 · 手测证据(2026-09-09)

环境:隔离端口 127.0.0.1:7599 + 临时 data-dir(tmp-mcp-stderr-test)+ 噪声插件
(noisy = mini_mcp.py 夹具头部注入 stderr 诊断行)。

证据(页面可见内容;截图通道本次会话级坏死,PITFALLS 已录怪癖,按规程以可见内容为证):

1. `GET /admin/mcp/stderr/noisy?lines=50` 返回:
   `{"lines":[{"generation":1,"text":"── 第 1 代子进程启动 ──"},{"generation":1,"text":"noisy-stderr-diagnostic g1"}],"name":"noisy","ok":true}`
2. 插件页 noisy 行新增「日志」按钮 → 弹窗可见内容(inner):
   [g1] ── 第 1 代子进程启动 ──
   [g1] noisy-stderr-diagnostic g1
3. 「刷新」按钮再取仍正确渲染;关闭后页面无残留。
4. 测毕:停进程、删临时目录、关标签页。
