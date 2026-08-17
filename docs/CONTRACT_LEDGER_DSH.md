# CONTRACT_LEDGER_DSH —— dsh 前端契约台账（骨架 v0）

> 状态：**骨架**（M2 前置产物，开工前必须填满）。
> 用途：实现清单 + 验收标准（v2 计划 §3.2/§3.4/§3.5）。来源：dsh `packages/` 源码，**逐条提取，逐条勾销**（勾销标准 = conformance harness 双后端 wire 轨迹 diff 一致）。
> 基线：`@deepseek-ai/dsh@0.1.0-rc.6`（2026-08-17 快照，与前端锁死同版本）。

## 1. 传输面（9 面 + 双栅栏）

| # | 面 | 形状 | 提取源（dsh 源码路径） | 状态 |
|---|---|---|---|---|
| 1 | HTTP POST /api/<channel>/<endpoint> | RPC 信封：client-request + rpcId + method + payload → server-response | packages/client/connection/src/client/rpc.ts | 待提取 |
| 2 | WS /api/events.mux | 宿主→浏览器 下行 MuxFrame（9 种）；上行 close(1008) | packages/client/connection/src/websocket-downlink.ts | 待提取 |
| 3 | WS /api/events.host | 宿主→浏览器 下行 HostFrame（9 种） | 同上 | 待提取 |
| 4 | 静态 SPA | 兜底 200 / 405 / 403 / octet-stream | packages/host/frontend-static/src | 待提取 |
| 5 | /plugins/<id>/client.js | __ModuleLoader__ 注册 | packages/client/modules | 待提取 |
| 6 | boot 3 槽 | __DSH_BOOT__ / __ModuleLoader__ / __DSH_MODULES__ | packages/client/modules | 待提取 |
| 7 | POST /api/respond | 审批/提问应答上行 | packages/client/connection/src/fetch/handler.ts | 待提取 |
| 8 | GET /api/session.export | 会话日志 ZIP 下载 | 同上 | 待提取 |
| 9 | SSE 备选 + /plugins/events HMR | 非 WS 环境的 SSE；插件 HMR 事件 | packages/client/connection + client/hmr | 待提取 |

**栅栏**：
- [ ] Host/Origin 栅栏（api-request-trust.ts）：loopback + --trusted-host 语义
- [ ] 16 个特权方法 loopback-pin（PRIVILEGED_METHODS：settings.* / credentials.* / agentPreset.* / host.pickDirectory / host.openPath / llm.discoverModels 等）——即使 LAN 部署也强制 loopback

## 2. RPC 方法面（55 方法 + 6 宿主概念）

- [ ] workspace / goals / skills / agentPresets / subagent / jobs 六概念各自方法集
- [ ] llm.*（providers/models/discoverModels 例外）
- [ ] session.*（create/queue/export…）
- [ ] settings.*（describe/update/replace/mutate + revision 冲突 + secret 脱敏）
- [ ] credentials.*（describe/set/unset）
- [ ] 其余方法逐条列全

## 3. 事件词汇（三层）

- [ ] wire 层：MuxFrame 9 + HostFrame 9 + SessionEvent 信封（wide-data + ignorable）
- [ ] 持久化层：SessionEvent 46 种（core 14 + 插件扩展 32）
- [ ] 扩展槽：session/projection、host/remote-event

## 4. 语义细节（行为逐字对齐）

- [ ] SPA 兜底 200 / 非 GET 405 / 路径越界 403 / 未知扩展 octet-stream
- [ ] rpcId 回显校验
- [ ] WS 上行拒绝 close(1008/426)
- [ ] 事件序与 seq 语义
- [ ] 断连恢复语义

## 5. 验收（conformance harness）

- [ ] wire 轨迹录制：dsh Node 后端 + 同一前端 → 请求/响应轨迹
- [ ] Rust 兼容层重放同一轨迹 → diff 一致
- [ ] 皮肤/第三方插件 UI 不改即用
