# issue #14 Turn 内调试日志开关 · 手测证据(2026-09-09)

环境:隔离端口 127.0.0.1:7596 + 临时 data-dir(无真实网关,内置 mock 模型)。

证据:
1. 01_turn_debug_capturing.png——日志页第 4 页签「Turn 调试」:采集开关
   「采集中(新回合落 turn-debug.jsonl)」,日志框内可见一个完整回合:
   model_request(用户原文「调试探测消息」/model_id/tools_count=6)→
   model_response(回复原文/finish_reason/tokens/latency)→ turn_end
   (succeeded/latency_ms/tool_rounds)。
2. 初始态:开关「已关闭」+「(空——尚无日志)」;集成测试 t48 证默认关不落盘、
   开启后落盘、context-log.jsonl 不受污染。
3. 测毕:停进程、删临时目录、关标签页。
