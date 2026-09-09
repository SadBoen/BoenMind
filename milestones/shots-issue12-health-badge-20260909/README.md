# issue #12 前端健康徽标 · 手测证据(2026-09-09)

环境:隔离端口 127.0.0.1:7598 + 临时 data-dir;注册死网关 provider
(baseUrl=http://127.0.0.1:9/v1)设为当前模型,重启后 3 次 /v1 调用全败,
熔断器开闸(openai-routing: unavailable,连败 135+,30s 冷却)。

证据:
1. 01_provider_health_badge.png——顶栏 self-hosted 旁红色「⚠ 网关 1」徽标;
   tooltip(accessible name)=「模型网关熔断(连续失败开闸,冷却后半开探测):
   openai-routing: unavailable(连败 142,冷却至 2026-09-09T01:41:25Z)」。
2. 健康空态(首次启动 {health:[]})徽标不渲染(JobsBadge 同款零打扰惯例)。
3. 测毕:停进程、删临时目录、关标签页。
